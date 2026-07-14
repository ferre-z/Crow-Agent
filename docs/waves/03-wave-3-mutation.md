---
type: wave-plan
status: detailed
wave: 3
phase: 2 + 3 (per 07-Build-Roadmap.md)
parent: 00-master-plan.md
---

# Wave 3 — Mutation tools + crash recovery (Phases 2+3)

**Goal:** `write`, `edit`, `bash` tools + interrupted-run recovery. Green gate: force-kill the process at every recorded state → restart → valid resumable session.

**Worktree:** `.worktrees/wave-3-mutation/` on branch `wave-3-mutation`. Wave 2 must be merged.
**Builds on:** Wave 2 (agent loop, tool registry, session, cancellation).

> **Spec reminder:** v0 spec §3.2 explicitly excludes **OS-level sandboxing, MCP, skills, plugins, hooks, subagents, swarms**. We do NOT add `bubblewrap`, `landlock`, or any sandbox. We rely on process-group termination + project-root confinement (in v0, "confinement" for the dedicated file tools only — bash is full user-level by spec design).

## Dependency map

```
3.1 write tool  ─────┐
                     ├── 3.4 crash recovery ── 3.6 sessions + resume
3.2 edit tool   ─────┤
                     │
3.3 bash tool   ─────┴── 3.5 symlink/path escape security tests
```

Wave 3 dispatches in 3 rounds:
- **Round H (parallel):** 3.1 + 3.2 + 3.3
- **Round I (1 task):** 3.4
- **Round J (parallel):** 3.5 + 3.6

## Tasks

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

### Task 3.2 — `edit` tool

`src/tool/edit.rs`:
- Args: `{ path: string, old: string, new: string }`
- Reads the file, finds `old`. **Fails** if `old` occurs 0 times or more than 1 time. The failure is a `ToolError` (not a panic) that the agent loop turns into a `ToolResult { is_error: true }`.
- Preserves the file's existing line ending (CRLF / LF) where detectable
- Uses the same atomic temp+rename as `write`
- Returns `{ match_position, new_bytes, diff_summary }`

**Spec:** §13 edit tool.
**Acceptance:** 12+ tests — exactly 1 match, 0 matches, 2 matches, 3 matches, line-ending preservation, missing file, symlink target, large file (10MB), secret redaction in diff.

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

### Task 3.4 — Crash recovery

`src/session.rs` (extend) + new `src/recovery.rs`:
- On `read_entries`, if a session file's last line is truncated (no terminating newline) or unparseable, mark that line as `RunInterrupted { active_call: <last seen tool call id, if any> }` and return the rest
- On resume (`crow --resume ID` and on agent startup if the user opts to "continue last"), the agent loop starts a new run with the prior `Conversation` preloaded, and the next durable entry is `RunInterrupted` (if not already written) followed by normal entries
- A small state machine: `{ Idle, Streaming(provider), Streaming(tool), Persisting }` — on session open we look at the last `SessionEntry` and emit a `RunInterrupted` for whatever state the previous run died in

**Spec:** §10, §16, §18 (acceptance criterion 7: "A forced crash during an operation produces a valid recoverable session").
**Acceptance:** 10+ tests — truncate last line at every byte position from 0 to "last line length" inclusive, resume yields correct history, double resume is idempotent, no seq gap, `RunInterrupted` carries the right `active_call` for each state.

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

### Task 3.6 — `crow sessions` + `crow --resume` + integration test sweeps

- `crow sessions` — lists sessions newest-first, prints `(id, started, last_status, message_count)`, table format
- `crow --resume ID` — reopens a session, replays history to stdout, then drops into `crow exec` mode for further input
- New `tests/integration/full_scenarios.rs` runs the v0 spec §18 acceptance criteria 1–9 as integration tests against a temporary Git repo, using the scripted provider (so they pass without network)

**Spec:** §15, §18.
**Acceptance:** 8+ tests, all the spec's "demonstrated in a temporary Git repository" criteria, except TUI-specific ones (deferred to wave 4).

## Review gate

Two reviewers per task, same as prior waves.

**Additional reject condition for wave 3:** any reviewer that finds a path escape or symlink bypass test that the implementer marked `#[ignore]` or skipped gets an automatic re-dispatch. Path escape tests are not optional.

## Decision log updates

- `docs/decisions/04-secret-redaction-patterns.md` — what patterns the redaction list matches, where it lives, and the explicit statement that it's not a security boundary.
- `docs/decisions/05-trust-model-v0.md` — restate the spec's "trusted-user tool" model and what wave 3 does NOT add (no sandbox, no permission prompts).
