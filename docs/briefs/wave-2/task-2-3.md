### Task 2.3 — `read` tool

**Files:**
- Create: `src/tool/mod.rs` (the Tool trait, ToolRegistry, execute_tool_call)
- Create: `src/tool/path.rs` (path canonicalization helpers)
- Create: `src/tool/read.rs`
- Modify: `src/lib.rs` (`pub mod tool;`)

**Spec references:** v0 spec §13 read tool contract, §4 (project-root confinement), §3.2 (no sandboxing).

**Why this exists:** the read tool is the only way the agent can see the repository. Its security model is the foundation: every path it returns is guaranteed to be inside `project_root` after canonicalization.

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
    tools: std::collections::HashMap<String, std::sync::Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self;
    pub fn register<T: Tool + 'static>(&mut self, tool: T);
    pub fn get(&self, name: &str) -> Option<std::sync::Arc<dyn Tool>>;
    pub fn names(&self) -> Vec<&'static str>;
    pub fn schemas_json(&self) -> Value;
}

pub async fn execute_tool_call(
    reg: &ToolRegistry,
    call: &ToolCall,
    ctx: ToolContext,
    events: ToolEventSink,
    cancel: CancellationToken,
) -> ToolOutcome;

pub struct ToolCall {
    pub call_id: ToolCallId,
    pub name: String,
    pub args: Value,
}
```

```rust
// src/tool/path.rs
use std::path::{Path, PathBuf};
use crate::tool::ToolError;

/// Resolve a path safely inside `project_root`.
///
/// - Rejects absolute paths that are not inside the project.
/// - Canonicalizes the input (resolves `..`, symlinks).
/// - If the path does not exist, canonicalizes the nearest existing
///   parent and re-appends the non-existing tail. This is the "TOCTOU-safe"
///   pattern from spec §4.
/// - Returns the canonical path or a `ToolError::PathEscape`.
pub fn safe_resolve(
    project_root: &Path,
    path: &Path,
) -> Result<PathBuf, ToolError>;

/// Returns true if `path` is inside `root` (after canonicalization).
pub fn is_inside(root: &Path, path: &Path) -> bool;

/// Sniff the first 8KB of a file for a NUL byte. Returns true if the
/// file looks binary and should be rejected by `read`.
pub fn looks_binary(bytes: &[u8]) -> bool;
```

```rust
// src/tool/read.rs
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, JsonSchema, Debug)]
pub struct ReadArgs {
    /// Path relative to the project root, or absolute if inside the root.
    pub path: String,
    /// Optional 1-based line offset (default: 1).
    pub offset: Option<u32>,
    /// Optional maximum number of lines to return.
    pub limit: Option<u32>,
}

pub struct ReadTool;

impl ReadTool {
    pub fn new() -> Self;
}

#[async_trait::async_trait]
impl Tool for ReadTool {
    fn name(&self) -> &'static str { "read" }
    fn description(&self) -> &'static str { "Read a file from the project. Path is relative to the project root or absolute within the project." }
    fn schema(&self) -> Schema { schemars::schema_for!(ReadArgs) }
    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: ToolContext,
        _events: ToolEventSink,
        cancel: CancellationToken,
    ) -> ToolResult {
        // Implementation:
        // 1. Parse args as ReadArgs. On failure -> ToolError::InvalidArgs.
        // 2. safe_resolve(ctx.project_root, &Path::new(&args.path)).
        // 3. Check is_file() (not a directory, not a symlink to dir, etc).
        // 4. Read first 8KB; if looks_binary -> ToolError::Binary.
        // 5. Read full content; cap at ctx.max_output_bytes; if exceeded,
        //    truncate and set ToolOutcome::Success { truncated: true, output: <truncated content> }.
        // 6. Apply line offset/limit. Compute line numbers.
        // 7. Return ToolOutcome::Success.
        // 8. Cancellation: check at the start of each step; if cancelled -> ToolError::Cancelled.
        todo!()
    }
}
```

**Acceptance:**
- 14+ unit tests:
  1. read a small file
  2. read with line offset and limit
  3. read with offset past EOF → empty output, no error
  4. read a binary file (NUL byte in first 8KB) → ToolError::Binary
  5. read a directory → ToolError::NotAFile
  6. read a path with `..` segments that escapes project root → ToolError::PathEscape
  7. read an absolute path outside project root → ToolError::PathEscape
  8. read through a symlink that points outside project root → ToolError::PathEscape
  9. read a file that exceeds max_output_bytes → truncated flag
  10. read with no offset/limit defaults to whole file
  11. ToolRegistry::new() is empty
  12. ToolRegistry::register then .get returns Some
  13. ToolRegistry::get(unknown_name) returns None
  14. execute_tool_call on unknown tool returns ToolOutcome::Error, NOT a panic
  15. schema validation rejects bad args (e.g. missing "path") → ToolError::InvalidArgs
  16. safe_resolve canonicalizes a `..` path
  17. looks_binary returns true for `{0x00, 0x01, ...}`, false for ASCII
- 2+ integration tests in `tests/tool_registry.rs`:
  - `crow::tool::execute_tool_call` with the ReadTool works end-to-end on a temp dir
- Gate: clean.

**Forbidden:**
- No `unsafe`.
- No `unwrap`/`expect` in library code.
- No `panic!`.
- No `std::fs::read_to_string` on a path that hasn't been `safe_resolve`d.

**Dependencies:** `schemars = "0.8"` (already in Cargo.toml). `tokio::fs` for async file IO.
