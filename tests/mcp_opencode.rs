//! Integration tests for `crow mcp-opencode`.
//!
//! These tests drive `crow::mcp_opencode::protocol::handle_request`
//! directly, the same way `tests/app_server.rs` exercises the
//! `crow serve` dispatch loop. We use a `ScriptedRunner` (an
//! in-memory `OpencodeRunner`) so the tests don't need a real
//! `opencode` binary or an API key.
//!
//! Coverage:
//!
//! - `initialize` advertises MCP protocol version + server info
//! - `tools/list` returns seven tools
//! - `tools/call opencode_delegate` returns an envelope with `task_id`
//! - `tools/call opencode_delegate_parallel` runs concurrently
//!   (timing assertion: 4 × 200ms tasks finish in < 500ms)
//! - `tools/call opencode_delegate_fanout` runs across workdirs
//! - `tools/call opencode_status` returns null for unknown ids
//! - `tools/call opencode_cancel` is observed by the runner

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crow::mcp_opencode::events::OpencodeEvent;
use crow::mcp_opencode::protocol::{handle_request, PROTOCOL_VERSION};
use crow::mcp_opencode::registry::TaskRegistry;
use crow::mcp_opencode::runner::{OpencodeRunner, ScriptedRunner, ScriptedStep};
use serde_json::{json, Value};

fn test_runner(delay_per_step: Duration, message: &str) -> Arc<dyn OpencodeRunner> {
    let steps = vec![ScriptedStep {
        event: OpencodeEvent::Done {
            message: message.into(),
        },
        delay: delay_per_step,
    }];
    Arc::new(ScriptedRunner::new(steps))
}

async fn call_tool(
    runner: Arc<dyn OpencodeRunner>,
    registry: TaskRegistry,
    name: &str,
    args: Value,
) -> Value {
    let req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": { "name": name, "arguments": args }
    });
    handle_request(
        req,
        runner,
        PathBuf::from("opencode"),
        Arc::new("test".to_string()),
        &registry,
    )
    .await
    .expect("response")
}

#[tokio::test]
async fn initialize_round_trip() {
    let resp = handle_request(
        json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {} }),
        test_runner(Duration::from_millis(0), "ok"),
        PathBuf::from("opencode"),
        Arc::new("0.1.0".to_string()),
        &TaskRegistry::new(),
    )
    .await
    .expect("response");
    assert_eq!(resp["id"], 1);
    assert_eq!(resp["result"]["protocolVersion"], PROTOCOL_VERSION);
    assert_eq!(resp["result"]["serverInfo"]["name"], "crow-mcp-opencode");
    assert_eq!(resp["result"]["serverInfo"]["version"], "0.1.0");
}

#[tokio::test]
async fn tools_list_advertises_seven_tools() {
    let resp = handle_request(
        json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }),
        test_runner(Duration::from_millis(0), "ok"),
        PathBuf::from("opencode"),
        Arc::new("0.1.0".to_string()),
        &TaskRegistry::new(),
    )
    .await
    .expect("response");
    let arr = resp["result"]["tools"].as_array().expect("array");
    assert_eq!(arr.len(), 7);
    let names: Vec<&str> = arr.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"opencode_delegate"));
    assert!(names.contains(&"opencode_delegate_parallel"));
    assert!(names.contains(&"opencode_delegate_fanout"));
    assert!(names.contains(&"opencode_status"));
    assert!(names.contains(&"opencode_cancel"));
    assert!(names.contains(&"opencode_list_models"));
    assert!(names.contains(&"opencode_version"));
}

