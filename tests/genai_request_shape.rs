//! Phase-1 gate test: prove that the live `genai` request path carries
//! the compiled system prompt + `AGENTS.md` content + every registered
//! tool (name, description, schema) + tool calls + tool results.
//!
//! The genai adapter hits the network, so we exercise the translation
//! functions directly (they are `pub(crate)` for exactly this reason).
//! The full Provider trait round-trip is covered by the unit tests in
//! `src/provider/genai.rs` against a `ScriptedProvider` event stream.

use std::path::PathBuf;

use crow::ids::{new_id, MessageId, ToolCallId};
use crow::message::{Message, Part, Role};
use crow::tool::{read::ReadTool, ToolRegistry};

use crow::context;
use crow::provider::genai::{message_to_chat, tools_from_schema};

#[test]
fn compiled_context_to_request_text_contains_prompt_and_instructions() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    // Layout:
    //   AGENTS.md          (root rule)
    //   src/AGENTS.md      (nested rule)
    //   src/lib.rs
    std::fs::create_dir_all(root.join("src")).expect("mkdir src");
    std::fs::write(root.join("AGENTS.md"), "root rule\n").expect("write root");
    std::fs::write(root.join("src/AGENTS.md"), "nested rule\n").expect("write nested");
    std::fs::write(root.join("src/lib.rs"), "// hi").expect("write lib");

    let cwd = root.join("src");
    let compiled = context::compile(root, &cwd).expect("compile");
    let text = compiled.to_request_text(root);

    // Embedded prompt is present.
    assert!(text.contains(&compiled.system_prompt));
    assert!(!compiled.system_prompt.is_empty());

    // Both instructions are present, with provenance headers.
    assert!(text.contains("## AGENTS.md"), "missing root provenance");
    assert!(text.contains("root rule"));
    assert!(
        text.contains("## src/AGENTS.md"),
        "missing nested provenance"
    );
    assert!(text.contains("nested rule"));
}

#[test]
fn tool_specs_carried_through_translator_with_descriptions() {
    // Build a registry with one tool and serialize it the way Agent
    // does. The translator must produce a `Tool` with name +
    // description + schema (not just schema).
    let mut registry = ToolRegistry::new();
    registry.register(ReadTool);
    let schema_value = serde_json::to_value(registry.tool_specs()).expect("serialize specs");

    let tools = tools_from_schema(&schema_value);
    assert_eq!(tools.len(), 1, "expected exactly one tool");

    // Inspect the produced tool via re-serialization so we don't reach
    // into private genai fields.
    let json = serde_json::to_value(&tools).expect("serialize tool");
    let arr = json.as_array().expect("array");
    let first = &arr[0];
    let map = first.as_object().expect("object");

    // genai's Tool uses a ToolName enum; custom names serialise as
    // the inner string.
    let name = map
        .get("name")
        .and_then(|v| v.as_str())
        .or_else(|| {
            map.get("name")
                .and_then(|v| v.get("Custom").and_then(|c| c.as_str()))
        })
        .unwrap_or("");
    assert_eq!(name, "read", "tool name must be preserved");

    let description = map
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert!(
        !description.is_empty(),
        "description must reach the provider (got empty: {first:?})"
    );
    assert!(
        description.to_lowercase().contains("read") || description.to_lowercase().contains("file"),
        "description should describe a file-reading tool, got: {description}"
    );

    // Schema is stored under `schema`, not `parameters`.
    let schema = map.get("schema").expect("schema");
    let schema_obj = schema.as_object().expect("schema object");
    assert!(
        schema_obj.contains_key("type") || schema_obj.contains_key("$schema"),
        "tool schema must look like a JSON Schema: {schema:?}"
    );
}

