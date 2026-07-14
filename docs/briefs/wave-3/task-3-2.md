### Task 3.2 — `edit` tool

`src/tool/edit.rs`:
- Args: `{ path: string, old: string, new: string }`
- Reads the file, finds `old`. **Fails** if `old` occurs 0 times or more than 1 time. The failure is a `ToolError` (not a panic) that the agent loop turns into a `ToolResult { is_error: true }`.
- Preserves the file's existing line ending (CRLF / LF) where detectable
- Uses the same atomic temp+rename as `write`
- Returns `{ match_position, new_bytes, diff_summary }`

**Spec:** §13 edit tool.
**Acceptance:** 12+ tests — exactly 1 match, 0 matches, 2 matches, 3 matches, line-ending preservation, missing file, symlink target, large file (10MB), secret redaction in diff.