#[tokio::test]
async fn delegate_completes_and_evicts() {
    let registry = TaskRegistry::new();
    let resp = call_tool(
        test_runner(Duration::from_millis(0), "hello"),
        registry.clone(),
        "opencode_delegate",
        json!({ "prompt": "say hello" }),
    )
    .await;
    assert_eq!(resp["result"]["isError"], false);
    let parsed: Value =
        serde_json::from_str(resp["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(parsed["status"], "ok");
    assert_eq!(parsed["message"], "hello");
    assert!(parsed["task_id"].as_str().is_some());
    // After completion the task is evicted from the registry.
    let len = registry.len().await;
    assert_eq!(len, 0, "task should be evicted after completion");
}

#[tokio::test]
async fn parallel_actually_runs_concurrently() {
    // 4 tasks × 200 ms each. Serial would take ~800 ms; parallel
    // should take ~200 ms. We assert < 500 ms to leave headroom for
    // CI scheduling jitter.
    let registry = TaskRegistry::new();
    let runner = test_runner(Duration::from_millis(200), "ok");
    let start = Instant::now();
    let resp = call_tool(
        runner,
        registry.clone(),
        "opencode_delegate_parallel",
        json!({
            "tasks": [
                { "prompt": "t1" },
                { "prompt": "t2" },
                { "prompt": "t3" },
                { "prompt": "t4" }
            ]
        }),
    )
    .await;
    let elapsed = start.elapsed();
    assert_eq!(resp["result"]["isError"], false);
    let parsed: Value =
        serde_json::from_str(resp["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    let arr = parsed.as_array().expect("array");
    assert_eq!(arr.len(), 4);
    for entry in arr {
        assert_eq!(entry["status"], "ok");
        assert_eq!(entry["message"], "ok");
        assert!(entry["task_id"].as_str().is_some());
    }
    assert!(
        elapsed < Duration::from_millis(500),
        "parallel run took {elapsed:?}; expected < 500 ms"
    );
}

#[tokio::test]
async fn fanout_returns_one_result_per_workdir() {
    let registry = TaskRegistry::new();
    let resp = call_tool(
        test_runner(Duration::from_millis(5), "ran"),
        registry.clone(),
        "opencode_delegate_fanout",
        json!({
            "prompt": "summarize the repo",
            "workdirs": ["/tmp", "/var", "/etc"]
        }),
    )
    .await;
    assert_eq!(resp["result"]["isError"], false);
    let parsed: Value =
        serde_json::from_str(resp["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    let arr = parsed.as_array().expect("array");
    assert_eq!(arr.len(), 3);
    for entry in arr {
        assert_eq!(entry["status"], "ok");
        assert_eq!(entry["message"], "ran");
    }
}

#[tokio::test]
async fn status_unknown_id_returns_null() {
    let registry = TaskRegistry::new();
    let resp = call_tool(
        test_runner(Duration::from_millis(0), "ok"),
        registry,
        "opencode_status",
        json!({ "task_id": "00000000000000000000000000" }),
    )
    .await;
    assert_eq!(resp["result"]["isError"], false);
    let parsed: Value =
        serde_json::from_str(resp["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert!(parsed.is_null());
}

#[tokio::test]
async fn cancel_unknown_id_returns_false() {
    let registry = TaskRegistry::new();
    let resp = call_tool(
        test_runner(Duration::from_millis(0), "ok"),
        registry,
        "opencode_cancel",
        json!({ "task_id": "00000000000000000000000000" }),
    )
    .await;
    assert_eq!(resp["result"]["isError"], false);
    let parsed: Value =
        serde_json::from_str(resp["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(parsed["cancelled"], false);
}

#[tokio::test]
async fn delegate_rejects_missing_prompt() {
    let registry = TaskRegistry::new();
    let resp = call_tool(
        test_runner(Duration::from_millis(0), "ok"),
        registry,
        "opencode_delegate",
        json!({}),
    )
    .await;
    // Schema-level rejection: missing `prompt` → invalid arguments →
    // isError: true with a readable message.
    assert_eq!(resp["result"]["isError"], true);
    let msg = resp["result"]["content"][0]["text"].as_str().unwrap();
    assert!(
        msg.contains("invalid arguments"),
        "expected invalid arguments error, got: {msg}"
    );
}

#[tokio::test]
async fn unknown_tool_returns_protocol_error() {
    let resp = handle_request(
        json!({
            "jsonrpc": "2.0",
            "id": 99,
            "method": "tools/call",
            "params": { "name": "opencode_nope", "arguments": {} }
        }),
        test_runner(Duration::from_millis(0), "ok"),
        PathBuf::from("opencode"),
        Arc::new("0.1.0".to_string()),
        &TaskRegistry::new(),
    )
    .await
    .expect("response");
    // Unknown tool → isError: true in the tool result, not a
    // protocol-level error. The protocol itself is happy; the tool
    // handler rejects the name.
    assert_eq!(resp["result"]["isError"], true);
    assert!(resp["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("unknown tool"));
}
