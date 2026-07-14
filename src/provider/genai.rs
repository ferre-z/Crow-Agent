use async_trait::async_trait;
use genai::adapter::AdapterKind;
use genai::chat::{
    ChatMessage, ChatOptions, ChatRequest, ChatRole, ChatStreamEvent, ContentPart, MessageContent,
};
use genai::resolver::{AuthData, Endpoint, ServiceTargetResolver};
use genai::{Client, ModelIden, ServiceTarget};
use tokio_stream::StreamExt;
use tokio_util::sync::CancellationToken;

use crate::event::{StopReason, Usage};
use crate::ids::{new_id, ToolCallId};
use crate::message::{Message, Part, Role};
use crate::provider::{ModelRequest, Provider, ProviderError, ProviderStream};
use crate::provider::{ProviderChunk, StreamAccumulator};

/// OpenAI-compatible provider backed by `genai` 0.6.5.
#[derive(Debug, Clone)]
pub struct GenaiProvider {
    client: Client,
    model: String,
}

impl GenaiProvider {
    /// Build a provider, reading the API key from `api_key_env` immediately.
    ///
    /// # Errors
    ///
    /// Returns an upstream `missing_api_key` error when the variable is absent.
    pub fn from_env(base_url: &str, model: &str, api_key_env: &str) -> Result<Self, ProviderError> {
        let api_key = std::env::var(api_key_env).map_err(|_| ProviderError::Upstream {
            code: "missing_api_key".to_owned(),
            message: format!("env var {api_key_env} not set"),
            retryable: false,
        })?;
        Ok(Self::with_api_key(base_url, model, api_key))
    }

    #[must_use]
    /// Build a provider using an explicit API key.
    pub fn with_api_key(base_url: &str, model: &str, api_key: String) -> Self {
        let endpoint = Endpoint::from_owned(base_url.to_owned());
        let auth = AuthData::from_single(api_key);
        let resolver = ServiceTargetResolver::from_resolver_fn(
            move |target: ServiceTarget| -> Result<ServiceTarget, genai::resolver::Error> {
                let ServiceTarget { model, .. } = target;
                let model = ModelIden::new(AdapterKind::OpenAI, model.model_name);
                Ok(ServiceTarget {
                    endpoint: endpoint.clone(),
                    auth: auth.clone(),
                    model,
                })
            },
        );
        let client = Client::builder()
            .with_service_target_resolver(resolver)
            .build();

        Self {
            client,
            model: model.to_owned(),
        }
    }
}

#[async_trait]
impl Provider for GenaiProvider {
    async fn stream(
        &self,
        req: ModelRequest,
        cancel: CancellationToken,
    ) -> Result<ProviderStream, ProviderError> {
        if cancel.is_cancelled() {
            return Err(ProviderError::Cancelled);
        }

        let chat_req = ChatRequest::new(req.messages.iter().map(message_to_chat).collect());
        let options = ChatOptions::default()
            .with_capture_tool_calls(true)
            .with_capture_usage(true)
            .with_capture_content(true)
            .with_capture_reasoning_content(true);
        let response = self
            .client
            .exec_chat_stream(&self.model, chat_req, Some(&options))
            .await
            .map_err(|error| ProviderError::Upstream {
                code: "genai_init".to_owned(),
                message: error.to_string(),
                retryable: false,
            })?;

        let mut upstream = response.stream;
        let (sender, receiver) = tokio::sync::mpsc::channel(64);
        tokio::spawn(async move {
            let mut accumulator = StreamAccumulator::new();
            loop {
                let item = tokio::select! {
                    () = cancel.cancelled() => {
                        let _ = sender.send(Err(ProviderError::Cancelled)).await;
                        break;
                    }
                    item = upstream.next() => item,
                };

                let Some(item) = item else {
                    if let Err(error) = accumulator.finish() {
                        let _ = sender
                            .send(Err(ProviderError::StreamInvalid(error.to_string())))
                            .await;
                    }
                    break;
                };
                let chunks = match item {
                    Ok(event) => event_to_chunks(event),
                    Err(error) => vec![ProviderChunk::Failed {
                        code: "upstream".to_owned(),
                        message: error.to_string(),
                        retryable: true,
                    }],
                };

                let terminal = chunks.iter().any(|chunk| {
                    matches!(
                        chunk,
                        ProviderChunk::Completed { .. } | ProviderChunk::Failed { .. }
                    )
                });
                for chunk in chunks {
                    let events = match accumulator.push_chunk(chunk) {
                        Ok(events) => events,
                        Err(error) => {
                            let _ = sender
                                .send(Err(ProviderError::StreamInvalid(error.to_string())))
                                .await;
                            return;
                        }
                    };
                    for event in events {
                        if sender.send(Ok(event)).await.is_err() {
                            return;
                        }
                    }
                }
                if terminal {
                    break;
                }
            }
        });

        Ok(ProviderStream {
            events: Box::pin(tokio_stream::wrappers::ReceiverStream::new(receiver)),
        })
    }
}

