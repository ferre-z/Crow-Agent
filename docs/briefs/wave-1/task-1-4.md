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
use tokio_util::sync::CancellationToken;
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
    async fn stream(
        &self,
        req: ModelRequest,
        cancel: CancellationToken,
    ) -> Result<ProviderStream, ProviderError>;
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

**Fixture format (one event per JSON line, using `#[serde(tag = "type")]` on `AgentEvent`):**
```json
{"type":"ModelStarted"}
{"type":"TextDelta","text":"Hello"}
{"type":"TextDelta","text":" world"}
{"type":"ModelFinished","usage":{"input_tokens":5,"output_tokens":2},"stop_reason":"EndTurn"}
{"type":"RunFinished","message":"done"}
```

**Implementation note:** the `AgentEvent` enum in `src/event.rs` MUST be defined with `#[serde(tag = "type")]` and field names matching the fixture (e.g. `TextDelta(String)`, not `TextDelta { text: String }`). This is the explicit contract — the fixture format is the spec. Variant names use PascalCase: `ModelStarted`, `TextDelta`, `ReasoningDelta`, `ToolStarted`, `ToolOutput`, `ToolFinished`, `ModelFinished`, `RunStarted`, `RunFinished`, `RunCancelled`, `RunFailed`.

**Stop reason format:** `{"type":"ModelFinished","usage":{...},"stop_reason":"EndTurn"}` — `StopReason` serializes to its variant name as a string (not nested), so it needs `#[serde(rename_all = "PascalCase")]` and the corresponding `#[serde(rename_all_fields = "...")]` for the inner data. `RunStarted` carries `{run_id, session_id, started_at}`; `RunFinished` carries `{message}`; `RunFailed` carries `{code, message, retryable}`.

**Acceptance:**
- `ScriptedProvider::from_fixture` loads JSONL, deserializes each line into `AgentEvent`
- `ScriptedProvider::stream` returns a `ProviderStream` that yields events in order
- All 3 fixtures load and replay
- Fragmented tool-call JSON is **not** required in 1.4 (added in 2.1) but the data model supports it
- Tests: 6+ — each fixture replays to identical event sequence, unknown event type fails loudly, empty file fails loudly, malformed line fails with line number

**Forbidden:** No real HTTP. No `genai` import. No `async_trait` macros outside the trait definition. No `unwrap` in library code.

---
