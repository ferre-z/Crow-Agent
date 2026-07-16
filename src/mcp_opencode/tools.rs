//! MCP tool surface: schemas + handler dispatch.
//!
//! Each tool has a JSON-Schema `inputSchema` advertised via `tools/list`
//! and a handler that takes the parsed arguments and returns a
//! [`ToolResult`] (MCP `content: [{type:"text", text:...}]`).
//!
//! Tool text payloads are always JSON-encoded envelopes so callers
//! that don't surface `structuredContent` can still parse the result.
//! For batched calls (parallel / fanout) the envelope is an array of
//! per-task results; clients can iterate and pick fields.
//!
//! Handler dispatch is async; the parallel / fanout handlers use
//! [`futures::future::join_all`] so all subprocesses run concurrently.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use crate::mcp_opencode::registry::{TaskId, TaskRegistry};
use crate::mcp_opencode::runner::{OpencodeRunner, RunRequest, RunResult, RunnerError};

// ---------------------------------------------------------------------------
// Public surface: tool schemas
// ---------------------------------------------------------------------------

/// Every tool this server advertises. `tools/list` returns these.
#[must_use]
pub fn tool_schemas() -> Vec<Value> {
    vec![
        schema_delegate(),
        schema_delegate_parallel(),
        schema_delegate_fanout(),
        schema_status(),
        schema_cancel(),
        schema_list_models(),
        schema_version(),
    ]
}

fn schema_delegate() -> Value {
    json!({
        "name": "opencode_delegate",
        "title": "Delegate a task to opencode",
        "description": "Submit one task to a fresh opencode subprocess and wait for the final result. Use this when you have a single self-contained task.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "prompt": { "type": "string", "description": "The prompt to send to opencode." },
                "workdir": { "type": "string", "description": "Working directory for the run. Defaults to the server's cwd." },
                "model": { "type": "string", "description": "Optional model override in provider/model form." },
                "agent": { "type": "string", "description": "Optional agent override." },
                "timeout_seconds": { "type": "integer", "minimum": 0, "description": "Optional wall-clock timeout in seconds." },
                "title": { "type": "string", "description": "Optional session title shown in opencode's UI." }
            },
            "required": ["prompt"],
            "additionalProperties": false
        }
    })
}

fn schema_delegate_parallel() -> Value {
    json!({
        "name": "opencode_delegate_parallel",
        "title": "Delegate N tasks in parallel",
        "description": "Submit N independent tasks and run them concurrently as separate opencode subprocesses. Returns an array of results in input order. Use this for fan-out research, parallel code review of distinct files, etc.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "tasks": {
                    "type": "array",
                    "description": "Independent tasks to run in parallel.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "prompt": { "type": "string" },
                            "workdir": { "type": "string" },
                            "model": { "type": "string" },
                            "agent": { "type": "string" },
                            "timeout_seconds": { "type": "integer" },
                            "title": { "type": "string" }
                        },
                        "required": ["prompt"],
                        "additionalProperties": false
                    },
                    "minItems": 1
                }
            },
            "required": ["tasks"],
            "additionalProperties": false
        }
    })
}

fn schema_delegate_fanout() -> Value {
    json!({
        "name": "opencode_delegate_fanout",
        "title": "Fan out one prompt across directories",
        "description": "Run the same prompt in N different working directories concurrently. Returns an array of per-directory results in input order. Use this to explore subtrees or apply the same prompt to several repos at once.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "prompt": { "type": "string", "description": "The prompt to run in each workdir." },
                "workdirs": {
                    "type": "array",
                    "description": "Working directories to run in.",
                    "items": { "type": "string" },
                    "minItems": 1
                },
                "model": { "type": "string" },
                "agent": { "type": "string" },
                "timeout_seconds": { "type": "integer" }
            },
            "required": ["prompt", "workdirs"],
            "additionalProperties": false
        }
    })
}

