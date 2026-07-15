//! The Crow v0 tool subsystem.
//!
//! Every capability the agent loop can invoke — reading a file, editing
//! text, running a shell — implements the [`Tool`] trait and is held by
//! a [`ToolRegistry`]. The agent loop does not call tools directly; it
//! builds a [`ToolCall`] from the model output, hands it to
//! [`execute_tool_call`], and records the resulting [`ToolOutcome`].
//!
//! ## Responsibilities
//!
//! - **Schema declaration.** Each tool exposes a [`schemars::Schema`]
//!   describing its arguments. The registry emits those schemas to the
//!   model so it can generate well-formed JSON.
//! - **Argument validation.** The registry validates the model's JSON
//!   against the schema *before* dispatch, so a tool never has to
//!   defend against malformed input.
//! - **Event stream.** Every call publishes a `ToolStarted` event at
//!   dispatch and either the tool itself or the wrapper publishes a
//!   `ToolFinished` event with the outcome.
//! - **Cancellation.** Tools receive a [`CancellationToken`] so a
//!   supervisor (or the user) can abort a slow call without leaking
//!   processes or file handles.
//!
//! ## `ToolEventSink` backpressure contract
//!
//! The sender is bounded (Decision 06, capacity 256). When the channel
//! is full, [`mpsc::Sender::send`] blocks rather than dropping. Tools
//! MUST call `send().await` for terminal events (`ToolStarted`,
//! `ToolFinished`) — never `try_send` — so the consumer always sees
//! them. Streaming chunks may use `try_send` and drop on backpressure;
//! the consumer counts drops and emits a summary.

pub mod bash;
pub mod edit;
pub mod path;
pub mod read;
pub mod write;

pub use bash::BashTool;
pub use edit::EditTool;
pub use read::ReadTool;
pub use write::WriteTool;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use schemars::schema::Schema;
use serde::Serialize;
use serde_json::Value;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::event::{AgentEvent, ErrorCode, ToolOutcome as EventToolOutcome};
use crate::ids::ToolCallId;

/// Per-invocation context handed to a [`Tool`] when it runs.
///
/// Cheap to copy (one `PathBuf`, two scalars) so we pass it by value
/// rather than threading an `Arc` through every call site.
#[derive(Debug, Clone)]
pub struct ToolContext {
    /// Absolute path that bounds every filesystem tool. A tool MUST NOT
    /// read or write anything outside this root (spec §4).
    pub project_root: PathBuf,
    /// Soft cap on the size of a tool's returned `output` string, in
    /// bytes. Tools that exceed this must set `truncated: true` on
    /// their outcome.
    pub max_output_bytes: usize,
    /// Per-command wall-clock timeout for shell-style tools. The read
    /// tool does not honour this directly — its I/O is bounded by the
    /// file size — but it is forwarded for tools that stream.
    pub command_timeout: Duration,
}

/// Sink for [`AgentEvent`] values produced while a tool runs.
///
/// The capacity and backpressure semantics are documented at the
/// module level; tools must respect them.
pub type ToolEventSink = mpsc::Sender<AgentEvent>;

/// Errors a tool can return. Each variant maps to a stable error code
/// the model can reason about (see the wrapper in
/// [`execute_tool_call`]).
#[derive(Debug, thiserror::Error)]
#[allow(missing_debug_implementations)]
pub enum ToolError {
    #[error("invalid arguments: {0}")]
    InvalidArgs(String),

    #[error("path escapes project root: {0}")]
    PathEscape(PathBuf),

    #[error("path is not a regular file: {0}")]
    NotAFile(PathBuf),

    #[error("file is binary: {0}")]
    Binary(PathBuf),

    #[error("read failed: {0}")]
    Io(#[from] std::io::Error),

    #[error("cancelled")]
    Cancelled,

    #[error("output too large ({actual} > {limit} bytes)")]
    TooLarge { actual: u64, limit: usize },
}

/// A tool's return value. `Ok` means the tool ran and produced output;
/// the wrapper converts the `Err` arm into a structured
/// `ToolOutcome::Error` rather than aborting the loop.
pub type ToolResult = Result<ToolOutcome, ToolError>;

/// What a tool produced. Persisted on a `SessionEntry::ToolFinished`
/// and replayed on resume, so the variants here are part of the
/// durable schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolOutcome {
    Success {
        output: String,
        truncated: bool,
    },
    Error {
        code: ErrorCode,
        message: String,
        truncated: bool,
    },
}

