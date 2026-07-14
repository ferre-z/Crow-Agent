### Task 2.1 — Provider-neutral stream processor

**Files:**
- Create: `src/provider/stream.rs`
- Modify: `src/provider/mod.rs` (re-export `StreamAccumulator` and `AccumulatedStream`)
- Modify: `src/lib.rs` (re-export)

**Spec references:** v0 spec §9 (provider events `Started/TextDelta/ReasoningDelta/ToolCallDelta/Completed/Failed`).

**Why this exists:** genai 0.6.5 streams events in fragmented chunks. A single `TextDelta("Hel")` + `TextDelta("lo")` may arrive as two separate chunks. Tool calls arrive as `ToolCallStart(name, call_id)` followed by `ToolArgumentsDelta(args, fragment)` until a `ToolCallComplete`. The accumulator turns the chunk stream into the project-owned `AgentEvent` sequence.

**Interfaces (exact):**

```rust
// src/provider/stream.rs
use tokio_util::sync::CancellationToken;
use crate::event::{AgentEvent, ErrorCode, StopReason, Usage, ToolCallId, ToolStream};
use crate::message::Part;
use futures::Stream;  // re-export from `futures` crate (added in wave 2 Cargo.toml)
use serde_json::Value;

/// A tool-call currently being assembled from streaming arguments.
#[derive(Debug, Clone)]
struct PendingToolCall {
    call_id: ToolCallId,
    name: String,
    args_buf: String,
}

/// Buffers fragmented provider chunks and yields `AgentEvent`s in source order.
///
/// The accumulator is the ONLY translation layer between provider-native
/// streams and the project-owned `AgentEvent`. No raw provider events
/// escape past this point.
#[derive(Debug, Default)]
pub struct StreamAccumulator {
    pending: Vec<PendingToolCall>,
    text_buf: String,
    reasoning_buf: String,
    finished: bool,
}

impl StreamAccumulator {
    pub fn new() -> Self { Self::default() }

    /// Push one provider chunk. Returns 0..N `AgentEvent`s to emit.
    /// Returns Err on malformed input — the caller is responsible for
    /// emitting a `Failed` AgentEvent and stopping the run.
    pub fn push_chunk(&mut self, chunk: ProviderChunk) -> Result<Vec<AgentEvent>, StreamError>;

    /// Signal end of stream. Returns the final `Completed` event or
    /// Err if the stream ended in an invalid state (e.g. partial tool call).
    pub fn finish(&mut self) -> Result<AgentEvent, StreamError>;
}

/// A single chunk from any provider (genai-shaped; other providers
/// can wrap their own chunks into this enum).
#[derive(Debug, Clone)]
pub enum ProviderChunk {
    Started,
    TextDelta { text: String },
    ReasoningDelta { text: String },
    ToolCallStart { call_id: ToolCallId, name: String },
    ToolArgumentsDelta { call_id: ToolCallId, fragment: String },
    ToolCallComplete { call_id: ToolCallId },
    Completed { usage: Usage, stop_reason: StopReason },
    Failed { code: String, message: String, retryable: bool },
}

#[derive(Debug, thiserror::Error)]
pub enum StreamError {
    #[error("stream invalid: {0}")] Invalid(String),
    #[error("UTF-8 boundary error in stream")] Utf8,
    #[error("double Completed")] DoubleCompleted,
    #[error("stream ended with pending tool call: {0}")] PendingToolCall(ToolCallId),
}
```

**Critical semantics (must match the test cases):**
- `Started` → emit `AgentEvent::ModelStarted`
- `TextDelta { text }` → accumulate, emit `AgentEvent::TextDelta { text }` for each chunk (don't merge)
- `ReasoningDelta { text }` → same, with `AgentEvent::ReasoningDelta`
- `ToolCallStart` → open a `PendingToolCall` entry
- `ToolArgumentsDelta` → append to the matching `PendingToolCall::args_buf`
- `ToolCallComplete` → parse the args_buf as `serde_json::Value`, emit `AgentEvent::ToolStarted { call_id, name, args }`
- `Completed` → emit `AgentEvent::ModelFinished { usage, stop_reason }`
- `Failed` → emit `AgentEvent::RunFailed { code, retryable, message }` (field order matches spec §9)
- UTF-8: chunks may split a multi-byte char. The accumulator must NOT panic; it should emit what it has and let the caller decide. For v0 we accept that a chunk containing an incomplete UTF-8 sequence is a `StreamError::Utf8` and aborts the run.
- Two `Completed` chunks → `StreamError::DoubleCompleted`
- `finish()` called with a pending `ToolCallStart` that never got `ToolCallComplete` → `StreamError::PendingToolCall`

**Acceptance:**
- 10+ unit tests covering:
  1. `TextDelta` chunks emit one `AgentEvent` each
  2. `ReasoningDelta` same
  3. `ToolCallStart` + multiple `ToolArgumentsDelta` + `ToolCallComplete` produces one `ToolStarted` with the concatenated JSON
  4. `ToolCallStart` with no `ArgumentsDelta` produces `ToolStarted` with `args = serde_json::Value::Null`
  5. UTF-8 split: a chunk ending with a partial multi-byte sequence is rejected with `StreamError::Utf8` (acceptable for v0; document this)
  6. Two `Completed` chunks → `StreamError::DoubleCompleted`
  7. `finish()` with no `Completed` → error
  8. `finish()` with pending tool call → `StreamError::PendingToolCall`
  9. `Failed` chunk emits `RunFailed` with field order `code, retryable, message` (spec §9)
  10. Empty stream (only `Started` then `Completed`) emits `ModelStarted` + `ModelFinished`
- All public types derive `Debug, Clone` where appropriate.
- `cargo test --lib` passes with the new tests.
- `cargo fmt --all --check` and `cargo clippy --all-targets --all-features -- -D warnings` clean.

**Forbidden:**
- No `genai` import in `stream.rs` (the accumulator is provider-agnostic).
- No real network IO.
- No `unwrap`/`expect` in library code.
- No newtype-variant enum members in `ProviderChunk` (we use struct variants per the wave 1 lesson: `#[serde(tag = "...")]` requires them, and the project policy is "struct variants everywhere").

**Dependency:** add `futures = "0.3"` to `[dependencies]` in Cargo.toml.
