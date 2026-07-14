### Task 3.3 — `bash` tool

`src/tool/bash.rs`:
- Args: `{ command: string, timeout_seconds?: u32 }`
- Resolves the user's login shell: `$SHELL` if set, else `/bin/sh` on Unix, `cmd.exe` on Windows (the latter is best-effort, v0 ships with Unix-first)
- Spawns the command in the project cwd with a new process group
- Streams stdout AND stderr as `ToolOutput { call_id, chunk }` events (separate byte streams, tagged with stream id 0=stdout, 1=stderr)
- Captures both into bounded byte buffers (default 1 MB each, configurable)
- Honors `cancel`: kills the process group with `SIGTERM`, waits 5s, then `SIGKILL` remaining
- Honors `timeout`: same kill chain
- Returns `{ exit_code, elapsed_ms, stdout_truncated, stderr_truncated }`

**Spec:** §13 bash tool, §4 bounded output + process-group termination.
**Acceptance:** 18+ tests — exit 0, exit 1, command not found, command hangs (timeout fires), command spawns subprocess (subprocess dies with parent on cancel), command runs forever (cancel kills it), output flood (1 MB stream → truncation flag), 5s SIGKILL grace period, shell not in PATH fallback, command that writes 100 MB (capped at 1 MB), `cd` to a path that doesn't exist.
