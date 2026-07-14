---
type: wave-plan
status: detailed
wave: 1
phase: 0 (per 07-Build-Roadmap.md)
parent: 00-master-plan.md
---

# Wave 1 — Foundation (Phase 0)

**Goal:** Deterministic skeleton that can replay a scripted provider stream end-to-end.
**Acceptance gate:** scripted multi-turn replay produces byte-stable normalized events; two observers see the same ordered event sequence; malformed input returns typed errors; interrupt completes within a fixed timeout.

**Worktree:** `.worktrees/wave-1-foundation/` on branch `wave-1-foundation`.
**Rust edition:** 2021. **MSRV:** 1.75 (per `genai` 0.6.5).

## Dependency map

```
1.1 Cargo workspace + CI
 └── 1.2 ID + event-envelope types ──┬── 1.3 JSONL session writer
                                      ├── 1.4 Scripted mock provider
                                      └── 1.5 Cancellation primitive
                                                                       └── 1.6 Public API smoke
```

Wave 1 ships in 3 dispatch rounds:
- **Round A (1 task):** 1.1 Cargo workspace + CI
- **Round B (2 tasks, parallel):** 1.2 + 1.5
- **Round C (3 tasks, parallel):** 1.3 + 1.4 + 1.6

## Tasks

### Task 1.1 — Cargo workspace + CI

**Files:**
- Create: `Cargo.toml`, `rust-toolchain.toml`, `clippy.toml`, `.github/workflows/ci.yml`
- Create: `src/main.rs` (skeleton, prints "crow v0.0.0" and exits)
- Create: `src/lib.rs` (re-exports nothing yet, just exists so integration tests can import)

**Spec references:** v0 spec §6 (project structure), §15 (CLI).

**Interfaces:**
- `crow` binary compiles, runs, prints version.
- `crow::` lib crate compiles.
- CI runs fmt + clippy + test on push.

**Acceptance:**
- `cargo build --release` exits 0
- `cargo fmt --all --check` exits 0
- `cargo clippy --all-targets --all-features -- -D warnings` exits 0
- `cargo test` exits 0 (no tests yet, just runs)
- CI workflow file is valid YAML and references real actions

**Forbidden:** No `async-trait` yet (added in 1.2 if needed). No business logic. No git hooks. No release config.

---

### Task 1.2 — ID + event-envelope + message types

**Files:**
- Create: `src/ids.rs`, `src/event.rs`, `src/message.rs`
- Modify: `src/lib.rs` (re-export public types)

**Spec references:** v0 spec §10 (message + event model).

**Interfaces (exact — implementers MUST use these names):**

```rust
// ids.rs
pub type Ulid = ulid::Ulid;
pub fn new_id() -> Ulid;
pub struct SessionId(pub Ulid);
pub struct RunId(pub Ulid);
pub struct MessageId(pub Ulid);
pub struct ToolCallId(pub Ulid);
pub struct ToolResultId(pub Ulid);
```

```rust
// event.rs
pub const SCHEMA_VERSION: u32 = 1;

pub enum AgentEvent {
    RunStarted { run_id: RunId, session_id: SessionId, started_at: chrono::DateTime<chrono::Utc> },
    ModelStarted,
    TextDelta(String),
    ReasoningDelta(String),
    ToolStarted { call_id: ToolCallId, name: String, args: serde_json::Value },
    ToolOutput { call_id: ToolCallId, chunk: Vec<u8> },
    ToolFinished { call_id: ToolCallId, result: ToolOutcome },
    ModelFinished { usage: Usage, stop_reason: StopReason },
    RunFinished { message: String },
    RunCancelled,
    RunFailed { code: ErrorCode, message: String, retryable: bool },
}

pub enum ToolOutcome {
    Success { output: String, truncated: bool },
    Error { code: ErrorCode, message: String },
}

pub struct Usage { pub input_tokens: u32, pub output_tokens: u32 }
pub enum StopReason { EndTurn, ToolUse, MaxTokens, Cancellation, Error }
pub struct ErrorCode(pub String); // e.g. "stream_invalid", "tool_timeout"
```

```rust
// message.rs
pub enum Role { System, User, Assistant, ToolResult }

pub struct Message {
    pub id: MessageId,
    pub role: Role,
    pub parts: Vec<Part>,
}

pub enum Part {
    Text(String),
    Reasoning(String),
    ToolCall { id: ToolCallId, name: String, args: serde_json::Value },
    ToolResult { call_id: ToolCallId, output: String, is_error: bool },
}

pub struct Conversation { pub messages: Vec<Message> }
impl Conversation {
    pub fn push(&mut self, m: Message);
    pub fn last_assistant(&self) -> Option<&Message>;
}
```

**Acceptance:**
- All types serialize to JSON and round-trip
- `SCHEMA_VERSION` is exposed and tested
- `Debug`/`Clone`/`PartialEq` derives correct (no `Serialize` on secrets)
- Tests cover: ID uniqueness, message round-trip, event ordering invariant
- `cargo test` exits 0 with at least 8 new unit tests
- All public types have `///` doc comments

