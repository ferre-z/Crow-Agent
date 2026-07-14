### Task 4.2 — Request/response handler dispatch

**Files:**
- Modify: `src/server/mod.rs` (add `dispatch()` function)
- Create: `src/server/handlers.rs` (one function per request variant)
- Create: `src/server/state.rs` (in-memory state: `Arc<Mutex<HashMap<SessionId, SessionHandle>>>`)

**Why this exists:** every `Request` variant needs a handler. The state map holds the live `Agent` instances keyed by `SessionId`. Handlers run on a `tokio::sync::Mutex` to serialize access.

**Interfaces (exact):**

```rust
// src/server/handlers.rs
use super::protocol::*;
use super::state::ServerState;
use std::sync::Arc;

pub async fn handle_initialize(
    state: Arc<ServerState>,
    req_id: ulid::Ulid,
    client_version: String,
    client_capabilities: Vec<String>,
) -> Response;

pub async fn handle_session_start(
    state: Arc<ServerState>,
    req_id: ulid::Ulid,
    project_root: std::path::PathBuf,
    model: Option<String>,
) -> Response;

pub async fn handle_session_list(
    state: Arc<ServerState>,
    req_id: ulid::Ulid,
    project_root: std::path::PathBuf,
) -> Response;

pub async fn handle_session_load(
    state: Arc<ServerState>,
    req_id: ulid::Ulid,
    session_id: crate::ids::SessionId,
) -> Response;

pub async fn handle_submit(
    state: Arc<ServerState>,
    req_id: ulid::Ulid,
    session_id: crate::ids::SessionId,
    user_message: crate::message::Message,
    event_tx: tokio::sync::mpsc::UnboundedSender<Response>,
) -> Response;  // returns Reply { request_id, Ok(SubmitAck{ run_id }) } immediately;
// The event stream is then pushed on `event_tx` by a spawned task.

pub async fn handle_interrupt(
    state: Arc<ServerState>,
    req_id: ulid::Ulid,
    session_id: crate::ids::SessionId,
    run_id: crate::ids::RunId,
) -> Response;

pub async fn handle_shutdown(
    state: Arc<ServerState>,
    req_id: ulid::Ulid,
) -> Response;
```

```rust
// src/server/state.rs
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use crate::ids::SessionId;
use crate::ids::RunId;
use crate::agent::{Agent, AgentConfig};
use crate::message::Message;
use crate::tool::ToolRegistry;
use crate::provider::Provider;
use crate::provider::mock::ScriptedProvider;
use crate::policy::ApprovalPolicy;

pub struct SessionHandle {
    pub agent: tokio::sync::Mutex<Agent>,
    pub cancel: CancellationToken,
    pub current_run: Option<RunId>,
}

#[derive(Default)]
pub struct ServerState {
    sessions: Mutex<HashMap<SessionId, Arc<SessionHandle>>>,
    /// Default provider is ScriptedProvider until task 2.2's genai adapter lands.
    default_provider: Mutex<Option<Arc<dyn Provider>>>,
    default_tools: Mutex<Option<Arc<ToolRegistry>>>,
    default_policy: Mutex<Option<Arc<dyn ApprovalPolicy>>>,
}

impl ServerState {
    pub async fn get_or_create_session(
        &self,
        session_id: SessionId,
        provider: Arc<dyn Provider>,
        tools: Arc<ToolRegistry>,
        policy: Arc<dyn ApprovalPolicy>,
        config: AgentConfig,
    ) -> Arc<SessionHandle>;
}
```

**Procedure:**
1. Build `state.rs` with a `ServerState` that holds a `Mutex<HashMap<SessionId, Arc<SessionHandle>>>`.
2. For `handle_session_start`, build an `Agent` with the default provider + tools + policy. Store in the state map.
3. For `handle_submit`, return `SubmitAck { run_id }` immediately, then spawn a tokio task that calls `agent.submit(user_msg)` and forwards every `AgentEvent` on the `event_tx` as `Response::Event`. The forwarder terminates when the agent returns or `cancel` fires.
4. For `handle_interrupt`, find the session, call `session.cancel()`, await the forwarder task's completion (it will emit a final `RunCancelled` event).
5. For `handle_session_list`, read sessions from `~/.local/share/crow/sessions/`, filter by `project_root`, return the metadata list.
6. For `handle_session_load`, call `read_entries(session_path)`, return the entries as a list.
7. For `handle_shutdown`, set a shutdown flag that the main loop checks; the loop then exits gracefully.

**Acceptance:**
- 8+ unit tests in `handlers.rs` covering: every handler returns a `Response::Reply` with the right shape.
- 4+ integration tests in `tests/app_server.rs`:
  - `Initialize` → `Hello` with protocol_version
  - `SessionStart` → `Reply { Ok: { session_id } }`
  - `Submit` → `Reply { SubmitAck }` followed by 1+ `Event` messages, terminating in `RunFinished`
  - `Interrupt` mid-run → events stop after a `RunCancelled`
  - `SessionList` returns the session metadata
  - `SessionLoad` returns the events
  - `Shutdown` → `Bye` and the process exits 0
  - Re-`SessionStart` with the same project_root reuses an existing session (or creates a new one — implementation choice)

**Forbidden:**
- No `unwrap`/`expect` in library code.
- No global mutable state. `ServerState` is held in an `Arc` and passed by reference.
- No `tokio::spawn` inside a handler that holds a `MutexGuard` — release the lock first, then spawn.

**Dependency:** none new. Uses `tokio`, `serde_json`, `ulid`, `async-trait` (already in).
