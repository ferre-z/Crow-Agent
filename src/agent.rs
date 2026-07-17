//! Provider-neutral agent state machine.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tokio_stream::StreamExt;
use tokio_util::sync::CancellationToken;

use crate::event::{AgentEvent, AgentEventSink, ErrorCode, StopReason, Usage, SCHEMA_VERSION};
use crate::ids::{new_id, RunId, SessionId, Timestamp, ToolCallId};
use crate::message::{Message, Part, Role};
use crate::policy::{AskResolver, Decision};
use crate::provider::{ModelRequest, Provider, ProviderError};
use crate::session::{read_entries_with_recovery, SessionWriter};
use crate::session_entry::SessionEntry;
use crate::tool::{execute_tool_call, ToolCall, ToolContext, ToolOutcome, ToolRegistry};

/// Limits and durable resources used by an [`Agent`].
pub struct AgentConfig {
    pub max_turns: u32,
    pub max_tool_calls: u32,
    pub model: String,
    pub project_root: PathBuf,
    pub session_writer: SessionWriter,
    /// Approval policy consulted before each tool call. Defaults to
    /// [`crate::policy::DefaultPolicy`] when not set; callers that
    /// want to set this build the config via [`AgentConfig::with_policy`].
    pub policy: Option<std::sync::Arc<dyn crate::policy::ApprovalPolicy>>,
    /// Channel for resolving pending Ask decisions. None means the
    /// agent loop will surface a typed error instead of blocking.
    pub ask_resolver: Option<AskResolver>,
}

impl std::fmt::Debug for AgentConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentConfig")
            .field("max_turns", &self.max_turns)
            .field("max_tool_calls", &self.max_tool_calls)
            .field("model", &self.model)
            .field("project_root", &self.project_root)
            .field("session_writer", &"<SessionWriter>")
            .field("policy", &"<dyn ApprovalPolicy>")
            .field(
                "ask_resolver",
                &self.ask_resolver.as_ref().map(|_| "<AskResolver>"),
            )
            .finish()
    }
}

impl AgentConfig {
    /// Construct a config with no policy or ask resolver (defaults
    /// will be applied at Agent construction time).
    pub fn new(
        max_turns: u32,
        max_tool_calls: u32,
        model: String,
        project_root: PathBuf,
        session_writer: SessionWriter,
    ) -> Self {
        Self {
            max_turns,
            max_tool_calls,
            model,
            project_root,
            session_writer,
            policy: None,
            ask_resolver: None,
        }
    }

    /// Attach an approval policy.
    #[must_use]
    pub fn with_policy(
        mut self,
        policy: std::sync::Arc<dyn crate::policy::ApprovalPolicy>,
    ) -> Self {
        self.policy = Some(policy);
        self
    }

