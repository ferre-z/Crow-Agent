### Task 3.2 — `edit` tool

**Files:**
- Create: `src/tool/edit.rs`

**Spec references:** v0 spec §13 edit tool.

**Why this exists:** the edit tool applies a precise text replacement. It's the right tool for surgical changes to existing files (the model gives an `old` snippet and a `new` snippet, we replace). Exact-match enforcement prevents accidental multi-replacement.

**Interfaces (exact):**

```rust
// src/tool/edit.rs
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use crate::tool::{Tool, ToolContext, ToolEventSink, ToolError, ToolResult, ToolOutcome};
use crate::event::ErrorCode;

#[derive(Serialize, Deserialize, JsonSchema, Debug)]
pub struct EditArgs {
    /// Path relative to the project root, or absolute if inside the root.
    pub path: String,
    /// Exact text to replace.
    pub old: String,
    /// Replacement text.
    pub new: String,
}

pub struct EditTool;

#[async_trait::async_trait]
impl Tool for EditTool {
    fn name(&self) -> &'static str { "edit" }
    fn description(&self) -> &'static str { "Replace an exact span of text in a file. Fails if `old` is not found exactly once. Path is relative to the project root or absolute within the project." }
    fn schema(&self) -> schemars::Schema { schemars::schema_for!(EditArgs) }
    async fn execute(&self, args: serde_json::Value, ctx: ToolContext, _events: ToolEventSink, cancel: CancellationToken) -> ToolResult {
        // 1. Parse args.
        // 2. safe_resolve. Reject if escapes.
        // 3. Read the file content. If missing → ToolError::Io(NotFound).
        // 4. Find `old` in the content. If 0 matches → ToolOutcome::Error { code: "no_match", message: "old text not found" }.
        // 5. If >1 match → ToolOutcome::Error { code: "multiple_matches", message: "old text found N times" }.
        // 6. Preserve the line ending style: detect whether the file uses CRLF, LF, or mixed. Use the dominant style for the replacement.
        // 7. Apply the edit. Write atomically (temp+rename, like WriteTool).
        // 8. Compute diff summary using `similar`. Redact secrets.
        // 9. Return ToolOutcome::Success.
        todo!()
    }
}
```

**Acceptance:**
- 12+ unit tests:
  1. exactly 1 match → edit applied
  2. 0 matches → ToolOutcome::Error with code "no_match"
  3. 2 matches → ToolOutcome::Error with code "multiple_matches"
  4. 3 matches → same as 2
  5. edit a file with CRLF line endings → output preserves CRLF
  6. edit a file with LF line endings → output preserves LF
  7. edit a file with mixed endings → use the dominant style
  8. edit a missing file → Io(NotFound)
  9. edit a path that escapes via `..` → PathEscape
  10. edit a file > 10MB → no panic
  11. edit a file whose `old` contains an API key → diff redacts it
  12. cancellation during edit → temp file cleaned up
  13. edit preserves the file's BOM if present

**Forbidden:**
- No `unsafe`.
- No `unwrap`/`expect` in library code.
- No `panic!` on 0 or many matches — return a typed error.
- No fuzzy matching. The match must be byte-exact.

**Dependencies:** same as 3.1.