**Forbidden:** No provider-specific types. No `genai` import yet. No `async_trait` in types themselves. No `Display` impls that leak internals.

---

### Task 1.5 — Hierarchical cancellation primitive

**Files:**
- Create: `src/cancel.rs`
- Modify: `src/lib.rs` (re-export)

**Spec references:** v0 spec §4 (cancellation), §11 (cancellation in loop).

**Interfaces (exact):**

```rust
// cancel.rs
use std::time::Duration;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Default)]
pub struct CancelScope { /* ... */ }

impl CancelScope {
    pub fn new() -> Self;
    pub fn root(&self) -> CancellationToken;
    pub fn child(&self) -> (CancelScope, CancellationToken);
    pub fn cancel(&self);
    pub fn is_cancelled(&self) -> bool;
    pub async fn cancelled(&self) -> ();
    pub async fn cancelled_or_timeout(&self, timeout: Duration) -> CancelOutcome;
}

pub enum CancelOutcome { Cancelled, TimedOut }
```

**Acceptance:**
- Child cancellation propagates upward only on demand (default: child cancel does NOT cancel parent — that's parent-only)
- Parent cancel cancels all children
- `cancelled_or_timeout(1ms)` returns within 50ms even on a healthy token
- Tests: 6+ cases — child cancel does not affect parent, parent cancel affects child, timeout fires, double cancel is idempotent, drop cancels child

**Forbidden:** No `unsafe`. No global state. No `tokio::spawn` from inside.

---

### Task 1.3 — JSONL session writer

**Files:**
- Create: `src/session.rs`
- Modify: `src/lib.rs` (re-export)

**Spec references:** v0 spec §10 (durable entries), §16 (crash recovery).

**Interfaces:**

```rust
pub struct SessionWriter {
    file: std::fs::File,
    path: std::path::PathBuf,
    seq: u64,
}

pub enum SessionEntry {
    SessionStarted { session_id: SessionId, schema_version: u32, started_at: chrono::DateTime<chrono::Utc>, cwd: std::path::PathBuf },
    UserMessage { id: MessageId, content: String },
    AssistantMessage { id: MessageId, parts: Vec<Part>, usage: Option<Usage>, stop_reason: Option<StopReason> },
    ToolStarted { call_id: ToolCallId, name: String, args: serde_json::Value },
    ToolFinished { call_id: ToolCallId, outcome: ToolOutcome },
    RunFinished { message: String },
    RunInterrupted { active_call: Option<ToolCallId> },
}

impl SessionWriter {
    pub async fn open(path: impl AsRef<std::path::Path>) -> Result<Self, SessionError>;
    pub async fn append(&mut self, entry: SessionEntry) -> Result<(), SessionError>;
    pub async fn finish(&mut self) -> Result<(), SessionError>;
    pub fn path(&self) -> &std::path::Path;
    pub fn seq(&self) -> u64;
}

pub async fn read_entries(path: impl AsRef<std::path::Path>) -> Result<Vec<SessionEntry>, SessionError>;
pub async fn list_sessions(dir: impl AsRef<std::path::Path>) -> Result<Vec<SessionMeta>, SessionError>;
```

**Acceptance:**
- Each `append` writes exactly one JSON object + `\n` + fsync
- `seq` is monotonically increasing
- Crashing mid-write produces a truncated final line; `read_entries` skips lines that don't parse, returns the rest
- `list_sessions` returns newest-first
- Tests: 10+ — round-trip, crash mid-write (truncate file at byte 50, read returns valid prefix), seq invariant, concurrent append fails loudly, file permissions 0600

**Forbidden:** No SQLite. No compaction. No in-memory buffering. No background thread for flush.

---

### Task 1.4 — Scripted mock provider

**Files:**
- Create: `src/provider/mod.rs`, `src/provider/mock.rs`
- Create: `tests/fixtures/scripted_text_only.jsonl`
- Create: `tests/fixtures/scripted_text_plus_tool_call.jsonl`
- Create: `tests/fixtures/scripted_two_turns.jsonl`
- Modify: `src/lib.rs`

**Spec references:** v0 spec §9 (provider boundary, `Provider` trait shell).

**Interfaces:**

```rust
// provider/mod.rs
use async_trait::async_trait;
use crate::cancel::CancelScope;
use crate::event::{AgentEvent, Usage, StopReason};

pub struct ModelRequest { pub messages: Vec<crate::message::Message>, pub tools_schema: serde_json::Value }

#[derive(thiserror::Error, Debug)]
pub enum ProviderError {
    #[error("stream invalid: {0}")] StreamInvalid(String),
    #[error("upstream error: {code} {message}")] Upstream { code: String, message: String, retryable: bool },
    #[error("cancelled")] Cancelled,
}

pub struct ProviderStream {
    pub events: std::pin::Pin<Box<dyn tokio::sync::Stream<Item = Result<AgentEvent, ProviderError>> + Send>>,
}

#[async_trait]
pub trait Provider: Send + Sync {
    async fn stream(&self, req: ModelRequest, cancel: crate::cancel::CancelScope) -> Result<ProviderStream, ProviderError>;
}
```

```rust
// provider/mock.rs
pub struct ScriptedProvider { /* ... */ }

impl ScriptedProvider {
    pub fn from_fixture(path: impl AsRef<std::path::Path>) -> Result<Self, ProviderError>;
    pub fn from_events(events: Vec<AgentEvent>) -> Self;
}
```

**Fixture format (one event per JSON line):**
```json
{"type":"ModelStarted"}
{"type":"TextDelta","text":"Hello"}
{"type":"TextDelta","text":" world"}
{"type":"ModelFinished","usage":{"input_tokens":5,"output_tokens":2},"stop_reason":"EndTurn"}
{"type":"RunFinished","message":"done"}
```

**Acceptance:**
- `ScriptedProvider::from_fixture` loads JSONL, deserializes each line into `AgentEvent`
- `ScriptedProvider::stream` returns a `ProviderStream` that yields events in order
- All 3 fixtures load and replay
- Fragmented tool-call JSON is **not** required in 1.4 (added in 2.1) but the data model supports it
- Tests: 6+ — each fixture replays to identical event sequence, unknown event type fails loudly, empty file fails loudly, malformed line fails with line number

**Forbidden:** No real HTTP. No `genai` import. No `async_trait` macros outside the trait definition. No `unwrap` in library code.

---

### Task 1.6 — Public API smoke

**Files:**
- Create: `tests/phase0_smoke.rs`
- Modify: `src/lib.rs` (re-export enough for the test to import)

**Spec references:** v0 spec §18 (acceptance criteria 10 — "Unit and integration suites pass without network access").

**What it does:**
- Imports every public type from `crow::*`
- Constructs one of each: `SessionId`, `RunId`, `MessageId`, `ToolCallId`, `AgentEvent`, `Message`, `SessionEntry`, `Provider` (via `ScriptedProvider`)
- Replays a one-event fixture end-to-end: open session, append started, append user, stream scripted provider, append assistant, finish
- Asserts: every call succeeds, no panics, no unwraps leaked, session file exists, `read_entries` returns the same 5 entries

**Acceptance:**
- `cargo test --test phase0_smoke` exits 0
- Runs without network (no provider URL, no real HTTP)
- The full sequence is byte-stable across runs (test asserts on a SHA256 of the session file content with a fixed `started_at` injected)

**Forbidden:** No `unwrap`/`expect` in the test body. No `tokio::test` without a `current_thread` runtime feature flag.

---

## Review rubrics

Both reviewers use the same template, dispatched in parallel after the implementer returns. Two MiniMax M3 subagents per task.

### Spec reviewer prompt (abbreviated)

> You are reviewing a single task's diff against the v0 spec.
> - Spec source: `~/code/crow/docs/spec/08-Personal-Agent-v0-Spec.md` (copy of the vault note)
> - Diff: <printed by orchestrator>
> - Task brief: this file, section "Task N.M"
>
> Output format:
> 1. **Spec coverage** — for every requirement in the task brief, cite the spec section and confirm ✅/❌. If ❌, quote the missing requirement and the actual code.
> 2. **Interface conformance** — for every interface in the task brief, confirm the signature matches exactly (names, types, visibility).
> 3. **Out-of-scope check** — list anything added that the spec excludes (e.g. permissions, MCP, SQLite). Each must be ❌.
> 4. **Verdict:** ✅ SPEC PASS or ❌ SPEC FAIL (with numbered findings).

### Quality reviewer prompt (abbreviated)

> You are reviewing a single task's diff for code quality and the project's quality gate.
> - Diff: <printed by orchestrator>
> - Quality gate: `cargo fmt --all --check && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-targets --all-features`
>
> Output format:
> 1. **Gate evidence** — paste the actual `cargo` output for fmt, clippy, test. If the implementer didn't include it, ❌ immediately.
> 2. **YAGNI / scope discipline** — list anything added that wasn't asked for. Each = ⚠️ Minor.
> 3. **Test coverage** — for every edge case the brief calls out, confirm a test exists. List missing.
> 4. **Doc comments** — every public item has `///` doc. Missing = ⚠️ Minor.
> 5. **Verdict:** ✅ QUALITY PASS or ❌ QUALITY FAIL (with numbered findings).

## Reject conditions (any one = re-dispatch the implementer)

- Implementer didn't paste the actual `cargo test` output
- Spec reviewer found a missing or wrong requirement
- Quality reviewer found a Critical (compile error, test fail, clippy warning)
- Diff includes files outside the task's "Create / Modify" list
- New dependency added without a decision doc (`docs/decisions/NN-...md`)

## Decision log

- `docs/decisions/01-binary-name.md` — why `crow` not `pale`
