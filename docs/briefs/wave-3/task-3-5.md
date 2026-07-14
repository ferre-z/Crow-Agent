### Task 3.5 — Symlink + path escape security tests

**Files:**
- Create: `tests/security/path_escape.rs`

**Spec references:** v0 spec §4 (project-root confinement), §18 (acceptance criterion 9: "No API key appears in the transcript, session, diagnostic log or panic output"), §3.2 (no sandboxing).

**Why this exists:** the v0 trust model relies on the file tools (read/write/edit) being unable to escape the project root. These tests prove the v0 model holds, not that we contain a malicious shell.

**Acceptance:**
- 12+ integration tests in `tests/security/path_escape.rs`:
  1. read path with `..` segments that resolve outside root → PathEscape
  2. read absolute path outside root → PathEscape
  3. read through a symlink that points outside root → PathEscape
  4. read through a symlink that itself contains `..` segments
  5. write to a path where the parent is a symlink that escapes root on canonicalize → PathEscape
  6. write through a symlink that points outside root → PathEscape
  7. edit a path that resolves to a FIFO → NotAFile
  8. edit a path that resolves to a socket → NotAFile
  9. edit a path that resolves to a block device → NotAFile
  10. edit a file owned by another user (permission denied) → typed Io error
  11. symlink swap mid-operation: file is a regular file at start, becomes a symlink to /etc/passwd between read and write → PathEscape (the write MUST fail)
  12. nested symlink chain that resolves outside root → PathEscape
  13. read a path inside root that contains a symlink to itself (loop) → typed error
  14. write a path that crosses a mount point boundary → typed error or success (depends on OS; we don't test for a specific outcome, only that the tool doesn't panic)

**Forbidden:**
- No `unsafe`.
- No `#[ignore]` on any test. Path escape tests are mandatory.
- No disabling checks (`#[allow(...)]`) to make a test pass.
- No mocking the file system (use real tempdirs and real symlinks).
- No sandboxing additions.

**Dependencies:** uses ReadTool/WriteTool/EditTool from earlier tasks.