/// A single tool invocation as parsed from the model output.
#[derive(Debug, Clone)]
pub struct ToolCall {
    pub call_id: ToolCallId,
    pub name: String,
    pub args: Value,
}

/// Object-safe tool contract. Implementors are boxed behind
/// `Arc<dyn Tool>` in the registry.
///
/// `Send + Sync` is required so tools can run from any Tokio worker.
/// The trait has no generic methods and no `Self` in return positions,
/// so it remains object-safe and can back a dynamic dispatch table.
#[async_trait]
pub trait Tool: Send + Sync {
    /// Stable, machine-readable name used by the model to address this
    /// tool (e.g. `"read"`). Must be unique within a registry.
    fn name(&self) -> &'static str;

    /// Human-readable description. Shown to the model alongside the
    /// schema so it knows when to invoke this tool.
    fn description(&self) -> &'static str;

    /// JSON Schema describing the tool's `args`. Generated from a
    /// `#[derive(JsonSchema)]` struct via
    /// [`schemars::schema_for!`](schemars::schema_for).
    fn schema(&self) -> Schema;

    /// Run the tool with the given (already schema-validated) args.
    ///
    /// `events` is a backpressured channel for streaming output;
    /// `cancel` fires when the supervisor wants to abort the tool.
    async fn execute(
        &self,
        args: Value,
        ctx: ToolContext,
        events: ToolEventSink,
        cancel: CancellationToken,
    ) -> ToolResult;
}

/// Holds the registered tools. Cheap to clone (just an `Arc` bump per
/// `ToolRegistry` itself is not `Clone`, but `get` returns an `Arc` so
/// callers can share tools without copying the registry).
#[derive(Default)]
#[allow(missing_debug_implementations)]
pub struct ToolRegistry {
    tools: HashMap<&'static str, Arc<dyn Tool>>,
}

impl ToolRegistry {
    /// Empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// Insert a tool under its `name()`. A second registration with
    /// the same name replaces the first; the registry does not
    /// deduplicate on purpose so callers can swap implementations in
    /// tests.
    pub fn register<T: Tool + 'static>(&mut self, tool: T) {
        self.tools.insert(tool.name(), Arc::new(tool));
    }

    /// Look up a tool by name. Returns an `Arc` so the caller can hold
    /// onto it without keeping the registry borrowed.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(name).cloned()
    }

    /// Names of every registered tool. Returned in lexicographic order
    /// so the model sees a stable tool list across turns and runs
    /// (deterministic schema payload, deterministic tool call logging).
    #[must_use]
    pub fn names(&self) -> Vec<&'static str> {
        let mut names: Vec<&'static str> = self.tools.keys().copied().collect();
        names.sort_unstable();
        names
    }

    /// Tool descriptors (name + description + schema) for every
    /// registered tool, in lexicographic name order. This is the shape
    /// the [`genai`] adapter consumes: each entry becomes a
    /// `genai::chat::Tool` declaration on the chat request.
    #[must_use]
    pub fn tool_specs(&self) -> Vec<ToolSpec> {
        let mut entries: Vec<(&'static str, &Arc<dyn Tool>)> =
            self.tools.iter().map(|(n, t)| (*n, t)).collect();
        entries.sort_unstable_by_key(|(name, _)| *name);
        entries
            .into_iter()
            .map(|(name, tool)| ToolSpec {
                name,
                description: tool.description(),
                schema: serde_json::to_value(tool.schema()).unwrap_or(Value::Null),
            })
            .collect()
    }

    /// Render every tool's schema into a JSON object keyed by tool
    /// name, in lexicographic name order. This is the payload sent to
    /// the model so it can generate arguments. If a schema somehow
    /// fails to serialise (it never should — `schemars` schemas are
    /// pure data) we emit `null` for that entry rather than panicking.
    #[must_use]
    pub fn schemas_json(&self) -> Value {
        let mut specs = self.tool_specs();
        // Already sorted by tool_specs; the BTreeMap insert below
        // preserves nothing because we use serde_json::Map, but the
        // explicit sort above keeps the contract honest.
        specs.sort_by(|a, b| a.name.cmp(b.name));
        let mut map = serde_json::Map::new();
        for spec in specs {
            map.insert(spec.name.to_string(), spec.schema);
        }
        Value::Object(map)
    }
}

