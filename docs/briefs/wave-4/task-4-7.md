### Task 4.7 — App-server integration tests

**Files:**
- Create: `tests/app_server.rs` (consolidated test file; 4.1–4.6 each add cases here)

**Why this exists:** waves 1-3 had per-task test files. The app-server is one logical surface and one test file is easier to maintain.

**Acceptance:**
- 15+ integration tests in `tests/app_server.rs` covering:
  1. `Initialize` handshake (4.1)
  2. `SessionStart` returns a `SessionId` (4.2)
  3. `Submit` with a scripted provider returns `SubmitAck` + at least 1 `Event` (4.2)
  4. `Submit` for a multi-turn task streams events in order (4.2)
  5. `Interrupt` mid-run produces a `RunCancelled` event (4.2, 4.3)
  6. `SessionList` returns sessions for the right project (4.2)
  7. `SessionLoad` returns the full event history (4.2, 4.6)
  8. `Shutdown` returns `Bye` and the process exits 0 (4.2)
  9. Backpressure: a slow receiver blocks the sender (4.3)
  10. Disconnect: closing stdin cancels the in-flight run (4.3)
  11. Policy: a `Deny` policy emits a `ToolResult { is_error: true }` and continues (4.4)
  12. Policy: a custom `Ask` policy that always denies is honoured (4.4)
  13. Policy file: a project with `~/.config/crow/policy.toml` denying `rm -rf` denies a bash call starting with `rm -rf` (4.5)
  14. Policy file: an empty policy file = default allow (4.5)
  15. Replay: a session with 5 entries builds a replay with `live_event_count = 5` (4.6)

**Test infrastructure:**
- All tests spawn `crow serve` as a child process via `tokio::process::Command`.
- A `TestServer` helper struct owns the child, provides `send(req) -> resp` and `events() -> impl Stream<Item = Event>`.
- The `ScriptedProvider` is used as the default — no network.

**Procedure:**
1. Write a `TestServer` helper in `tests/app_server.rs` (or a `tests/common/mod.rs`):
   - `TestServer::spawn() -> Self` — runs `cargo run --bin crow -- serve` with a temp config dir
   - `pub async fn send(&mut self, req: Request) -> Response`
   - `pub fn events(&mut self) -> impl Stream<Item = Response>`
2. Write the 15 tests using the helper.
3. Add a `crow serve --test-mode` flag that uses ScriptedProvider regardless of any config — speeds up tests.

**Acceptance:**
- All 15+ tests pass with `cargo test --test app_server`.
- `cargo test --all-targets --all-features` is green.
- `cargo fmt --check`, `cargo clippy -D warnings` clean.

**Forbidden:**
- No real network. No real genai. No `NVIDIA_API_KEY` referenced.
- No `unwrap`/`expect` in test bodies — use `?` and `Result` returns.
- No `tokio::time::sleep` longer than 1s in any test.

**Dependency:** `tokio`, `serde_json`, `ulid` already in.
