### Task 3.5 — Symlink + path escape security tests

`tests/security/path_escape.rs`:
- Symlink swap mid-operation (the file is a regular file, becomes a symlink to `/etc/passwd` between read and write)
- `..` in path
- Absolute path outside project root
- Symlink that itself contains `..` segments
- Relative path with embedded symlink parent
- `read` on a path that resolves to a FIFO, socket, block device
- `edit` on a file owned by another user (permission denied → typed error, not panic)
- A `write` whose parent is a symlink that escapes the project root on canonicalize

**Spec:** §4 (project-root confinement), §18 (acceptance criteria).
**Acceptance:** 12+ tests, all must pass. None may use `unsafe` or disable checks.
**Forbidden:** No sandboxing additions — these tests prove the v0 trust model holds, not that we contain a malicious shell.
