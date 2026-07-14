### Task 4.3 — Event streaming with backpressure + cancellation

**Files:**
- Create: `src/server/streaming.rs`
- Modify: `src/server/handlers.rs` (the `handle_submit` forwarder uses `streaming.rs`)

**Why this exists:** the submit response streams events until the run finishes. If the client is slow, the server must not OOM. If the client disconnects (pipe closed), the server should cancel the run.

**Interfaces (exact):**

```rust
// src/server/streaming.rs
use tokio::sync::mpsc;
use crate::event::AgentEvent;
use crate::ids::{RunId, SessionId};

/// A bounded MPSC channel for streaming AgentEvents. Capacity is
/// configurable; default 1024. When full, the sender blocks (backpressure).
pub fn event_channel(capacity: usize) -> (
    mpsc::Sender<StreamItem>,
    mpsc::Receiver<StreamItem>,
);

#[derive(Debug, Clone)]
pub enum StreamItem {
    Event { run_id: RunId, seq: u64, event: AgentEvent },
    Done { run_id: RunId, final_outcome: FinalOutcome },
}

#[derive(Debug, Clone)]
pub enum FinalOutcome {
    Completed,
    Cancelled,
    Failed { code: String, message: String },
}

/// Forwarder: takes events from the agent (via a stream-of-events
/// `Stream<Item = AgentEvent>`), assigns a sequence number, and pushes
/// them on the channel. The forwarder terminates when the agent
/// stream ends or the cancel token fires.
pub async fn forward_events<S>(
    session_id: SessionId,
    run_id: RunId,
    agent_stream: S,
    cancel: tokio_util::sync::CancellationToken,
    tx: mpsc::Sender<StreamItem>,
) -> FinalOutcome
where
    S: futures::Stream<Item = Result<AgentEvent, crate::provider::ProviderError>> + Unpin;

/// Watcher: monitors the channel's receiver. If the receiver is
/// dropped (client disconnected), cancels the cancel token.
pub fn spawn_disconnect_watcher(
    rx: &mut mpsc::Receiver<StreamItem>,
    cancel: tokio_util::sync::CancellationToken,
) -> tokio::task::JoinHandle<()>;
```

**Backpressure policy:** the channel capacity is 1024. If the receiver is slow, the sender awaits. This blocks the agent loop, which is intentional — we don't want the agent to outrun the user.

**Client-disconnect policy:** when the client closes stdin (the IPC pipe), the receiver returns `None`. The watcher fires the cancel token. The agent loop's existing cancellation hooks (wave 1 task 1.5) ensure clean shutdown: the in-flight tool call gets a `SIGTERM` then `SIGKILL`; the session file gets a `RunInterrupted` entry.

**Procedure:**
1. Implement `event_channel` with the bounded mpsc.
2. Implement `forward_events`: pull from the agent stream, push to the channel. On `None` or `Err`, send a `Done` and return the appropriate `FinalOutcome`. On cancel, send a `Done::Cancelled` and return.
3. Implement `spawn_disconnect_watcher`: in a tokio task, await on the receiver. If the receiver returns `None`, cancel the token and exit.
4. In `handle_submit`, spawn the forwarder and watcher as separate tasks. Both are joined when the run ends.
5. Tests:
   - forwarder assigns monotonic seq numbers
   - forwarder handles stream errors with `Done::Failed`
   - forwarder respects cancel with `Done::Cancelled`
   - disconnect watcher fires cancel on receiver drop
   - backpressure: a slow receiver blocks the sender (test with a sync channel)

**Acceptance:**
- 6+ unit tests in `streaming.rs`.
- 2+ integration tests in `tests/app_server.rs`:
  - Submit a long-running task; the response stream is backpressured (test by having a slow handler that pulls one event at a time)
  - Submit, then close stdin → the agent run is cancelled and a `Done::Cancelled` is sent
- `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test` clean.

**Forbidden:**
- No `unwrap`/`expect` in library code.
- No `tokio::spawn` that holds a `MutexGuard` across an `await` (we learned this in wave 2).
- No unbounded channels anywhere in the server.
- No `tokio::time::sleep` for "give the client time to read" — use backpressure instead.

**Dependency:** `futures` already in Cargo.toml from wave 2 task 2.1.