fn schema_status() -> Value {
    json!({
        "name": "opencode_status",
        "title": "Get status of a delegated task",
        "description": "Look up the in-flight status of a previously-submitted task by id. Returns null if the task is no longer tracked (already finished or never existed).",
        "inputSchema": {
            "type": "object",
            "properties": {
                "task_id": { "type": "string", "description": "The task id returned by opencode_delegate(_parallel|_fanout)." }
            },
            "required": ["task_id"],
            "additionalProperties": false
        }
    })
}

fn schema_cancel() -> Value {
    json!({
        "name": "opencode_cancel",
        "title": "Cancel an in-flight task",
        "description": "Cancel an in-flight task by id. The underlying opencode subprocess receives SIGKILL via its process group.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "task_id": { "type": "string" }
            },
            "required": ["task_id"],
            "additionalProperties": false
        }
    })
}

fn schema_list_models() -> Value {
    json!({
        "name": "opencode_list_models",
        "title": "List opencode models",
        "description": "Return the list of models the opencode binary knows about, parsed from `opencode models`.",
        "inputSchema": {
            "type": "object",
            "properties": {},
            "additionalProperties": false
        }
    })
}

fn schema_version() -> Value {
    json!({
        "name": "opencode_version",
        "title": "opencode server / binary info",
        "description": "Return diagnostic info about the MCP server itself and the opencode binary it delegates to.",
        "inputSchema": {
            "type": "object",
            "properties": {},
            "additionalProperties": false
        }
    })
}

