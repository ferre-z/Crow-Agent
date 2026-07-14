//! Provider-neutral agent state machine.

use std::path::PathBuf;
use std::sync::Arc;

use tokio_util::sync::CancellationToken;
use tokio_stream::StreamExt;

use crate::event::{AgentEvent, StopReason, Usage, SCHEMA_VERSION};
use crate::ids::{new_id, SessionId, Timestamp, ToolCallId};
use crate::message::{Message, Part, Role};
use crate::provider::{ModelRequest, Provider, ProviderError};
use crate::session::SessionWriter;
use crate::session_entry::SessionEntry;
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
    session_id: SessionId,
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
            session_id: SessionId(new_id()),
        }
    }

    /// Append a user message and run the loop to completion.
    pub async fn submit(&mut self, user_msg: Message) -> Result<AgentEvent, AgentError> {
        self.ensure_not_cancelled().await?;
        if self.config.session_writer.seq() == 0 {
            self.append(SessionEntry::SessionStarted {
                schema_version: SCHEMA_VERSION,
                session_id: self.session_id,
                started_at: Timestamp::now(),
                cwd: self.config.project_root.clone(),
            })
            .await?;
        }
        let content = text_content(&user_msg);
        self.append(SessionEntry::UserMessage {
            id: user_msg.id,
            content,
            timestamp: Timestamp::now(),
        })
        .await?;
        self.history.push(user_msg);

        for turn in 1..=self.config.max_turns {
            self.ensure_not_cancelled().await?;
            self.state = AgentState::Sampling { turn };
            if serialized_len(&self.history) > 128_000 {
                self.state = AgentState::Failed;
                return Err(AgentError::ContextLimit);
            }
            let request = ModelRequest {
                messages: self.history.clone(),
                tools_schema: self.tools.schemas_json(),
            };
            let stream_result = self.provider.stream(request, self.cancel.clone()).await;
            self.ensure_not_cancelled().await?;
            let mut stream = stream_result.map_err(map_provider_error)?.events;
            let mut parts = Vec::new();
            let mut text = String::new();
            let mut finished: Option<(Usage, StopReason)> = None;

            loop {
                let next = tokio::select! {
                    biased;
                    () = self.cancel.cancelled() => return self.interrupt(None).await,
                    item = stream.next() => item,
                };
                self.ensure_not_cancelled().await?;
                match next {
                    Some(Ok(AgentEvent::TextDelta { text: delta })) => text.push_str(&delta),
                    Some(Ok(AgentEvent::ReasoningDelta { text })) => {
                        parts.push(Part::Reasoning { text });
                    }
                    Some(Ok(AgentEvent::ToolStarted { call_id, name, args })) => {
                        parts.push(Part::ToolCall { id: call_id, name, args });
                    }
                    Some(Ok(AgentEvent::ModelFinished { usage, stop_reason })) => {
                        finished = Some((usage, stop_reason));
                        break;
                    }
                    Some(Ok(AgentEvent::RunFailed { message, .. })) => {
                        self.state = AgentState::Failed;
                        return Err(AgentError::Provider(message));
                    }
                    Some(Ok(AgentEvent::RunCancelled)) => return self.interrupt(None).await,
                    Some(Err(error)) => return Err(map_provider_error(error)),
                    Some(Ok(_)) => {}
                    None => break,
                }
            }

            let Some((usage, stop_reason)) = finished else {
                continue;
            };
            if !text.is_empty() {
                parts.insert(0, Part::Text { text: text.clone() });
            }
            let assistant = Message {
                id: crate::ids::MessageId(new_id()),
                role: Role::Assistant,
                parts: parts.clone(),
            };
            self.append(SessionEntry::AssistantMessage {
                id: assistant.id,
                parts,
                usage: Some(usage),
                stop_reason: Some(stop_reason.clone()),
                timestamp: Timestamp::now(),
            })
            .await?;
            self.history.push(assistant);

            if matches!(stop_reason, StopReason::ToolUse) {
                self.state = AgentState::Failed;
                return Err(AgentError::Tool("tool calls are not implemented".into()));
            }
            self.state = AgentState::Completing;
            self.append(SessionEntry::RunFinished {
                message: text.clone(),
                timestamp: Timestamp::now(),
            })
            .await?;
            self.state = AgentState::Finished;
            return Ok(AgentEvent::RunFinished { message: text });
        }
        self.state = AgentState::Failed;
        Err(AgentError::MaxTurnsExceeded(self.config.max_turns))
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

    async fn append(&mut self, entry: SessionEntry) -> Result<(), AgentError> {
        self.ensure_not_cancelled_now()?;
        let result = self.config.session_writer.append(entry).await;
        self.ensure_not_cancelled_now()?;
        result.map_err(|error| AgentError::SessionWrite(error.to_string()))
    }

    async fn ensure_not_cancelled(&mut self) -> Result<(), AgentError> {
        if self.cancel.is_cancelled() {
            return self.interrupt(None).await;
        }
        Ok(())
    }

    fn ensure_not_cancelled_now(&self) -> Result<(), AgentError> {
        if self.cancel.is_cancelled() {
            Err(AgentError::Cancelled)
        } else {
            Ok(())
        }
    }

    async fn interrupt<T>(&mut self, active_call: Option<ToolCallId>) -> Result<T, AgentError> {
        self.state = AgentState::Cancelling;
        let result = self
            .config
            .session_writer
            .append(SessionEntry::RunInterrupted {
                active_call,
                timestamp: Timestamp::now(),
            })
            .await;
        if let Err(error) = result {
            self.state = AgentState::Failed;
            return Err(AgentError::SessionWrite(error.to_string()));
        }
        self.state = AgentState::Finished;
        Err(AgentError::Cancelled)
    }
}

