### Task 2.3 — `read` tool

`src/tool/read.rs`:
- Args: `{ path: string, offset?: u32, limit?: u32 }`
- Validates: path is inside project root (rejects `..`, absolute outside root, symlinks pointing outside)
- Rejects directories and binary files (sniff first 8KB for NUL)
- Returns `{ content: string, line_count, truncated, byte_size }`
- Capped at 1 MB returned bytes; reports truncation

**Spec:** §13 read tool contract, §4 path containment.
**Acceptance:** 12+ tests including path escape via `..`, symlink swap after canonicalize, binary file, empty file, line offset past EOF, limit smaller than file.
