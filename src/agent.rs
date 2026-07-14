//! Provider-neutral agent state machine.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tokio_stream::StreamExt;
use tokio_util::sync::CancellationToken;

use crate::event::{AgentEvent, StopReason, Usage, SCHEMA_VERSION};
use crate::ids::{new_id, SessionId, Timestamp, ToolCallId};
use crate::message::{Message, Part, Role};
use crate::provider::{ModelRequest, Provider, ProviderError};
use crate::session::SessionWriter;
use crate::session_entry::SessionEntry;
use crate::tool::{execute_tool_call, ToolCall, ToolContext, ToolOutcome, ToolRegistry};

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

        let mut total_tool_calls = 0_u32;
        for turn in 1..=self.config.max_turns {
            self.ensure_not_cancelled().await?;
            self.state = AgentState::Sampling { turn };
            let compiled =
                crate::context::compile(&self.config.project_root, &self.config.project_root)
                    .map_err(|error| AgentError::Provider(error.to_string()))?;
            let context_len = compiled.system_prompt.len()
                + compiled
                    .instructions
                    .iter()
                    .map(|instruction| instruction.content.len())
                    .sum::<usize>();
            if serialized_len(&self.history).saturating_add(context_len) > 128_000 {
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
                    Some(Ok(AgentEvent::ToolStarted {
                        call_id,
                        name,
                        args,
                    })) => {
                        parts.push(Part::ToolCall {
                            id: call_id,
                            name,
                            args,
                        });
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

            let tool_calls: Vec<ToolCall> = self
                .history
                .last()
                .into_iter()
                .flat_map(|message| message.parts.iter())
                .filter_map(|part| match part {
                    Part::ToolCall { id, name, args } => Some(ToolCall {
                        call_id: *id,
                        name: name.clone(),
                        args: args.clone(),
                    }),
                    _ => None,
                })
                .collect();
            if !tool_calls.is_empty() {
                total_tool_calls = total_tool_calls.saturating_add(tool_calls.len() as u32);
                if total_tool_calls > self.config.max_tool_calls {
                    self.state = AgentState::Failed;
                    return Err(AgentError::MaxToolCallsExceeded(self.config.max_tool_calls));
                }
                for call in tool_calls {
                    self.ensure_not_cancelled().await?;
                    self.state = AgentState::ExecutingTool {
                        turn,
                        call_id: call.call_id,
                    };
                    self.append(SessionEntry::ToolStarted {
                        call_id: call.call_id,
                        name: call.name.clone(),
                        args: call.args.clone(),
                        timestamp: Timestamp::now(),
                    })
                    .await?;
                    let (event_sink, _events) = tokio::sync::mpsc::channel(256);
                    let outcome = execute_tool_call(
                        &self.tools,
                        &call,
                        ToolContext {
                            project_root: self.config.project_root.clone(),
                            max_output_bytes: 1_048_576,
                            command_timeout: Duration::from_secs(30),
                        },
                        event_sink,
                        self.cancel.clone(),
                    )
                    .await;
                    if self.cancel.is_cancelled() {
                        return self.interrupt(Some(call.call_id)).await;
                    }
                    let (event_outcome, output, is_error, truncated) = match outcome {
                        ToolOutcome::Success { output, truncated } => (
                            crate::event::ToolOutcome::Success {
                                output: output.clone(),
                                truncated,
                            },
                            output,
                            false,
                            truncated,
                        ),
                        ToolOutcome::Error {
                            code,
                            message,
                            truncated,
                        } => (
                            crate::event::ToolOutcome::Error {
                                code,
                                message: message.clone(),
                                truncated,
                            },
                            message,
                            true,
                            truncated,
                        ),
                    };
                    self.append(SessionEntry::ToolFinished {
                        call_id: call.call_id,
                        outcome: event_outcome,
                        timestamp: Timestamp::now(),
                    })
                    .await?;
                    self.history.push(Message {
                        id: crate::ids::MessageId(new_id()),
                        role: Role::ToolResult,
                        parts: vec![Part::ToolResult {
                            call_id: call.call_id,
                            output,
                            is_error,
                            truncated,
                            display: None,
                        }],
                    });
                }
                continue;
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
    use crate::provider::ProviderStream;
    use crate::tool::read::ReadTool;
    use async_trait::async_trait;
    use std::collections::VecDeque;
    use std::sync::Mutex;

    struct TurnProvider {
        turns: Mutex<VecDeque<Vec<AgentEvent>>>,
        request_lengths: Arc<Mutex<Vec<usize>>>,
    }

    #[async_trait]
    impl Provider for TurnProvider {
        async fn stream(
            &self,
            req: ModelRequest,
            cancel: CancellationToken,
        ) -> Result<ProviderStream, ProviderError> {
            if cancel.is_cancelled() {
                return Err(ProviderError::Cancelled);
            }
            self.request_lengths
                .lock()
                .map_err(|_| ProviderError::StreamInvalid("length lock poisoned".into()))?
                .push(req.messages.len());
            let events = self
                .turns
                .lock()
                .map_err(|_| ProviderError::StreamInvalid("turn lock poisoned".into()))?
                .pop_front()
                .unwrap_or_default();
            Ok(ProviderStream {
                events: Box::pin(tokio_stream::iter(events.into_iter().map(Ok))),
            })
        }
    }

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
                usage: Usage {
                    input_tokens: 1,
                    output_tokens: 1,
                },
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

        assert_eq!(
            result,
            AgentEvent::RunFinished {
                message: "Hello".into()
            }
        );
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

        assert!(
            matches!(entries.get(1), Some(SessionEntry::UserMessage { content, .. }) if content == "hello")
        );
        assert!(
            matches!(entries.get(2), Some(SessionEntry::AssistantMessage { parts, .. }) if matches!(parts.first(), Some(Part::Text { text }) if text == "Hi"))
        );
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

    async fn tool_agent(root: &std::path::Path, turns: Vec<Vec<AgentEvent>>) -> Agent {
        let writer = SessionWriter::open(root.join("session.jsonl"))
            .await
            .expect("open session");
        let mut tools = ToolRegistry::new();
        tools.register(ReadTool);
        Agent::new(
            AgentConfig {
                max_turns: 2,
                max_tool_calls: 2,
                model: "test".into(),
                project_root: root.to_path_buf(),
                session_writer: writer,
            },
            Arc::new(TurnProvider {
                turns: Mutex::new(turns.into()),
                request_lengths: Arc::new(Mutex::new(Vec::new())),
            }),
            Arc::new(tools),
            CancellationToken::new(),
            Vec::new(),
        )
    }

    fn tool_turn(path: &str) -> Vec<AgentEvent> {
        vec![
            AgentEvent::ToolStarted {
                call_id: ToolCallId(new_id()),
                name: "read".into(),
                args: serde_json::json!({"path": path}),
            },
            AgentEvent::ModelFinished {
                usage: Usage {
                    input_tokens: 1,
                    output_tokens: 1,
                },
                stop_reason: StopReason::ToolUse,
            },
        ]
    }

    #[tokio::test]
    async fn agent_calls_read_tool_then_responds() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("hello.txt"), "hello\n").expect("write file");
        let mut agent = tool_agent(
            dir.path(),
            vec![tool_turn("hello.txt"), text_events("Done")],
        )
        .await;

        let result = agent.submit(user("read it")).await.expect("submit");

        assert_eq!(
            result,
            AgentEvent::RunFinished {
                message: "Done".into()
            }
        );
        assert!(agent
            .history
            .iter()
            .any(|message| message.role == Role::ToolResult));
    }

    #[tokio::test]
    async fn agent_handles_tool_error_gracefully() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut agent = tool_agent(
            dir.path(),
            vec![tool_turn("missing.txt"), text_events("Recovered")],
        )
        .await;

        let result = agent.submit(user("read it")).await.expect("submit");

        assert_eq!(
            result,
            AgentEvent::RunFinished {
                message: "Recovered".into()
            }
        );
        assert!(agent.history.iter().any(|message| {
            matches!(
                message.parts.first(),
                Some(Part::ToolResult { is_error: true, .. })
            )
        }));
    }

    #[tokio::test]
    async fn agent_enforces_max_tool_calls() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut agent = tool_agent(dir.path(), vec![tool_turn("one.txt")]).await;
        agent.config.max_tool_calls = 0;

        let error = agent.submit(user("read")).await.expect_err("tool limit");

        assert!(matches!(error, AgentError::MaxToolCallsExceeded(0)));
    }

    #[tokio::test]
    async fn cancellation_writes_run_interrupted() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("session.jsonl");
        let mut agent = tool_agent(dir.path(), vec![text_events("unused")]).await;
        agent.cancel();

        let error = agent.submit(user("stop")).await.expect_err("cancelled");
        let entries = crate::session::read_entries(path).await.expect("read");

        assert!(matches!(error, AgentError::Cancelled));
        assert!(matches!(
            entries.last(),
            Some(SessionEntry::RunInterrupted { .. })
        ));
    }

    struct ErrorProvider;

    #[async_trait]
    impl Provider for ErrorProvider {
        async fn stream(
            &self,
            _req: ModelRequest,
            _cancel: CancellationToken,
        ) -> Result<ProviderStream, ProviderError> {
            Err(ProviderError::Upstream {
                code: "offline".into(),
                message: "unavailable".into(),
                retryable: true,
            })
        }
    }

    #[tokio::test]
    async fn provider_error_is_typed_and_not_retried() {
        let dir = tempfile::tempdir().expect("tempdir");
        let writer = SessionWriter::open(dir.path().join("session.jsonl"))
            .await
            .expect("open");
        let mut agent = Agent::new(
            AgentConfig {
                max_turns: 3,
                max_tool_calls: 1,
                model: "test".into(),
                project_root: dir.path().to_path_buf(),
                session_writer: writer,
            },
            Arc::new(ErrorProvider),
            Arc::new(ToolRegistry::new()),
            CancellationToken::new(),
            Vec::new(),
        );

        let error = agent.submit(user("hello")).await.expect_err("provider");

        assert!(matches!(error, AgentError::Provider(message) if message.contains("offline")));
    }

    #[tokio::test]
    async fn oversized_history_returns_context_limit() {
        let dir = tempfile::tempdir().expect("tempdir");
        let writer = SessionWriter::open(dir.path().join("session.jsonl"))
            .await
            .expect("open");
        let mut agent = Agent::new(
            AgentConfig {
                max_turns: 1,
                max_tool_calls: 1,
                model: "test".into(),
                project_root: dir.path().to_path_buf(),
                session_writer: writer,
            },
            Arc::new(ScriptedProvider::from_events(text_events("unused"))),
            Arc::new(ToolRegistry::new()),
            CancellationToken::new(),
            vec![user(&"x".repeat(128_000))],
        );

        let error = agent.submit(user("hello")).await.expect_err("context");

        assert!(matches!(error, AgentError::ContextLimit));
    }

    #[tokio::test]
    async fn three_tool_calls_execute_sequentially() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("a.txt"), "a").expect("write");
        let mut calls = Vec::new();
        for _ in 0..3 {
            calls.push(AgentEvent::ToolStarted {
                call_id: ToolCallId(new_id()),
                name: "read".into(),
                args: serde_json::json!({"path": "a.txt"}),
            });
        }
        calls.push(AgentEvent::ModelFinished {
            usage: Usage {
                input_tokens: 1,
                output_tokens: 1,
            },
            stop_reason: StopReason::ToolUse,
        });
        let mut agent = tool_agent(dir.path(), vec![calls, text_events("Done")]).await;
        agent.config.max_tool_calls = 3;

        agent.submit(user("read thrice")).await.expect("submit");

        assert_eq!(
            agent
                .history
                .iter()
                .filter(|message| message.role == Role::ToolResult)
                .count(),
            3
        );
    }

    #[tokio::test]
    async fn history_grows_monotonically_between_turns() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("a.txt"), "a").expect("write");
        let lengths = Arc::new(Mutex::new(Vec::new()));
        let writer = SessionWriter::open(dir.path().join("session.jsonl"))
            .await
            .expect("open");
        let mut tools = ToolRegistry::new();
        tools.register(ReadTool);
        let mut agent = Agent::new(
            AgentConfig {
                max_turns: 2,
                max_tool_calls: 1,
                model: "test".into(),
                project_root: dir.path().to_path_buf(),
                session_writer: writer,
            },
            Arc::new(TurnProvider {
                turns: Mutex::new(vec![tool_turn("a.txt"), text_events("Done")].into()),
                request_lengths: lengths.clone(),
            }),
            Arc::new(tools),
            CancellationToken::new(),
            Vec::new(),
        );

        agent.submit(user("read")).await.expect("submit");

        assert_eq!(*lengths.lock().expect("lengths"), vec![1, 3]);
    }

    #[tokio::test]
    async fn consecutive_submits_append_to_existing_history() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut agent = tool_agent(
            dir.path(),
            vec![text_events("First"), text_events("Second")],
        )
        .await;

        agent.submit(user("one")).await.expect("first");
        agent.submit(user("two")).await.expect("second");

        assert_eq!(agent.history.len(), 4);
    }
}