fn text_content(message: &Message) -> String {
    message
        .parts
        .iter()
        .filter_map(|part| match part {
            Part::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

fn serialized_len(messages: &[Message]) -> usize {
    messages
        .iter()
        .filter_map(|message| serde_json::to_vec(message).ok())
        .map(|bytes| bytes.len())
        .sum()
}

fn map_provider_error(error: ProviderError) -> AgentError {
    match error {
        ProviderError::Cancelled => AgentError::Cancelled,
        other => AgentError::Provider(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{StopReason, Usage};
    use crate::ids::{new_id, MessageId};
    use crate::message::{Part, Role};
    use crate::provider::mock::ScriptedProvider;

    fn user(text: &str) -> Message {
        Message {
            id: MessageId(new_id()),
            role: Role::User,
            parts: vec![Part::Text { text: text.into() }],
        }
    }

    fn text_events(text: &str) -> Vec<AgentEvent> {
        vec![
            AgentEvent::TextDelta { text: text.into() },
            AgentEvent::ModelFinished {
                usage: Usage { input_tokens: 1, output_tokens: 1 },
                stop_reason: StopReason::EndTurn,
            },
        ]
    }

    #[tokio::test]
    async fn agent_runs_one_turn_no_tools() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("session.jsonl");
        let writer = SessionWriter::open(&path).await.expect("open session");
        let provider = ScriptedProvider::from_events(text_events("Hello"));
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
        let result = agent.submit(user("hello")).await.expect("submit");

        assert_eq!(result, AgentEvent::RunFinished { message: "Hello".into() });
        assert_eq!(agent.state(), AgentState::Finished);
    }

    #[tokio::test]
    async fn agent_writes_session_entries() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("session.jsonl");
        let writer = SessionWriter::open(&path).await.expect("open session");
        let config = AgentConfig {
            max_turns: 1,
            max_tool_calls: 1,
            model: "test".into(),
            project_root: dir.path().to_path_buf(),
            session_writer: writer,
        };
        let mut agent = Agent::new(
            config,
            Arc::new(ScriptedProvider::from_events(text_events("Hi"))),
            Arc::new(ToolRegistry::new()),
            CancellationToken::new(),
            Vec::new(),
        );

        agent.submit(user("hello")).await.expect("submit");
        let entries = crate::session::read_entries(&path).await.expect("read");

        assert!(matches!(entries.get(1), Some(SessionEntry::UserMessage { content, .. }) if content == "hello"));
        assert!(matches!(entries.get(2), Some(SessionEntry::AssistantMessage { parts, .. }) if matches!(parts.first(), Some(Part::Text { text }) if text == "Hi")));
    }

    #[tokio::test]
    async fn agent_returns_max_turns_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let writer = SessionWriter::open(dir.path().join("session.jsonl"))
            .await
            .expect("open session");
        let config = AgentConfig {
            max_turns: 1,
            max_tool_calls: 1,
            model: "test".into(),
            project_root: dir.path().to_path_buf(),
            session_writer: writer,
        };
        let mut agent = Agent::new(
            config,
            Arc::new(ScriptedProvider::from_events(Vec::new())),
            Arc::new(ToolRegistry::new()),
            CancellationToken::new(),
            Vec::new(),
        );

        let error = agent.submit(user("hello")).await.expect_err("max turns");

        assert!(matches!(error, AgentError::MaxTurnsExceeded(1)));
    }
}
