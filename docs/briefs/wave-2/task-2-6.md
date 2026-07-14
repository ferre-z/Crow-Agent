### Task 2.6 — Agent state machine

**Files:**
- Create: `src/agent.rs`
- Modify: `src/lib.rs` (`pub mod agent;`)

**Spec references:** v0 spec §11 (agent loop), §16 (failure and recovery), §18 (acceptance criteria 1-3, 5, 6).

**Why this exists:** the agent loop is the heart of Crow. It owns the conversation history, runs the model→tool→model cycle, and enforces all the limits (max_turns, max_tool_calls, output bounds, cancellation).

**Interfaces (exact):**

```rust
// src/agent.rs
use std::sync::Arc;
use std::path::PathBuf;
use tokio_util::sync::CancellationToken;
use crate::context::CompiledContext;
use crate::event::AgentEvent;
use crate::message::Message;
use crate::provider::Provider;
use crate::session::SessionWriter;
use crate::tool::ToolRegistry;

#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub max_turns: u32,
    pub max_tool_calls: u32,
    pub model: String,
    pub project_root: PathBuf,
    pub session_writer: SessionWriter,
}

pub struct Agent {
    config: AgentConfig,
    provider: Arc<dyn Provider>,
    tools: Arc<ToolRegistry>,
    cancel: CancellationToken,
    history: Vec<Message>,
}

impl Agent {
    pub fn new(
        config: AgentConfig,
        provider: Arc<dyn Provider>,
        tools: Arc<ToolRegistry>,
        cancel: CancellationToken,
        initial_history: Vec<Message>,
    ) -> Self;

    /// Append a user message and run the agent loop to completion.
    /// Returns the final `RunFinished` event.
    pub async fn submit(&mut self, user_msg: Message) -> Result<AgentEvent, AgentError>;

    /// Cancel the in-flight run. Cancellation is cooperative; the loop
    /// exits at the next safe point.
    pub fn cancel(&self);

    /// Current state.
    pub fn state(&self) -> AgentState;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentState {
    Idle,
    Sampling { turn: u32 },
    ExecutingTool { turn: u32, call_id: ToolCallId },
    Completing,
    Cancelling,
    Finished,
    Failed,
}

#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("max turns ({0}) exceeded")] MaxTurnsExceeded(u32),
    #[error("max tool calls ({0}) exceeded")] MaxToolCallsExceeded(u32),
    #[error("context size exceeds model limit")] ContextLimit,
    #[error("session append failed: {0}")] SessionWrite(String),
    #[error("provider error: {0}")] Provider(String),
    #[error("tool error: {0}")] Tool(String),
    #[error("cancelled")] Cancelled,
}
```

**Loop semantics (verbatim from spec §11):**

```
append and emit user message
for turn in 1..=max_turns:
  compile system prompt and model-visible history
  stream provider response while forwarding live deltas
  if cancelled: record interruption and stop
  persist completed assistant message
  collect assistant tool calls in source order
  if no calls: record success and stop
  if total tool calls exceed limit: record limit error and stop
  execute calls sequentially in v0
  persist each completed result
  append results to model-visible history
if max_turns reached: record limit error and stop
```

**Acceptance:**
- 12+ unit tests:
  1. text-only response exits after 1 turn (no tool calls)
  2. single tool call then final response (2 turns)
  3. 3 sequential tool calls (4 turns: 1 with 3 tool calls + 1 final)
  4. tool error → model recovers and continues
  5. max_turns enforcement
  6. max_tool_calls enforcement
  7. cancellation mid-stream: history preserved, RunInterrupted written
  8. session append failure aborts run
  9. RunInterrupted entry written on cancel
  10. provider error: typed error, no auto-retry (per spec §16)
  11. history grows monotonically
  12. submit is sequential (second submit waits for first)
- Gate: clean.

**Forbidden:**
- No `unsafe`.
- No `unwrap`/`expect` in library code.
- No auto-retry on provider errors (spec §16).
- No parallel tool calls in v0 (spec §11: "execute calls sequentially").

**Context size estimation (Decision 05):** we don't implement real compaction in v0. Before each provider call, estimate the context size as `sum(message.serialized_len())` and if it exceeds `model_limit` (we'll use a hardcoded `128_000` for Nemotron 3 Ultra), return `AgentError::ContextLimit`.

**Dependencies:** add `sha2 = "0.10"` if not already in (task 2.5 adds it).