#[test]
fn tool_specs_are_emitted_in_lexicographic_order() {
    // Determinism gate: the live model sees the same tool order on
    // every turn. We register two tools and assert alphabetical order.
    use crow::tool::Tool;
    use schemars::schema::Schema;
    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};

    struct EchoTool;

    #[derive(Serialize, Deserialize, JsonSchema)]
    struct EchoArgs {
        msg: String,
    }

    #[async_trait::async_trait]
    impl Tool for EchoTool {
        fn name(&self) -> &'static str {
            "echo"
        }
        fn description(&self) -> &'static str {
            "Echoes its argument."
        }
        fn schema(&self) -> Schema {
            let root: schemars::schema::RootSchema = schemars::schema_for!(EchoArgs);
            serde_json::from_value(serde_json::to_value(&root.schema).unwrap())
                .expect("schema is serialisable")
        }
        async fn execute(
            &self,
            _args: serde_json::Value,
            _ctx: crow::tool::ToolContext,
            _events: crow::tool::ToolEventSink,
            _cancel: tokio_util::sync::CancellationToken,
        ) -> crow::tool::ToolResult {
            unimplemented!()
        }
    }

    let mut registry = ToolRegistry::new();
    // Register out of alphabetical order.
    registry.register(ReadTool);
    registry.register(EchoTool);

    let specs = registry.tool_specs();
    let names: Vec<&str> = specs.iter().map(|s| s.name).collect();
    assert_eq!(names, vec!["echo", "read"]);

    // And the genai adapter must respect that order.
    let schema_value = serde_json::to_value(&specs).expect("serialize");
    let tools = tools_from_schema(&schema_value);
    let emitted: Vec<String> = serde_json::to_value(&tools)
        .expect("serialize")
        .as_array()
        .expect("array")
        .iter()
        .filter_map(|v| {
            v.get("name")
                .and_then(|n| n.as_str().map(str::to_owned))
                .or_else(|| {
                    v.get("name")
                        .and_then(|n| n.get("Custom").and_then(|c| c.as_str().map(str::to_owned)))
                })
        })
        .collect();
    assert_eq!(emitted, vec!["echo", "read"]);
}

#[test]
fn assistant_tool_call_message_renders_as_tool_calls_content() {
    // The round-trip for tool calls: a Crow assistant message with one
    // ToolCall part must translate to a genai ChatMessage whose
    // content carries exactly one tool_call.
    let message = Message {
        id: MessageId(new_id()),
        role: Role::Assistant,
        parts: vec![Part::ToolCall {
            id: ToolCallId(new_id()),
            name: "read".to_string(),
            args: serde_json::json!({"path": "src/lib.rs"}),
        }],
    };
    let chat = message_to_chat(&message);
    let value = serde_json::to_value(&chat).expect("serialize");
    // genai's ChatRole serialises in PascalCase.
    assert_eq!(value["role"], "Assistant");
    // MessageContent is #[serde(transparent)] around `Vec<ContentPart>`,
    // so `content` is itself an array. ContentPart uses serde's
    // default external tagging, so a ToolCall tuple variant
    // serialises as `{"ToolCall": {<inner>}}`.
    let parts = value["content"]
        .as_array()
        .expect("content is a parts array");
    assert_eq!(parts.len(), 1);
    let tool_call = parts[0]["ToolCall"].as_object().expect("ToolCall object");
    assert_eq!(tool_call["fn_name"], "read");
    assert_eq!(tool_call["fn_arguments"]["path"], "src/lib.rs");
    let call_id = tool_call["call_id"].as_str().expect("call_id string");
    assert!(!call_id.is_empty());
}

