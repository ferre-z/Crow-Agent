### Task 3.6 — `crow sessions` + `crow --resume` + full §18 integration suite

**Files:**
- Modify: `src/main.rs` + `src/cli.rs` (already partially implemented in task 2.7; add `crow sessions` table output and `crow --resume` history replay)
- Create: `tests/integration/full_scenarios.rs`
- Modify: `tests/cli.rs` (extend with sessions + resume)

**Spec references:** v0 spec §15 (CLI behavior), §18 (acceptance criteria 1-9 in a temporary Git repo).

**Acceptance:**
- 4+ integration tests in `tests/cli.rs`:
  1. `crow --version` exits 0
  2. `crow sessions` on an empty dir prints a header and exits 0
  3. `crow sessions` on a populated dir prints one row per session, newest first
  4. `crow --resume <id>` on a nonexistent id exits non-zero with a clear error
  5. `crow doctor` exits 0 when API key is missing (does NOT make a request)
  6. `crow doctor --live` exits 0 when API key is present and endpoint is reachable
- 8+ integration tests in `tests/integration/full_scenarios.rs` (using a temp Git repo + scripted provider):
  1. TUI starts, accepts a multiline task, streams scripted output (uses headless `crow exec` since TUI is wave 4)
  2. scripted provider reads at least two files through the read tool
  3. scripted provider edits a file, runs `cargo test` through bash, observes the result
  4. tool events appear in order
  5. Esc cancels model streaming (via cancel token)
  6. close + reopen: `crow --resume` reopens a session and history is intact
  7. forced crash during an operation: kill the process, restart, recovery emits RunInterrupted
  8. nested AGENTS.md instructions appear in the correct order in the captured scripted request
- 1+ negative test:
  - API key never appears in session file, log, or panic output (search for "sk-" or "nvapi-" pattern in all session/log output)

**Forbidden:**
- No real network (all scripted provider).
- No `#[ignore]` on the negative API key test.
- No redaction that mutates the diff (use the redaction from task 3.1).
- No TUI work (wave 4).

**Dependencies:** All wave 2 + wave 3 tasks must be merged first.
