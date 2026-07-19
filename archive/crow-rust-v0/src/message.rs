//! Persisted chat-message model.
//!
//! A [`Message`] is the durable record of one turn in the conversation.
//! Each message carries an ordered list of [`Part`]s so that we can
//! represent text, reasoning, tool calls and tool results in a single
//! structured payload.
//!
//! Spec §10 pins this shape exactly. The `Role` enum intentionally has
//! no `System` variant — system prompts are injected by the runtime and
//! never persisted as a message.

use serde::{Deserialize, Serialize};

use crate::ids::{MessageId, ToolCallId};

/// Who produced a [`Message`].
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// Human-authored message.
    User,
    /// Model-authored message.
    Assistant,
    /// Result returned from a tool execution.
    ToolResult,
}

/// A single message in the conversation log.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Message {
    /// Unique identifier for this message.
    pub id: MessageId,
    /// Who produced the message.
    pub role: Role,
    /// Ordered body of the message.
    pub parts: Vec<Part>,
}

/// One piece of a message body.
///
/// All variants are **struct variants**, never newtype variants, because
/// `#[serde(tag = "kind")]` requires field names.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "kind", rename_all = "PascalCase")]
pub enum Part {
    /// Plain text content.
    Text {
        /// The text itself.
        text: String,
    },
    /// Reasoning / chain-of-thought content from the model.
    Reasoning {
        /// The reasoning text itself.
        text: String,
    },
    /// A model-issued tool invocation.
    ToolCall {
        /// Identifier matching the eventual [`Part::ToolResult`].
        id: ToolCallId,
        /// Tool name to dispatch to.
        name: String,
        /// Arguments as a JSON value.
        args: serde_json::Value,
    },
    /// The result of a tool invocation.
    ///
    /// `truncated` records whether the result was elided before
    /// persistence; `display` carries optional presentation hints for
    /// the UI layer (file path, line count, byte size).
    ToolResult {
        /// Identifier of the [`Part::ToolCall`] this answers.
        call_id: ToolCallId,
        /// The textual result body.
        output: String,
        /// Whether the tool returned an error result.
        is_error: bool,
        /// Whether the body was truncated before being stored.
        truncated: bool,
        /// Optional presentation hints for the UI.
        display: Option<DisplayDetails>,
    },
}

/// Optional presentation hints for a [`Part::ToolResult`].
///
/// Any subset of fields may be `Some`; consumers should fall back to
/// sensible defaults when all three are `None`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct DisplayDetails {
    /// Filesystem path the result refers to, if any.
    pub path: Option<std::path::PathBuf>,
    /// Number of lines in the result, if known.
    pub line_count: Option<u32>,
    /// Size of the result in bytes, if known.
    pub byte_size: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::new_id;

    #[test]
    fn role_has_no_system_variant() {
        // Compile-time guarantee that the variant is absent.
        let json = serde_json::to_string(&Role::User).expect("serialize");
        assert_eq!(json, "\"user\"");
        let json = serde_json::to_string(&Role::Assistant).expect("serialize");
        assert_eq!(json, "\"assistant\"");
        let json = serde_json::to_string(&Role::ToolResult).expect("serialize");
        assert_eq!(json, "\"toolresult\"");
    }

    #[test]
    fn message_round_trips() {
        let message = Message {
            id: MessageId(new_id()),
            role: Role::Assistant,
            parts: vec![
                Part::Text {
                    text: "Hello".into(),
                },
                Part::ToolCall {
                    id: ToolCallId(new_id()),
                    name: "shell".into(),
                    args: serde_json::json!({"cmd": "ls"}),
                },
                Part::ToolResult {
                    call_id: ToolCallId(new_id()),
                    output: "ok".into(),
                    is_error: false,
                    truncated: false,
                    display: Some(DisplayDetails {
                        path: Some(std::path::PathBuf::from("/tmp/out.txt")),
                        line_count: Some(1),
                        byte_size: Some(2),
                    }),
                },
            ],
        };
        let json = serde_json::to_string(&message).expect("serialize");
        let back: Message = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(message, back);
    }

    #[test]
    fn part_tool_result_carries_truncated_and_display() {
        let part = Part::ToolResult {
            call_id: ToolCallId(new_id()),
            output: "out".into(),
            is_error: false,
            truncated: true,
            display: None,
        };
        let value = serde_json::to_value(&part).expect("to_value");
        assert_eq!(value["kind"], "ToolResult");
        assert_eq!(value["truncated"], true);
        assert!(value["display"].is_null());
    }
}
