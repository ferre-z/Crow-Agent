### Task 5.7 — IPC bridge (Tauri commands + events)

**Files:**
- Modify: `crates/crow-desktop/src/lib.rs` (the Tauri command registry)
- New: `crates/crow-desktop/src/ipc.rs` (the JSON-RPC client wrapper)

**Why this exists:** the frontend is sandboxed; it can't directly call into the `crow serve` child process. The Tauri IPC layer bridges them.

**Interfaces (exact):**

```rust
// crates/crow-desktop/src/ipc.rs
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use crate::server::protocol::{Request, Response, StreamItem};

pub struct IpcClient {
    write: tokio::sync::mpsc::UnboundedSender<String>,
    pending: Arc<Mutex<HashMap<ulid::Ulid, tokio::sync::oneshot::Sender<Response>>>>,
    events: tokio::sync::mpsc::UnboundedReceiver<StreamItem>,
}

impl IpcClient {
    pub fn spawn(child_stdout: tokio::process::ChildStdout, child_stdin: tokio::process::ChildStdin) -> Self;
    pub async fn send(&self, req: Request) -> Result<Response, IpcError>;
    pub fn events(&mut self) -> impl futures::Stream<Item = StreamItem>;
}
```

```rust
// In src/lib.rs
fn register_commands<R: tauri::Runtime>(app: &mut tauri::App<R>) {
    // Tauri commands:
    //   crow://session/new         -> SessionStart
    //   crow://session/list        -> SessionList
    //   crow://session/load        -> SessionLoad
    //   crow://session/submit      -> Submit
    //   crow://session/interrupt   -> Interrupt
    //   crow://project/pick        -> pick_project dialog
    //   crow://policy/show         -> show the effective policy
    //
    // Tauri events (server -> frontend):
    //   crow://event/agent         -> AgentEvent
    //   crow://event/tool_started  -> ToolStartedEvent (with diff if available)
    //   crow://event/run_finished  -> RunFinishedEvent
}
```

**Procedure:**
1. Implement `IpcClient`:
   - Spawn 2 tasks: a writer (reads commands from a channel, writes JSON lines to child stdin) and a reader (reads JSON lines from child stdout, dispatches to either the pending `pending` map or the `events` channel).
   - `send()` registers a `oneshot::Sender` in `pending` keyed by the request ID, then writes the request to stdin.
2. Wire Tauri commands to `IpcClient::send()`.
3. Wire the events stream to Tauri `app.emit("crow://event/agent", ...)` calls.
4. Tests:
   - IpcClient unit test: spawn a fake `crow serve` (a tiny binary that reads lines, echoes, and exits) and exercise the protocol.
   - Integration test: full Tauri app, submit a message, see the event in the frontend.

**Acceptance:**
- All 6 Tauri commands work end-to-end.
- The event stream is forwarded in real time (no buffering).
- A unit test for `IpcClient` with a fake server.
- `cargo build --workspace` is clean.

**Forbidden:**
- No `unwrap`/`expect` in library code.
- No buffering events on the frontend (we want sub-100ms latency).
- No `tokio::spawn` that holds a `MutexGuard` across `await`.

**Dependency:** `tokio`, `serde_json`, `ulid`, `futures` already in.
