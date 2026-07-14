### Task 3.1 — `write` tool

**Files:**
- Create: `src/tool/write.rs`
- Modify: `src/lib.rs` (`pub mod write;` — actually already exposed via `pub mod tool;`)
- Modify: `src/tool/mod.rs` (register WriteTool in the example/default registry if there is one; otherwise no change)

**Spec references:** v0 spec §13 write tool, §4 atomic file replacement, §3.2 (no sandboxing).

**Why this exists:** the write tool creates or replaces files. Atomicity is required so a crash mid-write can't corrupt an existing file (the original is preserved until the rename).

**Interfaces (exact):**

```rust
// src/tool/write.rs
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio_util::sync::CancellationToken;
use crate::tool::{Tool, ToolContext, ToolEventSink, ToolError, ToolResult, ToolOutcome};
use crate::event::ErrorCode;

#[derive(Serialize, Deserialize, JsonSchema, Debug)]
pub struct WriteArgs {
    /// Path relative to the project root, or absolute if inside the root.
    pub path: String,
    /// Complete file content. The whole file is replaced.
    pub content: String,
}

pub struct WriteTool;

#[async_trait::async_trait]
impl Tool for WriteTool {
    fn name(&self) -> &'static str { "write" }
    fn description(&self) -> &'static str { "Write content to a file, replacing any existing content. Path is relative to the project root or absolute within the project. Uses atomic temp+rename." }
    fn schema(&self) -> schemars::Schema { schemars::schema_for!(WriteArgs) }
    async fn execute(&self, args: serde_json::Value, ctx: ToolContext, _events: ToolEventSink, cancel: CancellationToken) -> ToolResult {
        // 1. Parse args.
        // 2. safe_resolve. Reject if escapes project root.
        // 3. Resolve nearest existing parent. Reject if parent doesn't exist OR is outside root.
        // 4. Create parent dirs as needed.
        // 5. Write to a temp sibling file (e.g. <path>.tmp.<ulid>), fsync, atomic rename.
        //    On Unix: tokio::fs::rename. On Windows: tokio::fs::rename (best-effort).
        // 6. Compute diff summary using `similar` crate. Apply redaction patterns to the diff.
        // 7. Return ToolOutcome::Success { output: <diff summary>, truncated: false }.
        todo!()
    }
}

impl WriteTool {
    pub fn new() -> Self;
}
```

**Acceptance:**
- 14+ unit tests:
  1. write to a new file → file created with content
  2. write to an existing file → content replaced
  3. write with parent dir missing → parent created
  4. write with parent dir permission denied → typed error
  5. write to a path that escapes via `..` → PathEscape
  6. write to an absolute path outside root → PathEscape
  7. write through a symlink parent that escapes → PathEscape
  8. write to a path inside a symlink loop → typed error
  9. write with the same content as the existing file → no-op (or short no-op message)
  10. write with very large content (10MB) → no panic
  11. write a file that is then read back → content matches
  12. write a file that contains an API key → diff summary redacts the key
  13. cancellation during write → temp file is cleaned up, original untouched
  14. write a file in a subdirectory → subdirectory created
  15. write to a path whose parent is a file (not a dir) → typed error

**Forbidden:**
- No `unsafe`.
- No `unwrap`/`expect` in library code.
- No `panic!`.
- No `std::fs::write` on a path that hasn't been `safe_resolve`d.
- No logging the file content (only the diff summary, and the diff is redacted).

**Dependencies:** `similar` already in Cargo.toml. `tempfile` already.
