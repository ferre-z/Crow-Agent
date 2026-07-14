//! Provider-neutral agent state machine.

use std::path::PathBuf;
use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use crate::event::AgentEvent;
use crate::ids::ToolCallId;
use crate::message::Message;
use crate::provider::Provider;
use crate::session::SessionWriter;
use crate::tool::ToolRegistry;

/// Limits and durable resources used by an [`Agent`].
#[derive(Debug)]
pub struct AgentConfig {
    pub max_turns: u32,
    pub max_tool_calls: u32,
    pub model: String,
    pub project_root: PathBuf,
    pub session_writer: SessionWriter,
}

/// Current phase of the agent state machine.
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

/// A terminal failure from the agent loop.
#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("max turns ({0}) exceeded")]
    MaxTurnsExceeded(u32),
    #[error("max tool calls ({0}) exceeded")]
    MaxToolCallsExceeded(u32),
    #[error("context size exceeds model limit")]
    ContextLimit,
    #[error("session append failed: {0}")]
    SessionWrite(String),
    #[error("provider error: {0}")]
    Provider(String),
    #[error("tool error: {0}")]
    Tool(String),
    #[error("cancelled")]
    Cancelled,
}

/// Owns conversation history and drives model/tool turns.
#[allow(missing_debug_implementations)]
pub struct Agent {
    config: AgentConfig,
    provider: Arc<dyn Provider>,
    tools: Arc<ToolRegistry>,
    cancel: CancellationToken,
    history: Vec<Message>,
    state: AgentState,
}

impl Agent {
    #[must_use]
    pub fn new(
        config: AgentConfig,
        provider: Arc<dyn Provider>,
        tools: Arc<ToolRegistry>,
        cancel: CancellationToken,
        initial_history: Vec<Message>,
    ) -> Self {
        Self {
            config,
            provider,
            tools,
            cancel,
            history: initial_history,
            state: AgentState::Idle,
        }
    }

    /// Append a user message and run the loop to completion.
    pub async fn submit(&mut self, _user_msg: Message) -> Result<AgentEvent, AgentError> {
        Err(AgentError::Provider(
            "agent loop is not implemented".to_string(),
        ))
    }

    /// Cooperatively cancel the in-flight run.
    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    /// Return the current state.
    #[must_use]
    pub fn state(&self) -> AgentState {
        self.state.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{StopReason, Usage};
    use crate::ids::{new_id, MessageId};
    use crate::message::{Part, Role};
    use crate::provider::mock::ScriptedProvider;

    #[tokio::test]
    #[ignore = "wip"]
    async fn agent_writes_at_least_one_session_entry() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("session.jsonl");
        let writer = SessionWriter::open(&path).await.expect("open session");
        let provider = ScriptedProvider::from_events(vec![AgentEvent::ModelFinished {
            usage: Usage {
                input_tokens: 1,
                output_tokens: 1,
            },
            stop_reason: StopReason::EndTurn,
        }]);
        let config = AgentConfig {
            max_turns: 1,
            max_tool_calls: 1,
            model: "test".into(),
            project_root: dir.path().to_path_buf(),
            session_writer: writer,
        };
        let mut agent = Agent::new(
            config,
            Arc::new(provider),
            Arc::new(ToolRegistry::new()),
            CancellationToken::new(),
            Vec::new(),
        );
        let message = Message {
            id: MessageId(new_id()),
            role: Role::User,
            parts: vec![Part::Text {
                text: "hello".into(),
            }],
        };

        let _ = agent.submit(message).await;

        assert!(agent.config.session_writer.seq() > 0);
    }
}