/// One tool's name + description + JSON Schema, as the registry hands
/// them to the provider adapter. `description` is included so the
/// adapter does not have to thread the [`Tool`] trait through.
#[derive(Debug, Clone, Serialize)]
pub struct ToolSpec {
    /// Stable name (`"read"`, `"write"`, ...).
    pub name: &'static str,
    /// Human-readable description for the model.
    pub description: &'static str,
    /// JSON Schema for the tool's `args`.
    pub schema: Value,
}

/// Validate `args` against the JSON Schema produced by `schemars`.
///
/// We compile the schema once per call (cheap, sub-microsecond for the
/// shapes we use) and run [`jsonschema::JSONSchema::validate`]. On
/// failure we return a single string that names the first violation
/// and the JSON pointer into the instance, which is enough for the
/// model to fix its call.
fn validate_args(args: &Value, schema: &Schema) -> Result<(), String> {
    // `Schema` serialises to a JSON value; that is what
    // `JSONSchema::compile` expects.
    let schema_value =
        serde_json::to_value(schema).map_err(|e| format!("schema is not serialisable: {e}"))?;
    let compiled = jsonschema::JSONSchema::compile(&schema_value)
        .map_err(|e| format!("schema is not a valid JSON Schema: {e}"))?;
    if let Err(errors) = compiled.validate(args) {
        // compiled.validate() returns a Box<dyn Iterator<Item = ValidationError>>.
        // Take the first error's Display string; that's enough for the
        // model to fix its next attempt.
        let first = errors.into_iter().next();
        return Err(match first {
            Some(e) => e.to_string(),
            None => "arguments do not match the tool schema".to_string(),
        });
    }
    Ok(())
}