#[test]
fn tool_result_message_renders_as_tool_responses() {
    // The round-trip for tool results: a Crow ToolResult message must
    // translate to a genai Tool-role message whose content carries
    // exactly one tool_response.
    let call_id = ToolCallId(new_id());
    let message = Message {
        id: MessageId(new_id()),
        role: Role::ToolResult,
        parts: vec![Part::ToolResult {
            call_id,
            output: "file contents".to_string(),
            is_error: false,
            truncated: false,
            display: None,
        }],
    };
    let chat = message_to_chat(&message);
    let value = serde_json::to_value(&chat).expect("serialize");
    assert_eq!(value["role"], "Tool");
    // ContentPart::ToolResponse -> `{"ToolResponse": {<inner>}}`.
    let parts = value["content"].as_array().expect("parts array");
    assert_eq!(parts.len(), 1);
    let response = parts[0]["ToolResponse"]
        .as_object()
        .expect("ToolResponse object");
    assert_eq!(
        response["content"].as_str(),
        Some("file contents"),
        "tool response content must round-trip"
    );
    assert_eq!(
        response["call_id"].as_str(),
        Some(call_id.0.to_string().as_str()),
        "call_id must round-trip"
    );
}

#[test]
fn tool_result_with_is_error_is_prefixed() {
    let call_id = ToolCallId(new_id());
    let message = Message {
        id: MessageId(new_id()),
        role: Role::ToolResult,
        parts: vec![Part::ToolResult {
            call_id,
            output: "permission denied".to_string(),
            is_error: true,
            truncated: false,
            display: None,
        }],
    };
    let chat = message_to_chat(&message);
    let value = serde_json::to_value(&chat).expect("serialize");
    let response = value["content"][0]["ToolResponse"]
        .as_object()
        .expect("ToolResponse object");
    let content = response["content"].as_str().expect("content");
    assert!(
        content.starts_with("ERROR: "),
        "is_error=true results must be prefixed so the model sees the failure: {content}"
    );
}

#[test]
fn assistant_text_message_renders_as_text_content() {
    let message = Message {
        id: MessageId(new_id()),
        role: Role::Assistant,
        parts: vec![Part::Text {
            text: "All done.".to_string(),
        }],
    };
    let chat = message_to_chat(&message);
    let value = serde_json::to_value(&chat).expect("serialize");
    assert_eq!(value["role"], "Assistant");
    // ContentPart::Text tuple variant -> `{"Text": "<inner>"}`.
    let parts = value["content"].as_array().expect("parts array");
    assert!(!parts.is_empty());
    let text = parts[0]["Text"].as_str().expect("text");
    assert_eq!(text, "All done.");
}

#[test]
fn user_message_with_text_renders_as_text_content() {
    let message = Message {
        id: MessageId(new_id()),
        role: Role::User,
        parts: vec![Part::Text {
            text: "read the file".to_string(),
        }],
    };
    let chat = message_to_chat(&message);
    let value = serde_json::to_value(&chat).expect("serialize");
    assert_eq!(value["role"], "User");
    let parts = value["content"].as_array().expect("parts array");
    assert_eq!(parts[0]["Text"].as_str(), Some("read the file"));
}

#[test]
fn legacy_tools_schema_without_descriptions_is_still_accepted() {
    // Backwards-compatibility: callers that still send the legacy
    // `{"name": schema, ...}` object (no description) must continue to
    // work. The adapter emits a tool with the name and schema, no
    // description.
    let legacy = serde_json::json!({
        "read": {"type": "object", "properties": {"path": {"type": "string"}}}
    });
    let tools = tools_from_schema(&legacy);
    assert_eq!(tools.len(), 1);
    let json = serde_json::to_value(&tools).expect("serialize");
    let name_value = &json[0]["name"];
    let name = name_value
        .as_str()
        .or_else(|| name_value.get("Custom").and_then(|c| c.as_str()))
        .unwrap_or("");
    assert_eq!(name, "read");
    // description field absent or empty — the genai Tool serializes
    // its description only when set.
    if let Some(desc) = json[0].get("description") {
        assert_eq!(desc.as_str().unwrap_or(""), "");
    }
}

// Keep an explicit reference so the registry import is not flagged
// unused when no other test reaches it.
#[allow(dead_code)]
fn _tool_registry_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}
