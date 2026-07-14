### Task 2.6 — Agent state machine

`src/agent.rs`:
```rust
pub struct AgentConfig { pub max_turns: u32, pub max_tool_calls: u32, pub model: String, pub project_root: PathBuf, /* ... */ }
pub struct Agent {
    config: AgentConfig,
    provider: Arc<dyn Provider>,
    tools: Arc<ToolRegistry>,
    session: SessionWriter,
    cancel: CancelScope,
}

impl Agent {
    pub fn new(...) -> Self;
    pub async fn submit(&mut self, user_msg: Message) -> Result<AgentEvent, AgentError>;
    pub async fn cancel(&self);
    pub fn state(&self) -> AgentState;
}
```

Implements the loop from spec §11 verbatim:
- append and emit user message
- for turn in 1..=max_turns: compile context, stream provider, persist assistant, collect tool calls, execute sequentially, persist results, append to history
- max_turns / max_tool_calls enforcement
- provider 429/5xx/disconnect → typed error, no auto-retry
- cancellation preserves completed history

**Spec:** §11, §16.
**Acceptance:** 12+ tests including: text-only response exits after 1 turn, single tool call then final response, 3 sequential tool calls, tool error → model recovery, max_turns enforcement, max_tool_calls enforcement, cancellation mid-stream preserves history, session append failure aborts run, RunInterrupted entry written on cancel.