/// Dispatch a [`ToolCall`] through the registry and return a
/// `ToolOutcome`. This function is the only public entry point used
/// by the agent loop.
///
/// Order of operations:
/// 1. Look up the tool; missing tools short-circuit with
///    `ErrorCode("unknown_tool")`.
/// 2. Emit `ToolStarted` on the event sink. `send().await` blocks
///    under backpressure (Decision 06) so the consumer is guaranteed
///    to see it before any `ToolOutput` or `ToolFinished`.
/// 3. Validate the JSON args against the tool's schema. Invalid args
///    become `ErrorCode("invalid_args")`.
/// 4. Run the tool. Any `ToolError` becomes
///    `ErrorCode("tool_error")` with the variant's `Display` payload
///    as the message — the model gets a structured retryable signal
///    and the UI gets the human-readable string.
pub async fn execute_tool_call(
    reg: &ToolRegistry,
    call: &ToolCall,
    ctx: ToolContext,
    events: ToolEventSink,
    cancel: CancellationToken,
) -> ToolOutcome {
    let tool = match reg.get(&call.name) {
        Some(t) => t,
        None => {
            return ToolOutcome::Error {
                code: ErrorCode("unknown_tool".into()),
                message: format!("no tool named {}", call.name),
                truncated: false,
            };
        }
    };

    // Step 2: ToolStarted. Awaited send respects Decision 06.
    let _ = events
        .send(AgentEvent::ToolStarted {
            call_id: call.call_id,
            name: tool.name().to_string(),
            args: call.args.clone(),
        })
        .await;

    // Step 3: schema validation.
    if let Err(message) = validate_args(&call.args, &tool.schema()) {
        return ToolOutcome::Error {
            code: ErrorCode("invalid_args".into()),
            message,
            truncated: false,
        };
    }

    // Step 4: execute.
    let outcome = match tool
        .execute(call.args.clone(), ctx, events.clone(), cancel)
        .await
    {
        Ok(outcome) => outcome,
        Err(err) => ToolOutcome::Error {
            code: ErrorCode("tool_error".into()),
            message: err.to_string(),
            truncated: false,
        },
    };

    // Step 5: emit the terminal `ToolFinished` event on the same sink
    // so consumers see the outcome paired with the call. We map our
    // internal `ToolOutcome` to the event-layer variant by hand to
    // avoid an accidental type drift if one of them is refactored.
    let event_outcome = match &outcome {
        ToolOutcome::Success { output, truncated } => EventToolOutcome::Success {
            output: output.clone(),
            truncated: *truncated,
        },
        ToolOutcome::Error {
            code,
            message,
            truncated,
        } => EventToolOutcome::Error {
            code: code.clone(),
            message: message.clone(),
            truncated: *truncated,
        },
    };
    let _ = events
        .send(AgentEvent::ToolFinished {
            call_id: call.call_id,
            result: event_outcome,
        })
        .await;

    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{AgentEvent, ToolOutcome as EventToolOutcome};
    use crate::ids::new_id;
    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};
    use tokio::sync::mpsc;

    /// A trivial test tool that echoes its argument back as a
    /// string. Used by most registry tests.
    struct EchoTool;

    #[async_trait]
    impl Tool for EchoTool {
        fn name(&self) -> &'static str {
            "echo"
        }
        fn description(&self) -> &'static str {
            "Echoes its single `msg` argument back."
        }
        fn schema(&self) -> Schema {
            // Use the schema_for! macro then convert via JSON round-trip
            // since RootSchema -> Schema needs a different code path
            // depending on the schemars version.
            let root: schemars::schema::RootSchema = schemars::schema_for!(EchoArgs);
            serde_json::from_value(serde_json::to_value(&root.schema).unwrap())
                .expect("schema is serialisable")
        }
        async fn execute(
            &self,
            args: Value,
            _ctx: ToolContext,
            _events: ToolEventSink,
            _cancel: CancellationToken,
        ) -> ToolResult {
            let parsed: EchoArgs =
                serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs(e.to_string()))?;
            Ok(ToolOutcome::Success {
                output: parsed.msg,
                truncated: false,
            })
        }
    }

    #[derive(Serialize, Deserialize, JsonSchema)]
    struct EchoArgs {
        msg: String,
    }

    /// A tool that fails unconditionally. Lets us assert the wrapper
    /// maps `Err` to a structured `ToolOutcome::Error`.
    struct BoomTool;

    #[async_trait]
    impl Tool for BoomTool {
        fn name(&self) -> &'static str {
            "boom"
        }
        fn description(&self) -> &'static str {
            "Always fails."
        }
        fn schema(&self) -> Schema {
            // Use the schema_for! macro then convert via JSON round-trip
            // since RootSchema -> Schema needs a different code path
            // depending on the schemars version.
            let root: schemars::schema::RootSchema = schemars::schema_for!(EmptyArgs);
            serde_json::from_value(serde_json::to_value(&root.schema).unwrap())
                .expect("schema is serialisable")
        }
        async fn execute(
            &self,
            _args: Value,
            _ctx: ToolContext,
            _events: ToolEventSink,
            _cancel: CancellationToken,
        ) -> ToolResult {
            Err(ToolError::Cancelled)
        }
    }

    /// A tool that requires a non-empty string and accepts an
    /// optional count. Exercises schema validation, including the
    /// required-vs-optional distinction.
    struct CountTool;

    #[derive(Serialize, Deserialize, JsonSchema)]
    struct CountArgs {
        label: String,
        #[serde(default)]
        count: Option<u32>,
    }

    #[derive(Serialize, Deserialize, JsonSchema)]
    struct EmptyArgs {}

    #[async_trait]
    impl Tool for CountTool {
        fn name(&self) -> &'static str {
            "count"
        }
        fn description(&self) -> &'static str {
            "Returns `label` repeated `count` times."
        }
        fn schema(&self) -> Schema {
            // Use the schema_for! macro then convert via JSON round-trip
            // since RootSchema -> Schema needs a different code path
            // depending on the schemars version.
            let root: schemars::schema::RootSchema = schemars::schema_for!(CountArgs);
            serde_json::from_value(serde_json::to_value(&root.schema).unwrap())
                .expect("schema is serialisable")
        }
        async fn execute(
            &self,
            args: Value,
            _ctx: ToolContext,
            _events: ToolEventSink,
            _cancel: CancellationToken,
        ) -> ToolResult {
            let parsed: CountArgs =
                serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs(e.to_string()))?;
            let n = parsed.count.unwrap_or(1);
            let output = parsed.label.repeat(n as usize);
            Ok(ToolOutcome::Success {
                output,
                truncated: false,
            })
        }
    }

    fn ctx() -> ToolContext {
        ToolContext {
            project_root: PathBuf::from("/tmp"),
            max_output_bytes: 4096,
            command_timeout: Duration::from_secs(5),
        }
    }

    fn call(name: &str, args: Value) -> ToolCall {
        ToolCall {
            call_id: ToolCallId(new_id()),
            name: name.into(),
            args,
        }
    }

    /// A bounded event channel sized for tests. Capacity must match
    /// the production contract (256) so we exercise the real
    /// backpressure path.
    fn events() -> (ToolEventSink, mpsc::Receiver<AgentEvent>) {
        mpsc::channel(256)
    }

    #[test]
    fn empty_registry_has_no_tools() {
        let reg = ToolRegistry::new();
        assert!(reg.names().is_empty());
        assert!(reg.get("anything").is_none());
        assert_eq!(reg.schemas_json(), serde_json::json!({}));
    }

    #[test]
    fn register_then_get_returns_some() {
        let mut reg = ToolRegistry::new();
        reg.register(EchoTool);
        assert!(reg.get("echo").is_some());
    }

    #[test]
    fn get_unknown_returns_none() {
        let reg = ToolRegistry::new();
        assert!(reg.get("nope").is_none());
    }

    #[test]
    fn names_lists_every_registered_tool() {
        // `names()` returns alphabetically sorted entries by contract;
        // registration order is irrelevant.
        let mut reg = ToolRegistry::new();
        reg.register(EchoTool);
        reg.register(BoomTool);
        reg.register(CountTool);
        assert_eq!(reg.names(), vec!["boom", "count", "echo"]);
    }

    #[test]
    fn schemas_json_has_one_key_per_tool() {
        let mut reg = ToolRegistry::new();
        reg.register(EchoTool);
        reg.register(BoomTool);
        let schemas = reg.schemas_json();
        let obj = schemas.as_object().expect("object");
        assert_eq!(obj.len(), 2);
        assert!(obj.contains_key("echo"));
        assert!(obj.contains_key("boom"));
        // Every schema is itself an object with a `type` discriminator
        // (schemars emits `$schema` for the draft marker).
        assert!(obj["echo"].is_object());
    }

    #[tokio::test]
    async fn execute_unknown_tool_returns_structured_error() {
        let reg = ToolRegistry::new();
        let (tx, mut rx) = events();
        // Hold an extra reference to `tx` so the channel is NOT
        // closed when we try_recv below. (Without this, the
        // receiver returns `Disconnected` because `tx` is dropped
        // at the end of the block.)
        let tx_keepalive = tx.clone();
        let outcome = execute_tool_call(
            &reg,
            &call("nope", serde_json::json!({})),
            ctx(),
            tx,
            CancellationToken::new(),
        )
        .await;
        match outcome {
            ToolOutcome::Error { code, .. } => {
                assert_eq!(code.0, "unknown_tool");
            }
            other => panic!("expected Error, got {other:?}"),
        }
        // Drain the channel: an unknown tool must not emit ToolStarted.
        match rx.try_recv() {
            Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {
                // expected: no events for an unknown tool
            }
            Ok(AgentEvent::ToolStarted { .. }) => {
                panic!("ToolStarted must NOT be emitted for an unknown tool")
            }
            Ok(other) => panic!("unexpected event: {other:?}"),
            Err(e) => panic!("unexpected channel state: {e}"),
        }
        drop(tx_keepalive); // explicit, for clarity
    }

    #[tokio::test]
    async fn execute_invalid_args_returns_structured_error() {
        let mut reg = ToolRegistry::new();
        reg.register(EchoTool); // requires `msg: String`
        let (tx, _rx) = events();
        // Missing required `msg`.
        let outcome = execute_tool_call(
            &reg,
            &call("echo", serde_json::json!({})),
            ctx(),
            tx,
            CancellationToken::new(),
        )
        .await;
        match outcome {
            ToolOutcome::Error { code, message, .. } => {
                assert_eq!(code.0, "invalid_args");
                assert!(!message.is_empty());
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn execute_tool_error_maps_to_tool_error_code() {
        let mut reg = ToolRegistry::new();
        reg.register(BoomTool);
        let (tx, _rx) = events();
        let outcome = execute_tool_call(
            &reg,
            &call("boom", serde_json::json!({})),
            ctx(),
            tx,
            CancellationToken::new(),
        )
        .await;
        match outcome {
            ToolOutcome::Error { code, message, .. } => {
                assert_eq!(code.0, "tool_error");
                assert!(message.contains("cancelled"));
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn execute_ok_returns_success_outcome() {
        let mut reg = ToolRegistry::new();
        reg.register(EchoTool);
        let (tx, _rx) = events();
        let outcome = execute_tool_call(
            &reg,
            &call("echo", serde_json::json!({"msg": "hello"})),
            ctx(),
            tx,
            CancellationToken::new(),
        )
        .await;
        assert_eq!(
            outcome,
            ToolOutcome::Success {
                output: "hello".into(),
                truncated: false,
            }
        );
    }

    #[tokio::test]
    async fn tool_started_event_fires_before_execute() {
        // Use a tool that we can block inside `execute` to assert the
        // ordering of events. We do this by adding a CountTool that
        // observes the channel order via the registry wrapper.
        let mut reg = ToolRegistry::new();
        reg.register(CountTool);
        let (tx, mut rx) = events();
        let _ = execute_tool_call(
            &reg,
            &call("count", serde_json::json!({"label": "x", "count": 3})),
            ctx(),
            tx,
            CancellationToken::new(),
        )
        .await;
        // The first event must be ToolStarted. Subsequent events are
        // tool-defined; we don't assert on those here.
        let first = rx.recv().await.expect("at least one event");
        match first {
            AgentEvent::ToolStarted { name, .. } => {
                assert_eq!(name, "count");
            }
            other => panic!("expected ToolStarted first, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn wrapper_emits_tool_finished_after_tool_returns() {
        // The wrapper now publishes the terminal `ToolFinished` event
        // itself, so consumers always see the outcome paired with the
        // call (matching the durable `SessionEntry::ToolFinished`
        // shape). For EchoTool, the event sequence is: ToolStarted,
        // then ToolFinished with the Success outcome.
        let mut reg = ToolRegistry::new();
        reg.register(EchoTool);
        let (tx, mut rx) = events();
        let _ = execute_tool_call(
            &reg,
            &call("echo", serde_json::json!({"msg": "ok"})),
            ctx(),
            tx,
            CancellationToken::new(),
        )
        .await;
        let evt = rx.recv().await.expect("event");
        assert!(matches!(evt, AgentEvent::ToolStarted { .. }));
        let evt = rx.recv().await.expect("event");
        match evt {
            AgentEvent::ToolFinished { result, .. } => match result {
                EventToolOutcome::Success { output, truncated } => {
                    assert_eq!(output, "ok");
                    assert!(!truncated);
                }
                other => panic!("expected Success, got {other:?}"),
            },
            other => panic!("expected ToolFinished, got {other:?}"),
        }
        assert!(rx.try_recv().is_err(), "no extra events expected");
    }

    #[test]
    fn schemas_json_is_lexicographically_ordered() {
        // Determinism gate: the live model sees the same tool order on
        // every turn. We register three tools out of order and assert
        // the JSON output is alphabetical.
        let mut reg = ToolRegistry::new();
        reg.register(CountTool); // "count"
        reg.register(EchoTool); // "echo"
        reg.register(BoomTool); // "boom"
        let value = reg.schemas_json();
        let obj = value.as_object().expect("object");
        let mut keys: Vec<&String> = obj.keys().collect();
        keys.sort();
        let original: Vec<&String> = obj.keys().collect();
        assert_eq!(original, keys, "tool schemas must be in sorted order");
    }

    #[test]
    fn tool_specs_include_description() {
        // The provider adapter needs the description alongside the
        // schema so it can build a `genai::chat::Tool` declaration.
        let mut reg = ToolRegistry::new();
        reg.register(EchoTool);
        let specs = reg.tool_specs();
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].name, "echo");
        assert!(specs[0].description.contains("Echoes"));
    }

    #[test]
    fn validate_args_reports_useful_error_message() {
        let mut reg = ToolRegistry::new();
        reg.register(EchoTool);
        let tool = reg.get("echo").expect("echo registered");
        let schema = tool.schema();
        // Missing required `msg`.
        let err =
            validate_args(&serde_json::json!({}), &schema).expect_err("missing required field");
        // Message must be non-empty and reference the missing
        // property so the model can fix its call.
        assert!(!err.is_empty());
        // The validation error for a missing required field mentions
        // `msg`. This is a soft assertion — jsonschema wording can
        // shift between point releases — but it pins the contract
        // that the error is actionable.
        assert!(
            err.to_lowercase().contains("msg") || err.contains("required"),
            "expected error to mention `msg` or `required`, got: {err}",
        );
    }

    #[test]
    fn validate_args_accepts_well_formed_input() {
        let mut reg = ToolRegistry::new();
        reg.register(EchoTool);
        let tool = reg.get("echo").unwrap();
        let schema = tool.schema();
        validate_args(&serde_json::json!({"msg": "hi"}), &schema)
            .expect("valid input must validate");
    }

    #[test]
    fn validate_args_rejects_wrong_type() {
        let mut reg = ToolRegistry::new();
        reg.register(EchoTool);
        let tool = reg.get("echo").unwrap();
        let schema = tool.schema();
        // `msg` is a string; pass a number.
        let err = validate_args(&serde_json::json!({"msg": 42}), &schema)
            .expect_err("number for string must fail");
        assert!(!err.is_empty());
    }

    #[test]
    fn schemas_for_count_tool_marks_label_required() {
        let mut reg = ToolRegistry::new();
        reg.register(CountTool);
        let tool = reg.get("count").unwrap();
        let schema = tool.schema();
        let value = serde_json::to_value(&schema).expect("serialise");
        // `count` is optional in CountArgs so it must NOT be in the
        // `required` array; `label` must be.
        let required = value
            .get("required")
            .and_then(|v| v.as_array())
            .expect("required array");
        let names: Vec<&str> = required.iter().filter_map(|v| v.as_str()).collect();
        assert!(names.contains(&"label"));
        assert!(!names.contains(&"count"));
    }

    #[tokio::test]
    async fn execute_with_optional_count_works() {
        // End-to-end check that an optional field (not present in
        // args) still validates and runs.
        let mut reg = ToolRegistry::new();
        reg.register(CountTool);
        let (tx, _rx) = events();
        let outcome = execute_tool_call(
            &reg,
            &call("count", serde_json::json!({"label": "ab"})),
            ctx(),
            tx,
            CancellationToken::new(),
        )
        .await;
        assert_eq!(
            outcome,
            ToolOutcome::Success {
                output: "ab".into(),
                truncated: false,
            }
        );
    }

    /// Sanity: the `Tool` trait is object-safe. If we accidentally
    /// add a generic method or `Self` in a return position, this
    /// line fails to compile.
    #[allow(dead_code)]
    fn _object_safety(t: Arc<dyn Tool>) -> Arc<dyn Tool> {
        t
    }

    // Smoke check that the wrapper's `ToolOutcome` and the event
    // stream's `ToolOutcome` are interchangeable at the value level
    // when produced by a tool. (They are the same type, but the
    // equality assertion catches a future refactor that splits them.)
    #[allow(dead_code)]
    fn _outcome_identity(o: ToolOutcome) -> EventToolOutcome {
        match o {
            ToolOutcome::Success { output, truncated } => {
                EventToolOutcome::Success { output, truncated }
            }
            ToolOutcome::Error {
                code,
                message,
                truncated,
            } => EventToolOutcome::Error {
                code,
                message,
                truncated,
            },
        }
    }
}
