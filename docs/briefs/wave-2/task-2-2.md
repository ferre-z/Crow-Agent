### Task 2.2 — `genai` 0.6.5 real provider adapter

**Files:**
- Create: `src/provider/genai.rs`
- Modify: `src/provider/mod.rs` (add `pub mod genai;` behind a feature flag is OUT — we always include it)
- Modify: `src/lib.rs` (no change — genai is internal)

**Spec references:** v0 spec §9 (Provider trait, ModelRequest, ProviderStream), §8 (config: `base_url`, `model`, `api_key_env = "NVIDIA_API_KEY"`).

**Why this exists:** the scripted mock provider (task 1.4) covers tests; this task provides the real Nemotron Ultra provider for production use. The adapter maps `genai::Client` events to the project-owned `AgentEvent` stream via the `StreamAccumulator` (task 2.1).

**Interfaces (exact):**

```rust
// src/provider/genai.rs
use genai::Client;
use genai::chat::{ChatRequest, ChatStreamEvent};
use crate::event::{AgentEvent, ErrorCode, StopReason, Usage};
use crate::provider::stream::{ProviderChunk, StreamAccumulator, StreamError};
use crate::provider::{ModelRequest, Provider, ProviderStream, ProviderError};
use tokio_util::sync::CancellationToken;

pub struct GenaiProvider {
    client: Client,
    model: String,
}

impl GenaiProvider {
    /// Build a provider from explicit config. Reads the API key from
    /// `api_key_env` at construction time; if missing, returns
    /// `ProviderError::Upstream` with code "missing_api_key".
    pub fn from_env(
        base_url: &str,
        model: &str,
        api_key_env: &str,
    ) -> Result<Self, ProviderError>;

    /// Build a provider with an explicit API key. Used by tests.
    pub fn with_api_key(
        base_url: &str,
        model: &str,
        api_key: String,
    ) -> Self;
}

#[async_trait::async_trait]
impl Provider for GenaiProvider {
    async fn stream(
        &self,
        req: ModelRequest,
        cancel: CancellationToken,
    ) -> Result<ProviderStream, ProviderError> {
        // Build a genai::chat::ChatRequest from our ModelRequest,
        // call client.exec_chat_stream, wrap the resulting
        // genai::chat::ChatStreamEvent stream in a StreamAccumulator
        // and yield ProviderChunk's. The stream ends when the
        // accumulator finishes or cancellation fires.
        todo!()  // IMPLEMENTATION
    }
}
```

**Mapping genai::chat::ChatStreamEvent → ProviderChunk:**

| genai event | ProviderChunk |
|---|---|
| `ChatStreamEvent::Start` | `ProviderChunk::Started` |
| `ChatStreamEvent::Chunk(text)` | `ProviderChunk::TextDelta { text }` |
| `ChatStreamEvent::ReasoningChunk(r)` | `ProviderChunk::ReasoningDelta { text: r }` |
| `ChatStreamEvent::ToolCallStart { id, name, .. }` | `ProviderChunk::ToolCallStart { call_id: id, name }` |
| `ChatStreamEvent::ToolCallChunk { id, fragment, .. }` | `ProviderChunk::ToolArgumentsDelta { call_id: id, fragment }` |
| `ChatStreamEvent::ToolCallEnd { id, .. }` | `ProviderChunk::ToolCallComplete { call_id: id }` |
| `ChatStreamEvent::End { usage, stop_reason, .. }` | `ProviderChunk::Completed { usage, stop_reason }` |
| `ChatStreamEvent::Error(e)` | `ProviderChunk::Failed { code: "upstream", message: e.to_string(), retryable: true }` |

Note: actual genai 0.6.5 enum names may differ. The implementer MUST read `genai-0.6.5/src/chat/stream.rs` and adapt the mapping. The brief is the contract for the *behavior*, not the exact genai enum names.

**Acceptance:**
- 4+ unit tests using the `with_api_key` constructor and a mock genai::Client (via the `genai::adapter::TestResolver` or equivalent). Tests verify:
  1. `Started` → first emitted `AgentEvent` is `ModelStarted`
  2. `TextDelta` chunks are passed through
  3. `ToolCallStart` + `ToolCallChunk` + `ToolCallEnd` produces exactly one `ToolStarted` with merged args
  4. `End` with `StopReason::EndTurn` produces `ModelFinished { stop_reason: EndTurn }`
- 1 opt-in live smoke test gated on `NVIDIA_API_KEY` env var. Without the key, `#[ignore]`. With the key, a 1-turn, 1-tool-call test that does NOT log response content (only token counts).
- All public types derive `Debug, Clone` where appropriate.
- Gate: `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test` clean.

**Forbidden:**
- No `unwrap`/`expect` in library code (test code is fine).
- No raw `genai` types exposed outside this file. The accumulator is the only translation point.
- No blocking IO in async fn.
- No `panic!` in the stream loop.

**Live test budget:** the live smoke test must:
- Use a `budget_capped_smoke!` macro or similar that aborts if more than 5 seconds / 1000 tokens / 1 retry elapse.
- Never log the assistant's response content unless `RUST_LOG=crow::provider::genai=debug` is set explicitly.
- Skip cleanly with `eprintln!("NVIDIA_API_KEY not set, skipping live smoke")` if the env var is missing.

**Reference for the implementer:**
- `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/genai-0.6.5/` — read the source for the actual enum names
- `docs/decisions/03-rust-toolchain.md` and `04-rust-toolchain-wave-2.md` for the toolchain
- Task 2.9 produces `docs/decisions/05-nemotron-genai-api.md` which has the Nemotron-specific quirks; this task may want to reference that doc once it lands
