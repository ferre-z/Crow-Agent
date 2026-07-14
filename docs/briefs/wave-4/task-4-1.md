### Task 4.1 — App-server skeleton

**Files:**
- Create: `src/server/mod.rs`
- Create: `src/server/ipc.rs` (stdin/stdout line framing)
- Create: `src/server/protocol.rs` (JSON-RPC types)
- Modify: `src/main.rs` (`crow serve` subcommand)
- Modify: `src/lib.rs` (`pub mod server;`)
- Modify: `Cargo.toml` (add `serde_json` already there; add `tokio` features for stdin/stdout if missing)

**Why this exists:** the desktop app, the TUI, and the headless `crow exec` are all clients of one kernel. The kernel doesn't know about them. The app-server is the protocol layer that lets any client talk to the kernel without each one re-implementing the same logic.

**Spec references:** v0 spec §15 (CLI — `crow doctor` and the headless behavior extend naturally to `crow serve`), §18 (acceptance criterion 6: "Closing and reopening the program resumes completed conversation history" — the app-server is the natural host for this).

**Interfaces (exact):**

```rust
// src/server/protocol.rs
use serde::{Deserialize, Serialize};
use serde_json::Value;
use crate::event::AgentEvent;
use crate::message::Message;
use crate::ids::{SessionId, RunId, ToolCallId};
use ulid::Ulid;

pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "method", content = "params")]
pub enum Request {
    #[serde(rename = "initialize")]
    Initialize {
        client_version: String,
        client_capabilities: Vec<String>,
    },

    #[serde(rename = "session/start")]
    SessionStart {
        project_root: std::path::PathBuf,
        /// Optional model id; defaults to the one in `~/.config/crow/config.toml`.
        #[serde(default)]
        model: Option<String>,
    },

    #[serde(rename = "session/list")]
    SessionList {
        project_root: std::path::PathBuf,
    },

    #[serde(rename = "session/load")]
    SessionLoad {
        session_id: SessionId,
    },

    #[serde(rename = "submit")]
    Submit {
        session_id: SessionId,
        user_message: Message,
    },

    #[serde(rename = "interrupt")]
    Interrupt {
        session_id: SessionId,
        run_id: RunId,
    },

    #[serde(rename = "shutdown")]
    Shutdown,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "kind", content = "data")]
pub enum Response {
    /// First message from server. The server announces its protocol version
    /// and capabilities (model list, tools list, etc).
    #[serde(rename = "hello")]
    Hello {
        protocol_version: u32,
        server_version: String,
        capabilities: Vec<String>,
    },

    /// Single-shot response to a request.
    #[serde(rename = "reply")]
    Reply {
        request_id: Ulid,
        /// Either `Ok(Value)` or `Err { code, message }`.
        result: Result<Value, ErrorPayload>,
    },

    /// Streaming notification (no request_id — these are server-pushed).
    /// The client uses the session_id + run_id + seq to order them.
    #[serde(rename = "event")]
    Event {
        session_id: SessionId,
        run_id: RunId,
        seq: u64,
        event: AgentEvent,
    },

    /// Server is shutting down (after `Shutdown` request).
    #[serde(rename = "bye")]
    Bye,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ErrorPayload {
    pub code: String,
    pub message: String,
    /// Optional structured detail (e.g. a validation error report).
    #[serde(default)]
    pub data: Option<Value>,
}
```

**Wire format:** newline-delimited JSON. One JSON object per line. Logging goes to stderr; the JSON stream is on stdout and must not be polluted.

**Procedure (TDD-ish, since the protocol is the contract):**
1. Define all types in `protocol.rs`. Derive `Serialize, Deserialize, Debug, Clone, PartialEq`.
2. Write 4 unit tests in `protocol.rs`:
   - round-trip every `Request` variant
   - round-trip every `Response` variant
   - unknown `method` or `kind` fails to deserialize (but doesn't panic)
   - `protocol_version` is `1`
3. Implement `ipc.rs`:
   - `pub async fn read_request_line<R: AsyncBufRead>(r: &mut R) -> Result<Request, IpcError>`
   - `pub async fn write_response<W: AsyncWrite>(w: &mut W, r: &Response) -> Result<(), IpcError>`
   - Lines are newline-terminated UTF-8. Errors on lines > 1 MB (configurable).
4. Implement `server/mod.rs`:
   - `pub async fn run_server(config: ServerConfig) -> Result<(), ServerError>`
   - Reads lines from stdin, dispatches to handlers, writes responses to stdout
   - Spawns one task per `Submit` to stream events; the request handler returns immediately with `SubmitAck { run_id }`, then events flow on the same stdout line-stream
5. Wire up `crow serve` in `main.rs`.
6. Integration test: spawn `crow serve` in a child process, connect via stdin/stdout, send `Initialize`, expect `Hello` with protocol_version 1.

**Acceptance:**
- 6+ unit tests in `protocol.rs`
- 2+ unit tests in `ipc.rs` (round-trip + oversize line rejected)
- 1+ integration test in `tests/app_server.rs` (Initialize handshake)
- `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test` all clean
- The integration test does NOT use network (no genai, no real provider)

**Forbidden:**
- No `unwrap`/`expect` in library code (test code is fine).
- No `panic!` in the request loop — return a typed error and (for the client-facing case) a `Response::Reply` with `code: "internal"`.
- No `println!` / `eprintln!` from the server — use `tracing` macros that go to stderr.
- No new dependencies unless explicitly listed in this brief (we can do JSON without `jsonrpsee`; if you want to use it, Decision 06).

**Dependency:** none new. `serde`, `serde_json`, `tokio`, `tracing`, `ulid` are already in Cargo.toml.
