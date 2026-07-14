### Task 3.1 — `write` tool

`src/tool/write.rs`:
- Args: `{ path: string, content: string }`
- Validates: path inside project root (same rules as `read`)
- Resolves nearest existing parent; creates missing parents
- Writes to a temp sibling file (e.g. `path.tmp.<ulid>`), fsync, atomic rename where supported
- On Unix: uses `rename(2)`. On Windows: best-effort + `replace_file` fallback
- Returns `{ bytes_written, created, diff_summary }`
- Secret redaction: the redaction list (API keys, common patterns) is applied to the **diff summary only**, never to the actual write

**Spec:** §13 write tool, §4 atomic file replacement.
**Acceptance:** 14+ tests — overwrite existing, new file, parent dir missing, parent dir permission denied, disk full, symlink parent, cross-device rename, secret redaction in diff.