// ---------------------------------------------------------------------------
// Argument types (parsed from MCP `arguments`)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DelegateArgs {
    prompt: String,
    #[serde(default)]
    workdir: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    agent: Option<String>,
    #[serde(default)]
    timeout_seconds: Option<u64>,
    #[serde(default)]
    title: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TaskSpec {
    prompt: String,
    #[serde(default)]
    workdir: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    agent: Option<String>,
    #[serde(default)]
    timeout_seconds: Option<u64>,
    #[serde(default)]
    title: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ParallelArgs {
    tasks: Vec<TaskSpec>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FanoutArgs {
    prompt: String,
    workdirs: Vec<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    agent: Option<String>,
    #[serde(default)]
    timeout_seconds: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TaskIdArgs {
    task_id: String,
}

#[derive(Debug, Clone, Serialize)]
struct DelegateEnvelope {
    task_id: TaskId,
    #[serde(flatten)]
    result: RunOutcome,
}

/// JSON-serialisable run outcome. Distinguishes success (`Ok`) from
/// failure (`Err`) and keeps the per-field shape machine-parseable.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum RunOutcome {
    Ok {
        message: String,
        elapsed_ms: u64,
        events: usize,
    },
    Err {
        kind: String,
        message: String,
    },
}

impl RunOutcome {
    fn from_result(r: &Result<RunResult, RunnerError>) -> Self {
        match r {
            Ok(rr) => Self::Ok {
                message: rr.message.clone(),
                elapsed_ms: rr.elapsed.as_millis() as u64,
                events: rr.events.len(),
            },
            Err(e) => Self::Err {
                kind: error_kind(e).to_string(),
                message: e.to_string(),
            },
        }
    }
}

fn error_kind(e: &RunnerError) -> &'static str {
    match e {
        RunnerError::BinaryNotFound(_) => "binary_not_found",
        RunnerError::NonZeroExit { .. } => "non_zero_exit",
        RunnerError::Cancelled => "cancelled",
        RunnerError::TimedOut(_) => "timed_out",
        RunnerError::Io(_) => "io",
        RunnerError::NoTerminalEvent => "no_terminal_event",
    }
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

/// Dispatch one `tools/call` invocation to its handler. Errors during
/// argument parsing are surfaced as `isError: true` results so callers
/// get a readable message instead of a JSON-RPC protocol error.
pub async fn dispatch(
    name: &str,
    arguments: Value,
    runner: Arc<dyn OpencodeRunner>,
    registry: TaskRegistry,
    server_version: Arc<String>,
    binary: PathBuf,
) -> Value {
    let result = match name {
        "opencode_delegate" => match serde_json::from_value::<DelegateArgs>(arguments) {
            Ok(a) => handle_delegate(a, runner, registry).await,
            Err(e) => Err(format!("invalid arguments: {e}")),
        },
        "opencode_delegate_parallel" => match serde_json::from_value::<ParallelArgs>(arguments) {
            Ok(a) => handle_parallel(a, runner, registry).await,
            Err(e) => Err(format!("invalid arguments: {e}")),
        },
        "opencode_delegate_fanout" => match serde_json::from_value::<FanoutArgs>(arguments) {
            Ok(a) => handle_fanout(a, runner, registry).await,
            Err(e) => Err(format!("invalid arguments: {e}")),
        },
        "opencode_status" => match serde_json::from_value::<TaskIdArgs>(arguments) {
            Ok(a) => handle_status(a, registry).await,
            Err(e) => Err(format!("invalid arguments: {e}")),
        },
        "opencode_cancel" => match serde_json::from_value::<TaskIdArgs>(arguments) {
            Ok(a) => handle_cancel(a, registry).await,
            Err(e) => Err(format!("invalid arguments: {e}")),
        },
        "opencode_list_models" => handle_list_models(binary.clone()).await,
        "opencode_version" => Ok(handle_version(&binary, &server_version).await),
        other => Err(format!("unknown tool: {other}")),
    };
    match result {
        Ok(value) => success_result(&value),
        Err(message) => error_result(&message),
    }
}

fn success_result(value: &Value) -> Value {
    json!({
        "content": [{
            "type": "text",
            "text": serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
        }],
        "isError": false
    })
}

fn error_result(message: &str) -> Value {
    json!({
        "content": [{
            "type": "text",
            "text": message
        }],
        "isError": true
    })
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn handle_delegate(
    a: DelegateArgs,
    runner: Arc<dyn OpencodeRunner>,
    registry: TaskRegistry,
) -> Result<Value, String> {
    let req = build_request(
        a.prompt,
        a.workdir,
        a.model,
        a.agent,
        a.timeout_seconds,
        a.title,
    )?;
    let (id, cancel) = registry.register(req.clone()).await;
    let outcome = runner.run(req, cancel).await;
    registry.remove(id).await;
    let envelope = DelegateEnvelope {
        task_id: id,
        result: RunOutcome::from_result(&outcome),
    };
    let value = serde_json::to_value(&envelope).map_err(|e| format!("serialize: {e}"))?;
    Ok(value)
}

async fn handle_parallel(
    a: ParallelArgs,
    runner: Arc<dyn OpencodeRunner>,
    registry: TaskRegistry,
) -> Result<Value, String> {
    if a.tasks.is_empty() {
        return Err("tasks must contain at least one item".into());
    }
    // Register every task up front so cancel works mid-flight.
    let mut prepared: Vec<(TaskId, CancellationToken, RunRequest)> =
        Vec::with_capacity(a.tasks.len());
    for t in a.tasks {
        let req = build_request(
            t.prompt,
            t.workdir,
            t.model,
            t.agent,
            t.timeout_seconds,
            t.title,
        )?;
        let (id, cancel) = registry.register(req.clone()).await;
        prepared.push((id, cancel, req));
    }
    // Fan out.
    let futures = prepared
        .iter()
        .map(|(_, cancel, req)| {
            let r = runner.clone();
            let c = cancel.clone();
            let req = req.clone();
            async move { r.run(req, c).await }
        })
        .collect::<Vec<_>>();
    let outcomes: Vec<Result<RunResult, RunnerError>> = futures::future::join_all(futures).await;
    // Evict everything we registered.
    for (id, _, _) in &prepared {
        registry.remove(*id).await;
    }
    let envelopes: Vec<DelegateEnvelope> = prepared
        .into_iter()
        .zip(outcomes.iter())
        .map(|((id, _, _), outcome)| DelegateEnvelope {
            task_id: id,
            result: RunOutcome::from_result(outcome),
        })
        .collect();
    serde_json::to_value(&envelopes).map_err(|e| format!("serialize: {e}"))
}

async fn handle_fanout(
    a: FanoutArgs,
    runner: Arc<dyn OpencodeRunner>,
    registry: TaskRegistry,
) -> Result<Value, String> {
    if a.workdirs.is_empty() {
        return Err("workdirs must contain at least one entry".into());
    }
    let mut prepared: Vec<(TaskId, CancellationToken, RunRequest)> =
        Vec::with_capacity(a.workdirs.len());
    for wd in &a.workdirs {
        let req = build_request(
            a.prompt.clone(),
            Some(wd.clone()),
            a.model.clone(),
            a.agent.clone(),
            a.timeout_seconds,
            None,
        )?;
        let (id, cancel) = registry.register(req.clone()).await;
        prepared.push((id, cancel, req));
    }
    let futures = prepared
        .iter()
        .map(|(_, cancel, req)| {
            let r = runner.clone();
            let c = cancel.clone();
            let req = req.clone();
            async move { r.run(req, c).await }
        })
        .collect::<Vec<_>>();
    let outcomes: Vec<Result<RunResult, RunnerError>> = futures::future::join_all(futures).await;
    for (id, _, _) in &prepared {
        registry.remove(*id).await;
    }
    let envelopes: Vec<DelegateEnvelope> = prepared
        .into_iter()
        .zip(outcomes.iter())
        .map(|((id, _, _), outcome)| DelegateEnvelope {
            task_id: id,
            result: RunOutcome::from_result(outcome),
        })
        .collect();
    serde_json::to_value(&envelopes).map_err(|e| format!("serialize: {e}"))
}

async fn handle_status(a: TaskIdArgs, registry: TaskRegistry) -> Result<Value, String> {
    let id = parse_task_id(&a.task_id)?;
    let status = registry.get(id).await;
    match status {
        Some(s) => serde_json::to_value(&s).map_err(|e| format!("serialize: {e}")),
        None => Ok(Value::Null),
    }
}

async fn handle_cancel(a: TaskIdArgs, registry: TaskRegistry) -> Result<Value, String> {
    let id = parse_task_id(&a.task_id)?;
    let cancelled = registry.cancel(id).await;
    Ok(json!({ "task_id": id.to_string(), "cancelled": cancelled }))
}

async fn handle_list_models(binary: PathBuf) -> Result<Value, String> {
    let models = crate::mcp_opencode::runner::opencode_models(&binary)
        .await
        .map_err(|e| e.to_string())?;
    serde_json::to_value(&models).map_err(|e| format!("serialize: {e}"))
}

async fn handle_version(binary: &std::path::Path, server_version: &str) -> Value {
    let opencode_version = crate::mcp_opencode::runner::opencode_version(binary).await;
    json!({
        "server": {
            "name": "crow-mcp-opencode",
            "version": server_version,
            "protocol": "mcp"
        },
        "opencode": {
            "binary": binary.display().to_string(),
            "version": opencode_version
        }
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn build_request(
    prompt: String,
    workdir: Option<String>,
    model: Option<String>,
    agent: Option<String>,
    timeout_seconds: Option<u64>,
    title: Option<String>,
) -> Result<RunRequest, String> {
    let workdir = match workdir {
        Some(w) => PathBuf::from(w),
        None => std::env::current_dir().map_err(|e| format!("workdir: {e}"))?,
    };
    let timeout = timeout_seconds.map(Duration::from_secs);
    Ok(RunRequest {
        workdir,
        prompt,
        model,
        agent,
        timeout,
        title,
    })
}

fn parse_task_id(s: &str) -> Result<TaskId, String> {
    let ulid = ulid::Ulid::from_string(s).map_err(|e| format!("invalid task_id: {e}"))?;
    Ok(TaskId(ulid))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp_opencode::events::OpencodeEvent;
    use crate::mcp_opencode::runner::{ScriptedRunner, ScriptedStep};

    fn scripted_runner(message: &str, delay: Duration) -> Arc<dyn OpencodeRunner> {
        let steps = vec![ScriptedStep {
            event: OpencodeEvent::Done {
                message: message.into(),
            },
            delay,
        }];
        Arc::new(ScriptedRunner::new(steps))
    }

    #[tokio::test]
    async fn delegate_returns_envelope() {
        let runner = scripted_runner("hi", Duration::from_millis(5));
        let registry = TaskRegistry::new();
        let args = json!({ "prompt": "say hi" });
        let value = dispatch(
            "opencode_delegate",
            args,
            runner,
            registry,
            Arc::new("test".to_string()),
            PathBuf::from("opencode"),
        )
        .await;
        assert_eq!(value["isError"], false);
        let text = value["content"][0]["text"].as_str().unwrap();
        let parsed: Value = serde_json::from_str(text).unwrap();
        assert_eq!(parsed["status"], "ok");
        assert_eq!(parsed["message"], "hi");
        assert!(parsed["task_id"].is_string());
    }

    #[tokio::test]
    async fn parallel_returns_array_in_input_order() {
        let runner = scripted_runner("ok", Duration::from_millis(10));
        let registry = TaskRegistry::new();
        let args = json!({
            "tasks": [
                { "prompt": "t1" },
                { "prompt": "t2" },
                { "prompt": "t3" }
            ]
        });
        let value = dispatch(
            "opencode_delegate_parallel",
            args,
            runner,
            registry,
            Arc::new("test".to_string()),
            PathBuf::from("opencode"),
        )
        .await;
        assert_eq!(value["isError"], false);
        let parsed: Value =
            serde_json::from_str(value["content"][0]["text"].as_str().unwrap()).unwrap();
        let arr = parsed.as_array().expect("array");
        assert_eq!(arr.len(), 3);
        for entry in arr {
            assert_eq!(entry["status"], "ok");
            assert_eq!(entry["message"], "ok");
        }
    }

    #[tokio::test]
    async fn parallel_rejects_empty_tasks() {
        let runner = scripted_runner("ok", Duration::from_millis(5));
        let registry = TaskRegistry::new();
        let value = dispatch(
            "opencode_delegate_parallel",
            json!({ "tasks": [] }),
            runner,
            registry,
            Arc::new("test".to_string()),
            PathBuf::from("opencode"),
        )
        .await;
        assert_eq!(value["isError"], true);
    }

    #[tokio::test]
    async fn unknown_tool_surfaces_as_is_error() {
        let runner = scripted_runner("ok", Duration::from_millis(5));
        let registry = TaskRegistry::new();
        let value = dispatch(
            "opencode_nope",
            json!({}),
            runner,
            registry,
            Arc::new("test".to_string()),
            PathBuf::from("opencode"),
        )
        .await;
        assert_eq!(value["isError"], true);
        assert!(value["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("unknown tool"));
    }

    #[tokio::test]
    async fn invalid_args_surfaces_as_is_error() {
        let runner = scripted_runner("ok", Duration::from_millis(5));
        let registry = TaskRegistry::new();
        let value = dispatch(
            "opencode_delegate",
            json!({ "prompt": 42 }), // wrong type
            runner,
            registry,
            Arc::new("test".to_string()),
            PathBuf::from("opencode"),
        )
        .await;
        assert_eq!(value["isError"], true);
        assert!(value["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("invalid arguments"));
    }

    #[tokio::test]
    async fn status_unknown_id_returns_null() {
        let runner = scripted_runner("ok", Duration::from_millis(5));
        let registry = TaskRegistry::new();
        let value = dispatch(
            "opencode_status",
            json!({ "task_id": TaskId::new().to_string() }),
            runner,
            registry,
            Arc::new("test".to_string()),
            PathBuf::from("opencode"),
        )
        .await;
        let parsed: Value =
            serde_json::from_str(value["content"][0]["text"].as_str().unwrap()).unwrap();
        assert!(parsed.is_null());
    }

    #[test]
    fn tool_schemas_are_well_formed() {
        let schemas = tool_schemas();
        assert_eq!(schemas.len(), 7);
        let names: Vec<&str> = schemas
            .iter()
            .map(|s| s["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"opencode_delegate"));
        assert!(names.contains(&"opencode_delegate_parallel"));
        assert!(names.contains(&"opencode_delegate_fanout"));
        assert!(names.contains(&"opencode_status"));
        assert!(names.contains(&"opencode_cancel"));
        assert!(names.contains(&"opencode_list_models"));
        assert!(names.contains(&"opencode_version"));
    }
}
