### Task 2.4 — Tool registry + schema validation + output truncation

**Files:**
- Create: `src/tool/mod.rs`
- Create: `src/tool/path.rs`
- Create: `src/tool/read.rs`
- Modify: `src/lib.rs` (`pub mod tool;`)

**Spec references:** v0 spec §13 (Tool trait), §4 (project-root confinement).

**Why this exists:** the registry is the bridge between the agent loop (which has tool calls as JSON) and the tools (which have typed Rust APIs). It validates args, runs the tool, captures the output, and wraps it in a `ToolResult` that the agent loop can record as a `SessionEntry::ToolFinished`.

**NOTE:** task 2.3 is a subset of this task in the wave plan. In the wave 1 plan, 2.3 was just the read tool and 2.4 was the registry. After the wave 1 review we combined them so the read tool and registry ship together. **Implement this task and skip 2.3 (mark it as merged into 2.4).**

**Interfaces (exact):**

```rust
// src/tool/mod.rs
use async_trait::async_trait;
use schemars::Schema;
use serde_json::Value;
use tokio_util::sync::CancellationToken;
use tokio::sync::mpsc;
use crate::event::{AgentEvent, ErrorCode, ToolCallId, ToolStream};
use std::time::Duration;
use std::path::PathBuf;
use std::sync::Arc;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct ToolContext {
    pub project_root: PathBuf,
    pub max_output_bytes: usize,
    pub command_timeout: Duration,
}

pub type ToolEventSink = mpsc::Sender<AgentEvent>;

#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("invalid arguments: {0}")] InvalidArgs(String),
    #[error("path escapes project root: {0}")] PathEscape(PathBuf),
    #[error("path is not a regular file: {0}")] NotAFile(PathBuf),
    #[error("file is binary: {0}")] Binary(PathBuf),
    #[error("read failed: {0}")] Io(#[from] std::io::Error),
    #[error("cancelled")] Cancelled,
    #[error("output too large ({actual} > {limit} bytes)")] TooLarge { actual: u64, limit: usize },
}

pub type ToolResult = Result<ToolOutcome, ToolError>;

#[derive(Debug, Clone)]
pub enum ToolOutcome {
    Success { output: String, truncated: bool },
    Error { code: ErrorCode, message: String, truncated: bool },
}

#[derive(Debug, Clone)]
pub struct ToolCall {
    pub call_id: ToolCallId,
    pub name: String,
    pub args: Value,
}

#[async_trait::async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn schema(&self) -> Schema;
    async fn execute(
        &self,
        args: Value,
        ctx: ToolContext,
        events: ToolEventSink,
        cancel: CancellationToken,
    ) -> ToolResult;
}

pub struct ToolRegistry {
    tools: HashMap<&'static str, Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self { Self { tools: HashMap::new() } }
    pub fn register<T: Tool + 'static>(&mut self, tool: T) {
        self.tools.insert(tool.name(), Arc::new(tool));
    }
    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(name).cloned()
    }
    pub fn names(&self) -> Vec<&'static str> {
        self.tools.keys().copied().collect()
    }
    pub fn schemas_json(&self) -> Value {
        let mut map = serde_json::Map::new();
        for (name, tool) in &self.tools {
            let schema = tool.schema();
            map.insert((*name).to_string(), serde_json::to_value(&schema).unwrap_or(Value::Null));
        }
        Value::Object(map)
    }
}

/// Execute a tool call, validate args, and capture the result.
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
    // Emit ToolStarted
    let _ = events.send(AgentEvent::ToolStarted {
        call_id: call.call_id,
        name: tool.name().to_string(),
        args: call.args.clone(),
    }).await;
    // Validate args against schema
    if let Err(e) = validate_args(&call.args, &tool.schema()) {
        return ToolOutcome::Error {
            code: ErrorCode("invalid_args".into()),
            message: e,
            truncated: false,
        };
    }
    // Execute
    match tool.execute(call.args.clone(), ctx, events.clone(), cancel).await {
        Ok(outcome) => outcome,
        Err(e) => ToolOutcome::Error {
            code: ErrorCode("tool_error".into()),
            message: e.to_string(),
            truncated: false,
        },
    }
}

fn validate_args(args: &Value, schema: &Schema) -> Result<(), String> {
    // Use jsonschema crate to validate `args` against `schema`.
    // jsonschema::validator_for(schema).validate(args).map_err(|e| e.to_string())
    todo!()
}
```

**`ToolEventSink` backpressure contract** (Decision 06):
- The `mpsc::Sender` has capacity 256.
- If the receiver is slow and the channel fills, `send().await` blocks.
- Tool implementations MUST NOT call `send().try_send()` (which would drop) for terminal events (`ToolStarted`, `ToolFinished`).
- For streaming output (chunks), tools MAY use `try_send` and silently drop chunks on backpressure. The receiver counts dropped chunks and emits a `ToolOutput { chunk: b"..." }` summary at the end if any were dropped.

**Acceptance:**
- 12+ unit tests for the registry:
  1. empty registry has no tools
  2. register + get returns Some
  3. get(unknown) returns None
  4. names() lists registered tools
  5. schemas_json returns one key per tool
  6. execute_tool_call on unknown tool returns ToolOutcome::Error (NOT a panic)
  7. execute_tool_call with invalid args returns ToolOutcome::Error
  8. execute_tool_call on a tool that returns Err returns ToolOutcome::Error with code "tool_error"
  9. execute_tool_call on a tool that returns Ok returns ToolOutcome::Success
  10. ToolStarted event is sent before the tool executes
  11. ToolFinished event is sent after the tool returns (via the wrapper or the tool itself)
  12. args validation uses jsonschema (or similar) and reports a useful error
- 12+ unit tests for `safe_resolve` (in `src/tool/path.rs`):
  1. relative path "foo/bar" inside root
  2. relative path ".." escapes root
  3. absolute path inside root
  4. absolute path outside root
  5. symlink that resolves inside root
  6. symlink that resolves outside root
  7. non-existing path resolves via nearest existing parent
  8. path with `..` segments that resolve inside
  9. path with `..` segments that resolve outside
  10. empty path
  11. root itself
  12. path with trailing slash
- 12+ unit tests for ReadTool:
  1. read a small file
  2. read with offset/limit
  3. read with offset past EOF → empty
  4. read binary file (NUL in first 8KB) → Binary error
  5. read directory → NotAFile error
  6. read path that escapes via `..` → PathEscape error
  7. read absolute path outside root → PathEscape error
  8. read through symlink pointing outside → PathEscape error
  9. read with output > max_output_bytes → truncated flag
  10. read empty file → empty output, no error
  11. cancellation token fires mid-read → Cancelled error
  12. looks_binary returns true for binary bytes, false for ASCII
- 2+ integration tests in `tests/tool_registry.rs`:
  1. ReadTool works end-to-end on a temp dir
  2. Unknown tool name in a `ToolCall` returns an error event
- Gate: clean.

**Forbidden:**
- No `unsafe`.
- No `unwrap`/`expect` in library code (test code is fine).
- No `panic!`.
- No `std::fs::read` on a path that hasn't been `safe_resolve`d.

**Dependencies:** add `jsonschema = "0.17"` to `[dependencies]`.