fn message_to_chat(message: &Message) -> ChatMessage {
    let role = match message.role {
        Role::User => ChatRole::User,
        Role::Assistant => ChatRole::Assistant,
        Role::ToolResult => ChatRole::Tool,
    };
    let parts = message
        .parts
        .iter()
        .filter_map(|part| match part {
            Part::Text { text } => Some(ContentPart::Text(text.clone())),
            Part::Reasoning { .. } | Part::ToolCall { .. } | Part::ToolResult { .. } => None,
        })
        .collect::<Vec<_>>();
    ChatMessage::new(role, MessageContent::from(parts))
}

fn event_to_chunks(event: ChatStreamEvent) -> Vec<ProviderChunk> {
    match event {
        ChatStreamEvent::Start => vec![ProviderChunk::Started],
        ChatStreamEvent::Chunk(chunk) => vec![ProviderChunk::TextDelta {
            text: chunk.content,
        }],
        ChatStreamEvent::ReasoningChunk(chunk) => vec![ProviderChunk::ReasoningDelta {
            text: chunk.content,
        }],
        ChatStreamEvent::ThoughtSignatureChunk(_) => Vec::new(),
        ChatStreamEvent::ToolCallChunk(chunk) => {
            let call_id = ToolCallId(new_id());
            vec![
                ProviderChunk::ToolCallStart {
                    call_id,
                    name: chunk.tool_call.fn_name,
                },
                ProviderChunk::ToolArgumentsDelta {
                    call_id,
                    fragment: chunk.tool_call.fn_arguments.to_string(),
                },
                ProviderChunk::ToolCallComplete { call_id },
            ]
        }
        ChatStreamEvent::End(end) => {
            let usage = end.captured_usage.unwrap_or_default();
            vec![ProviderChunk::Completed {
                usage: Usage {
                    input_tokens: nonnegative_u32(usage.prompt_tokens),
                    output_tokens: nonnegative_u32(usage.completion_tokens),
                },
                stop_reason: map_stop_reason(end.captured_stop_reason),
            }]
        }
    }
}

fn nonnegative_u32(value: Option<i32>) -> u32 {
    value
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(0)
}

fn map_stop_reason(reason: Option<genai::chat::StopReason>) -> StopReason {
    match reason {
        Some(genai::chat::StopReason::ToolCall(_)) => StopReason::ToolUse,
        Some(genai::chat::StopReason::MaxTokens(_)) => StopReason::MaxTokens,
        Some(genai::chat::StopReason::ContentFilter(_) | genai::chat::StopReason::Other(_)) => {
            StopReason::Error
        }
        Some(genai::chat::StopReason::Completed(_) | genai::chat::StopReason::StopSequence(_))
        | None => StopReason::EndTurn,
    }
}

#[cfg(test)]
mod tests {
    use genai::chat::{
        ChatStreamEvent, StopReason as GenaiStopReason, StreamChunk, StreamEnd, ToolCall, ToolChunk,
    };
    use serde_json::json;

    use super::GenaiProvider;
    use crate::event::{AgentEvent, StopReason, Usage};
    use crate::provider::{
        ModelRequest, Provider, ProviderChunk, ProviderError, StreamAccumulator,
    };

    #[test]
    fn from_env_missing_key_returns_err() {
        const ENV_NAME: &str = "CROW_TEST_MISSING_GENAI_API_KEY_2_2";
        // SAFETY: this test uses a task-specific variable that no other test reads.
        unsafe { std::env::remove_var(ENV_NAME) };

        let result = GenaiProvider::from_env("https://example.invalid/v1", "test-model", ENV_NAME);

        assert!(result.is_err());
    }