    /// Attach an ask resolver.
    #[must_use]
    pub fn with_ask_resolver(mut self, resolver: AskResolver) -> Self {
        self.ask_resolver = Some(resolver);
        self
    }
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
    policy: Arc<dyn crate::policy::ApprovalPolicy>,
    cancel: CancellationToken,
    history: Vec<Message>,
    state: AgentState,
    session_id: SessionId,
    run_id: RunId,
    /// Live event sink. Receives every [`AgentEvent`] the loop
    /// observes; consumers (CLI, TUI, app-server, tests) plug in their
    /// own implementation.
    sink: Arc<dyn AgentEventSink>,
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
        Self::with_sink(
            config,
            provider,
            tools,
            cancel,
            initial_history,
            Arc::new(crate::event::CollectingSink::new()),
        )
    }

    /// Build an agent that forwards every observed event to `sink`.
    /// Use this from the CLI / TUI / app-server; tests typically use
    /// the default no-op sink via [`Agent::new`].
    #[must_use]
    pub fn with_sink(
        config: AgentConfig,
        provider: Arc<dyn Provider>,
        tools: Arc<ToolRegistry>,
        cancel: CancellationToken,
        initial_history: Vec<Message>,
        sink: Arc<dyn AgentEventSink>,
    ) -> Self {
        let policy = config
            .policy
            .clone()
            .unwrap_or_else(|| Arc::new(crate::policy::DefaultPolicy));
        Self {
            config,
            provider,
            tools,
            policy,
            cancel,
            history: initial_history,
            state: AgentState::Idle,
            session_id: SessionId(new_id()),
            run_id: RunId(new_id()),
            sink,
        }
    }

    /// Resume an existing session by rebuilding history from the
    /// JSONL log at `path`. Returns the new agent and the recovered
    /// history.
    ///
    /// The reconstructed history contains:
    /// - one `User` message per `UserMessage` entry (text only);
    /// - one `Assistant` message per `AssistantMessage` entry
    ///   (text + tool calls + reasoning, with original ids);
    /// - one `ToolResult` message per `ToolFinished` entry whose
    ///   matching `ToolStarted` was not later orphaned by an
    ///   interruption.
    ///
    /// A `RunInterrupted` entry is treated as an "active call" marker;
    /// the corresponding `ToolStarted` (if present) is skipped so the
    /// resumed run does not re-execute it.
    #[allow(clippy::missing_errors_doc)]
    pub async fn resume_into(
        config: AgentConfig,
        provider: Arc<dyn Provider>,
        tools: Arc<ToolRegistry>,
        cancel: CancellationToken,
        sink: Arc<dyn AgentEventSink>,
        path: &Path,
    ) -> Result<(Self, Vec<Message>), AgentError> {
        let report = read_entries_with_recovery(path)
            .await
            .map_err(|e| AgentError::SessionWrite(e.to_string()))?;
        let history = history_from_entries(&report.entries);
        let mut agent = Self::with_sink(config, provider, tools, cancel, history.clone(), sink);
        // Recover the session_id from the first SessionStarted entry
        // so the resumed run appends to the same logical session.
        if let Some(SessionEntry::SessionStarted { session_id, .. }) = report.entries.first() {
            agent.session_id = *session_id;
        }
        Ok((agent, history))
    }

    /// The session id this agent appends to. After [`Agent::resume_into`]
    /// this is the persisted session's id, not a fresh one.
    #[must_use]
    pub fn session_id(&self) -> SessionId {
        self.session_id
    }

    /// The run id for the current/next `submit`. Fresh per agent
    /// instance. Callers correlate streamed events by this value.
    #[must_use]
    pub fn run_id(&self) -> RunId {
        self.run_id
    }

    /// Emit a terminal `RunFailed` to the live event sink without
    /// moving the values (the caller has usually already moved them
    /// into a `SessionEntry::RunFailed` write). Used at every
    /// failing exit that previously left the stream silent.
    fn emit_run_failed(&self, code: ErrorCode, retryable: bool, message: &str) {
        self.sink.on_event(AgentEvent::RunFailed {
            code,
            retryable,
            message: message.to_string(),
        });
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
        let started_at = Timestamp::now();
        self.sink.on_event(AgentEvent::RunStarted {
            run_id: self.run_id,
            session_id: self.session_id,
            started_at,
        });
        let content = text_content(&user_msg);
        self.append(SessionEntry::UserMessage {
            id: user_msg.id,
            content,
            timestamp: Timestamp::now(),
        })
        .await?;
        self.history.push(user_msg);

        let terminal = self.run_loop().await;
        // Always seal the writer on a terminal outcome so consumers
        // can tell the log is complete.
        let _ = self.config.session_writer.finish().await;
        terminal
    }

    async fn run_loop(&mut self) -> Result<AgentEvent, AgentError> {
        let mut total_tool_calls = 0_u32;
        for turn in 1..=self.config.max_turns {
            self.ensure_not_cancelled().await?;
            self.state = AgentState::Sampling { turn };
            let compiled =
                crate::context::compile(&self.config.project_root, &self.config.project_root)
                    .map_err(|error| AgentError::Provider(error.to_string()))?;
            let system = compiled.to_request_text(&self.config.project_root);
            let context_len = system.len();
            if serialized_len(&self.history).saturating_add(context_len) > 128_000 {
                self.state = AgentState::Failed;
                let failure = AgentError::ContextLimit;
                let message = failure.to_string();
                self.emit_run_failed(ErrorCode("context_limit".into()), false, &message);
                self.append(SessionEntry::RunFailed {
                    code: ErrorCode("context_limit".into()),
                    retryable: false,
                    message,
                    timestamp: Timestamp::now(),
                })
                .await?;
                return Err(failure);
            }
            let request = ModelRequest {
                messages: self.history.clone(),
                tools_schema: serde_json::to_value(self.tools.tool_specs())
                    .unwrap_or(serde_json::Value::Null),
                system,
            };
            let stream_result = self.provider.stream(request, self.cancel.clone()).await;
            self.ensure_not_cancelled().await?;
            let mut stream = match stream_result {
                Ok(s) => s.events,
                Err(error) => {
                    let mapped = map_provider_error(error.clone());
                    let (code, retryable) = match &error {
                        ProviderError::Upstream { retryable, .. } => {
                            (ErrorCode("provider_upstream".into()), *retryable)
                        }
                        ProviderError::StreamInvalid(_) => {
                            (ErrorCode("stream_invalid".into()), true)
                        }
                        ProviderError::Cancelled => (ErrorCode("cancelled".into()), false),
                    };
                    self.append(SessionEntry::RunFailed {
                        code,
                        retryable,
                        message: error.to_string(),
                        timestamp: Timestamp::now(),
                    })
                    .await?;
                    self.state = AgentState::Failed;
                    self.sink.on_event(AgentEvent::RunFailed {
                        code: ErrorCode("provider_error".into()),
                        retryable,
                        message: error.to_string(),
                    });
                    return Err(mapped);
                }
            };
            let mut parts = Vec::new();
            let mut text = String::new();
            let mut finished: Option<(Usage, StopReason)> = None;
            let mut last_event_seen = false;

            loop {
                let next = tokio::select! {
                    biased;
                    () = self.cancel.cancelled() => return self.interrupt(None).await,
                    item = stream.next() => item,
                };
                self.ensure_not_cancelled().await?;
                match next {
                    Some(Ok(AgentEvent::ModelStarted)) => {
                        self.sink.on_event(AgentEvent::ModelStarted);
                        last_event_seen = true;
                    }
                    Some(Ok(AgentEvent::TextDelta { text: delta })) => {
                        text.push_str(&delta);
                        self.sink.on_event(AgentEvent::TextDelta { text: delta });
                        last_event_seen = true;
                    }
                    Some(Ok(AgentEvent::ReasoningDelta { text })) => {
                        parts.push(Part::Reasoning { text: text.clone() });
                        self.sink.on_event(AgentEvent::ReasoningDelta { text });
                        last_event_seen = true;
                    }
                    Some(Ok(AgentEvent::ToolStarted {
                        call_id,
                        name,
                        args,
                    })) => {
                        parts.push(Part::ToolCall {
                            id: call_id,
                            name: name.clone(),
                            args: args.clone(),
                        });
                        self.sink.on_event(AgentEvent::ToolStarted {
                            call_id,
                            name,
                            args,
                        });
                        last_event_seen = true;
                    }
                    Some(Ok(AgentEvent::ToolOutput { .. })) => {
                        // Pass-through: streaming tool output is
                        // meaningful to live consumers but we do not
                        // persist it as a Part (the eventual
                        // ToolFinished captures the result body).
                        // We forward to the sink but don't touch
                        // history.
                        last_event_seen = true;
                    }
                    Some(Ok(AgentEvent::ToolFinished { .. })) => {
                        // The tool wrapper emits this; persistence
                        // happens after we observe the outcome
                        // ourselves in the tool execution branch
                        // below. Forward to the sink so live UIs see
                        // the terminal event.
                        last_event_seen = true;
                    }
                    Some(Ok(AgentEvent::ModelFinished { usage, stop_reason })) => {
                        self.sink.on_event(AgentEvent::ModelFinished {
                            usage,
                            stop_reason: stop_reason.clone(),
                        });
                        finished = Some((usage, stop_reason));
                        break;
                    }
                    Some(Ok(AgentEvent::RunFinished { .. })) => {
                        // Provider shouldn't emit this mid-stream; if
                        // it does, treat as a clean stop.
                        last_event_seen = true;
                    }
                    Some(Ok(AgentEvent::RunCancelled)) => {
                        return self.interrupt(None).await;
                    }
                    Some(Ok(AgentEvent::RunStarted { .. })) => {
                        // Ignore a stray RunStarted from the provider
                        // stream (we already emitted our own above).
                        last_event_seen = true;
                    }
                    Some(Ok(AgentEvent::RunFailed {
                        code,
                        retryable,
                        message,
                    })) => {
                        self.append(SessionEntry::RunFailed {
                            code: code.clone(),
                            retryable,
                            message: message.clone(),
                            timestamp: Timestamp::now(),
                        })
                        .await?;
                        self.state = AgentState::Failed;
                        return Err(AgentError::Provider(message));
                    }
                    Some(Err(error)) => {
                        let mapped = map_provider_error(error.clone());
                        let message = error.to_string();
                        self.emit_run_failed(ErrorCode("stream_error".into()), true, &message);
                        self.append(SessionEntry::RunFailed {
                            code: ErrorCode("stream_error".into()),
                            retryable: true,
                            message,
                            timestamp: Timestamp::now(),
                        })
                        .await?;
                        self.state = AgentState::Failed;
                        return Err(mapped);
                    }
                    None => {
                        // Stream ended without a terminal event. If
                        // we observed any event at all and a tool
                        // call was in flight, treat as success; if
                        // the stream was completely empty, surface
                        // as a typed failure so the caller doesn't
                        // silently spin on the next turn.
                        if !last_event_seen && text.is_empty() && parts.is_empty() {
                            self.emit_run_failed(
                                ErrorCode("empty_stream".into()),
                                true,
                                "provider stream ended with no events",
                            );
                            self.append(SessionEntry::RunFailed {
                                code: ErrorCode("empty_stream".into()),
                                retryable: true,
                                message: "provider stream ended with no events".into(),
                                timestamp: Timestamp::now(),
                            })
                            .await?;
                            self.state = AgentState::Failed;
                            return Err(AgentError::Provider(
                                "provider stream ended with no events".into(),
                            ));
                        }
                        break;
                    }
                }
            }

            let Some((usage, stop_reason)) = finished else {
                // No terminal — fall through to next turn.
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

            // Branch on stop reason per phase-1 contract.
            match stop_reason {
                StopReason::Error => {
                    let failure =
                        AgentError::Provider("provider returned stop_reason=Error".into());
                    self.append(SessionEntry::RunFailed {
                        code: ErrorCode("provider_error".into()),
                        retryable: false,
                        message: failure.to_string(),
                        timestamp: Timestamp::now(),
                    })
                    .await?;
                    self.state = AgentState::Failed;
                    self.sink.on_event(AgentEvent::RunFailed {
                        code: ErrorCode("provider_error".into()),
                        retryable: false,
                        message: failure.to_string(),
                    });
                    return Err(failure);
                }
                StopReason::MaxTokens => {
                    let failure =
                        AgentError::Provider("provider returned stop_reason=MaxTokens".into());
                    self.append(SessionEntry::RunFailed {
                        code: ErrorCode("max_tokens".into()),
                        retryable: true,
                        message: failure.to_string(),
                        timestamp: Timestamp::now(),
                    })
                    .await?;
                    self.state = AgentState::Failed;
                    self.sink.on_event(AgentEvent::RunFailed {
                        code: ErrorCode("max_tokens".into()),
                        retryable: true,
                        message: failure.to_string(),
                    });
                    return Err(failure);
                }
                StopReason::Cancellation => {
                    return self.interrupt(None).await;
                }
                StopReason::EndTurn | StopReason::ToolUse => {
                    // Continue below.
                }
            }

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
                    let failure = AgentError::MaxToolCallsExceeded(self.config.max_tool_calls);
                    let message = failure.to_string();
                    self.emit_run_failed(ErrorCode("max_tool_calls".into()), false, &message);
                    self.append(SessionEntry::RunFailed {
                        code: ErrorCode("max_tool_calls".into()),
                        retryable: false,
                        message,
                        timestamp: Timestamp::now(),
                    })
                    .await?;
                    self.state = AgentState::Failed;
                    return Err(failure);
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

                    // Policy gate. We record the started entry above
                    // so a denied/asked call is still visible in the
                    // log (paired with its finished entry below).
                    let decision = self.policy.decide(&call, &self.history).await;
                    let outcome = match decision {
                        Decision::Allow => None,
                        Decision::Deny { reason } => Some(ToolOutcome::Error {
                            code: ErrorCode("policy_denied".into()),
                            message: reason,
                            truncated: false,
                        }),
                        Decision::Ask { ask_id } => {
                            // Block until the resolver fires. If no
                            // resolver is wired up, surface as a
                            // typed error rather than deadlock.
                            let resolver = match self.config.ask_resolver.clone() {
                                Some(r) => r,
                                None => {
                                    return Err(AgentError::Provider(format!(
                                        "policy requested Ask for {ask_id} but no resolver is configured"
                                    )));
                                }
                            };
                            let (tx, rx) = tokio::sync::oneshot::channel();
                            let request = crate::policy::AskRequest {
                                ask_id: ask_id.clone(),
                                call: call.clone(),
                                response: tx,
                            };
                            // Forward the request to the policy layer.
                            if resolver.send(request).await.is_err() {
                                Some(ToolOutcome::Error {
                                    code: ErrorCode("policy_ask_closed".into()),
                                    message: "ask resolver dropped before responding".into(),
                                    truncated: false,
                                })
                            } else {
                                // Wait for the oneshot with cancellation.
                                let response = tokio::select! {
                                    () = self.cancel.cancelled() => {
                                        return self.interrupt(Some(call.call_id)).await;
                                    }
                                    resp = rx => resp,
                                };
                                match response {
                                    Ok(crate::policy::AskResponse::Allow) => None,
                                    Ok(crate::policy::AskResponse::Deny) => {
                                        Some(ToolOutcome::Error {
                                            code: ErrorCode("policy_denied".into()),
                                            message: "denied by user".into(),
                                            truncated: false,
                                        })
                                    }
                                    Err(_) => Some(ToolOutcome::Error {
                                        code: ErrorCode("policy_ask_closed".into()),
                                        message: "ask responder dropped without answering".into(),
                                        truncated: false,
                                    }),
                                }
                            }
                        }
                    };

                    // Persist the synthetic outcome (deny/no-resolver)
                    // BEFORE the tool runs, so the session log stays
                    // consistent.
                    if let Some(synthetic) = &outcome {
                        let event_outcome = match synthetic {
                            ToolOutcome::Success { output, truncated } => {
                                crate::event::ToolOutcome::Success {
                                    output: output.clone(),
                                    truncated: *truncated,
                                }
                            }
                            ToolOutcome::Error {
                                code,
                                message,
                                truncated,
                            } => crate::event::ToolOutcome::Error {
                                code: code.clone(),
                                message: message.clone(),
                                truncated: *truncated,
                            },
                        };
                        self.append(SessionEntry::ToolFinished {
                            call_id: call.call_id,
                            outcome: event_outcome.clone(),
                            timestamp: Timestamp::now(),
                        })
                        .await?;
                        self.sink.on_event(AgentEvent::ToolFinished {
                            call_id: call.call_id,
                            result: event_outcome,
                        });
                        let (output, is_error, truncated) = match synthetic {
                            ToolOutcome::Success { output, truncated } => {
                                (output.clone(), false, *truncated)
                            }
                            ToolOutcome::Error {
                                message, truncated, ..
                            } => (message.clone(), true, *truncated),
                        };
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
                        continue;
                    }

                    let (sink_tx, sink_rx) = tokio::sync::mpsc::channel(256);
                    // Forward tool events to the agent sink so live
                    // consumers (CLI / TUI) see tool output chunks
                    // and the terminal ToolFinished event in order.
                    let agent_sink = Arc::clone(&self.sink);
                    tokio::spawn(async move {
                        let mut rx = sink_rx;
                        while let Some(event) = rx.recv().await {
                            agent_sink.on_event(event);
                        }
                    });
                    let outcome = execute_tool_call(
                        &self.tools,
                        &call,
                        ToolContext {
                            call_id: call.call_id,
                            project_root: self.config.project_root.clone(),
                            max_output_bytes: 1_048_576,
                            command_timeout: Duration::from_secs(30),
                        },
                        sink_tx,
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
                        outcome: event_outcome.clone(),
                        timestamp: Timestamp::now(),
                    })
                    .await?;
                    self.sink.on_event(AgentEvent::ToolFinished {
                        call_id: call.call_id,
                        result: event_outcome,
                    });
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
            let event = AgentEvent::RunFinished { message: text };
            self.sink.on_event(event.clone());
            return Ok(event);
        }
        let failure = AgentError::MaxTurnsExceeded(self.config.max_turns);
        let message = failure.to_string();
        self.emit_run_failed(ErrorCode("max_turns".into()), false, &message);
        self.append(SessionEntry::RunFailed {
            code: ErrorCode("max_turns".into()),
            retryable: false,
            message,
            timestamp: Timestamp::now(),
        })
        .await?;
        self.state = AgentState::Failed;
        Err(failure)
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
        self.sink.on_event(AgentEvent::RunCancelled);
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

/// Reconstruct model-visible [`Message`] history from a sequence of
/// durable [`SessionEntry`]s. Rules:
///
/// - `SessionStarted` is consumed as session metadata; not a message.
/// - `UserMessage` → `Role::User` message with a single Text part.
/// - `AssistantMessage` → `Role::Assistant` message with the recorded
///   parts verbatim.
/// - `ToolStarted` is buffered; it surfaces as a `ToolCall` part on
///   the next `AssistantMessage`. If a `RunInterrupted` follows before
///   the matching `ToolFinished`, that buffered call is dropped (the
///   resumed run starts a fresh turn).
/// - `ToolFinished` closes a buffered call by emitting a
///   `Role::ToolResult` message with a `Part::ToolResult`.
/// - `RunFinished` / `RunInterrupted` / `RunFailed` are terminal
///   markers and do not produce additional messages.
///
/// The reconstructed history is what the agent loop sees on its next
/// turn; the model treats it as if the previous turns had just
/// happened.
fn history_from_entries(entries: &[SessionEntry]) -> Vec<Message> {
    let mut out: Vec<Message> = Vec::new();
    let mut pending_tool_call_ids: Vec<ToolCallId> = Vec::new();
    for entry in entries {
        match entry {
            SessionEntry::SessionStarted { .. } => {}
            SessionEntry::UserMessage { id, content, .. } => {
                out.push(Message {
                    id: *id,
                    role: Role::User,
                    parts: vec![Part::Text {
                        text: content.clone(),
                    }],
                });
            }
            SessionEntry::AssistantMessage {
                id,
                parts,
                stop_reason: _,
                ..
            } => {
                out.push(Message {
                    id: *id,
                    role: Role::Assistant,
                    parts: parts.clone(),
                });
            }
            SessionEntry::ToolStarted { call_id, .. } => {
                pending_tool_call_ids.push(*call_id);
            }
            SessionEntry::ToolFinished {
                call_id, outcome, ..
            } => {
                if let Some(pos) = pending_tool_call_ids.iter().position(|c| c == call_id) {
                    pending_tool_call_ids.remove(pos);
                }
                let (output, is_error, truncated) = match outcome {
                    crate::event::ToolOutcome::Success { output, truncated } => {
                        (output.clone(), false, *truncated)
                    }
                    crate::event::ToolOutcome::Error {
                        message, truncated, ..
                    } => (message.clone(), true, *truncated),
                };
                out.push(Message {
                    id: crate::ids::MessageId(new_id()),
                    role: Role::ToolResult,
                    parts: vec![Part::ToolResult {
                        call_id: *call_id,
                        output,
                        is_error,
                        truncated,
                        display: None,
                    }],
                });
            }
            SessionEntry::RunInterrupted { active_call, .. } => {
                if let Some(call) = active_call {
                    if let Some(pos) = pending_tool_call_ids.iter().position(|c| c == call) {
                        pending_tool_call_ids.remove(pos);
                    }
                }
            }
            SessionEntry::RunFinished { .. } | SessionEntry::RunFailed { .. } => {}
        }
    }
    // Any tool calls still pending at EOF were never finished; drop
    // them so the resumed turn does not synthesize a stray ToolResult
    // for a call that did not actually complete.
    let _ = pending_tool_call_ids;
    out
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
        let config = AgentConfig::new(1, 1, "test".into(), dir.path().to_path_buf(), writer);
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
        let config = AgentConfig::new(1, 1, "test".into(), dir.path().to_path_buf(), writer);
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
        let config = AgentConfig::new(1, 1, "test".into(), dir.path().to_path_buf(), writer);
        let mut agent = Agent::new(
            config,
            Arc::new(ScriptedProvider::from_events(Vec::new())),
            Arc::new(ToolRegistry::new()),
            CancellationToken::new(),
            Vec::new(),
        );

        let error = agent.submit(user("hello")).await.expect_err("empty stream");

        // Phase-1 contract: a stream that yields no events at all is
        // a typed failure, not a silent loop that consumes all turns.
        match &error {
            AgentError::Provider(message) if message.contains("no events") => {}
            other => panic!("expected Provider error mentioning 'no events', got {other:?}"),
        }
    }

    #[tokio::test]
    async fn agent_enforces_max_turns_with_real_output() {
        // The empty-stream case is a typed failure (see above). Real
        // max-turns enforcement requires a provider that yields text
        // but never signals EndTurn, so the loop must consume its
        // turn budget.
        let dir = tempfile::tempdir().expect("tempdir");
        let writer = SessionWriter::open(dir.path().join("session.jsonl"))
            .await
            .expect("open");
        let provider = ScriptedProvider::from_events(vec![
            AgentEvent::TextDelta { text: "x".into() },
            // No ModelFinished — loop must hit its turn cap.
        ]);
        let mut agent = Agent::new(
            AgentConfig::new(1, 0, "test".into(), dir.path().to_path_buf(), writer),
            Arc::new(provider),
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
            AgentConfig::new(2, 2, "test".into(), root.to_path_buf(), writer),
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
            AgentConfig::new(3, 1, "test".into(), dir.path().to_path_buf(), writer),
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
            AgentConfig::new(1, 1, "test".into(), dir.path().to_path_buf(), writer),
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
            AgentConfig::new(2, 1, "test".into(), dir.path().to_path_buf(), writer),
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

    #[tokio::test]
    async fn resume_into_reconstructs_history_from_log() {
        // Drive an agent to write a session, then resume into a new
        // agent from the same log file and check that history is
        // reconstructed and the session_id is preserved.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("session.jsonl");
        let original_id = {
            let writer = SessionWriter::open(&path).await.expect("open");
            let config = AgentConfig::new(1, 1, "test".into(), dir.path().to_path_buf(), writer);
            let mut a = Agent::new(
                config,
                Arc::new(ScriptedProvider::from_events(text_events("Hi"))),
                Arc::new(ToolRegistry::new()),
                CancellationToken::new(),
                Vec::new(),
            );
            a.submit(user("hello")).await.expect("submit");
            a.session_id
        };

        // Now resume.
        let resumed_writer = SessionWriter::open(&path).await.expect("reopen");
        let resumed_config = AgentConfig::new(
            1,
            1,
            "test".into(),
            dir.path().to_path_buf(),
            resumed_writer,
        );
        let (agent, history) = Agent::resume_into(
            resumed_config,
            Arc::new(ScriptedProvider::from_events(Vec::new())),
            Arc::new(ToolRegistry::new()),
            CancellationToken::new(),
            Arc::new(crate::event::CollectingSink::new()),
            &path,
        )
        .await
        .expect("resume");

        assert_eq!(agent.session_id, original_id, "session_id must persist");
        // History should contain User + Assistant = 2 messages.
        assert_eq!(history.len(), 2);
        assert!(matches!(history[0].role, Role::User));
        assert!(matches!(history[1].role, Role::Assistant));
    }
}