    #[test]
    fn with_api_key_constructs() {
        let provider = GenaiProvider::with_api_key(
            "https://example.invalid/v1",
            "test-model",
            "test-key".to_owned(),
        );

        let _ = provider;
    }

    #[test]
    fn start_and_text_events_map_to_provider_chunks() {
        let native_events = [
            ChatStreamEvent::Start,
            ChatStreamEvent::Chunk(StreamChunk {
                content: "hello".to_owned(),
            }),
        ];
        let mut accumulator = StreamAccumulator::new();
        let events = native_events
            .into_iter()
            .flat_map(super::event_to_chunks)
            .flat_map(|chunk| accumulator.push_chunk(chunk).expect("valid chunk"))
            .collect::<Vec<_>>();

        assert_eq!(
            events,
            vec![
                AgentEvent::ModelStarted,
                AgentEvent::TextDelta {
                    text: "hello".to_owned(),
                },
            ]
        );
    }

    #[test]
    fn tool_call_event_expands_for_accumulator() {
        let chunks = super::event_to_chunks(ChatStreamEvent::ToolCallChunk(ToolChunk {
            tool_call: ToolCall {
                call_id: "upstream-call-id".to_owned(),
                fn_name: "read".to_owned(),
                fn_arguments: json!({"path": "README.md"}),
                thought_signatures: None,
            },
        }));

        assert_eq!(chunks.len(), 3);
        assert!(matches!(
            &chunks[0],
            ProviderChunk::ToolCallStart { name, .. } if name == "read"
        ));
        assert!(matches!(
            &chunks[1],
            ProviderChunk::ToolArgumentsDelta { fragment, .. }
                if fragment == r#"{"path":"README.md"}"#
        ));
        assert!(matches!(
            (&chunks[0], &chunks[1], &chunks[2]),
            (
                ProviderChunk::ToolCallStart { call_id: start, .. },
                ProviderChunk::ToolArgumentsDelta { call_id: delta, .. },
                ProviderChunk::ToolCallComplete { call_id: complete },
            ) if start == delta && delta == complete
        ));

        let mut accumulator = StreamAccumulator::new();
        let events = chunks
            .into_iter()
            .flat_map(|chunk| accumulator.push_chunk(chunk).expect("valid tool chunk"))
            .collect::<Vec<_>>();
        assert!(matches!(
            events.as_slice(),
            [AgentEvent::ToolStarted { name, args, .. }]
                if name == "read" && args == &json!({"path": "README.md"})
        ));
    }

    #[test]
    fn end_event_maps_usage_and_stop_reason() {
        let chunks = super::event_to_chunks(ChatStreamEvent::End(StreamEnd {
            captured_usage: Some(genai::chat::Usage {
                prompt_tokens: Some(12),
                completion_tokens: Some(7),
                ..Default::default()
            }),
            captured_stop_reason: Some(GenaiStopReason::Completed("end_turn".to_owned())),
            ..Default::default()
        }));

        assert!(matches!(
            chunks.as_slice(),
            [ProviderChunk::Completed {
                usage: Usage {
                    input_tokens: 12,
                    output_tokens: 7,
                },
                stop_reason: StopReason::EndTurn,
            }]
        ));
    }

    #[test]
    fn max_tokens_stop_reason_is_preserved() {
        let chunks = super::event_to_chunks(ChatStreamEvent::End(StreamEnd {
            captured_stop_reason: Some(GenaiStopReason::MaxTokens("length".to_owned())),
            ..Default::default()
        }));

        assert!(matches!(
            chunks.as_slice(),
            [ProviderChunk::Completed {
                stop_reason: StopReason::MaxTokens,
                ..
            }]
        ));
    }

    #[tokio::test]
    async fn pre_cancelled_request_does_not_open_network_stream() {
        let provider = GenaiProvider::with_api_key(
            "https://example.invalid/v1",
            "test-model",
            "test-key".to_owned(),
        );
        let cancel = tokio_util::sync::CancellationToken::new();
        cancel.cancel();

        let result = provider
            .stream(
                ModelRequest {
                    messages: Vec::new(),
                    tools_schema: serde_json::Value::Null,
                },
                cancel,
            )
            .await;

        assert!(matches!(result, Err(ProviderError::Cancelled)));
    }
}
