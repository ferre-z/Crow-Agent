//! MCP JSON-RPC-over-stdio dispatch loop.
//!
//! Mirrors `src/app_server.rs:24-130` exactly: read one JSON object per
//! line on stdin, write one JSON object per line on stdout, log to
//! stderr. The wire format is JSON-RPC 2.0 with the MCP lifecycle
//! extensions:
//!
//! - `initialize` — handshake; returns server info + protocol version.
//! - `notifications/initialized` — client confirmation; no response.
//! - `tools/list` — return the schemas from [`super::tools::tool_schemas`].
//! - `tools/call` — dispatch to [`super::tools::dispatch`].
//! - `ping` — liveness; returns `{}`.
//!
//! Notifications (no `id`) get no response. Errors are returned as
//! standard JSON-RPC error envelopes with `code`/`message`.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::mcp_opencode::registry::TaskRegistry;
use crate::mcp_opencode::runner::OpencodeRunner;
use crate::mcp_opencode::tools;

/// MCP protocol version we advertise. Matches the 2025-06-18 revision
/// of the spec.
pub const PROTOCOL_VERSION: &str = "2025-06-18";

/// JSON-RPC error code we use for protocol / method errors. Mirrors
/// the `-32000` server-error range used by `src/app_server.rs`.
const SERVER_ERROR_CODE: i32 = -32000;

/// Top-level entry point. Blocks until stdin closes (EOF).
///
/// `server_version` is the value embedded in `serverInfo.version` (we
/// use Crow's crate version); `binary` is the path to the `opencode`
/// binary the runner should invoke.
#[allow(clippy::missing_errors_doc)]
pub async fn run(
    runner: Arc<dyn OpencodeRunner>,
    binary: PathBuf,
    server_version: Arc<String>,
) -> Result<()> {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let mut stdin = BufReader::new(stdin);
    let mut stdout = stdout;

    let registry = TaskRegistry::new();

    let mut buffer = String::new();
    loop {
        buffer.clear();
        let n = stdin
            .read_line(&mut buffer)
            .await
            .context("reading stdin")?;
        if n == 0 {
            // EOF: clean shutdown.
            return Ok(());
        }
        let trimmed = buffer.trim();
        if trimmed.is_empty() {
            continue;
        }
        let request: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(e) => {
                let resp = make_error_response(None, -32700, format!("parse error: {e}"));
                write_line(&mut stdout, &resp).await?;
                continue;
            }
        };
        let response = handle_request(
            request,
            runner.clone(),
            binary.clone(),
            server_version.clone(),
            &registry,
        )
        .await;
        if let Some(resp) = response {
            write_line(&mut stdout, &resp).await?;
        }
    }
}

/// Dispatch one JSON-RPC request. Returns `None` for notifications.
pub async fn handle_request(
    request: Value,
    runner: Arc<dyn OpencodeRunner>,
    binary: PathBuf,
    server_version: Arc<String>,
    registry: &TaskRegistry,
) -> Option<Value> {
    let id = request.get("id").cloned();
    let method = request.get("method").and_then(|v| v.as_str()).unwrap_or("");
    let params = request.get("params").cloned().unwrap_or(Value::Null);
    tracing::debug!(method = %method, "mcp request");

    // Notifications carry no id and expect no response.
    let is_notification = id.is_none();
    let result: Result<Value> = match method {
        "initialize" => Ok(json!({
            "protocolVersion": PROTOCOL_VERSION,
            "serverInfo": {
                "name": "crow-mcp-opencode",
                "version": server_version.as_str()
            },
            "capabilities": {
                "tools": { "listChanged": false }
            },
            "instructions": "Delegate coding tasks to opencode, optionally in parallel. Use opencode_delegate for one task, opencode_delegate_parallel for many independent tasks, opencode_delegate_fanout to apply the same prompt to multiple working directories."
        })),
        "notifications/initialized" | "notifications/cancelled" => {
            // Lifecycle pings; no response needed.
            return None;
        }
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({
            "tools": tools::tool_schemas(),
        })),
        "tools/call" => {
            let name = match params.get("name").and_then(|v| v.as_str()) {
                Some(n) => n.to_string(),
                None => {
                    return Some(make_error_response(
                        id,
                        SERVER_ERROR_CODE,
                        "tools/call: name required".to_string(),
                    ));
                }
            };
            let arguments = params.get("arguments").cloned().unwrap_or(Value::Null);
            // tools/call is a *request* (it has an id), so we always
            // respond — the response content tells the client whether
            // the tool succeeded.
            let value = tools::dispatch(
                &name,
                arguments,
                runner.clone(),
                registry.clone(),
                server_version.clone(),
                binary.clone(),
            )
            .await;
            return Some(make_ok_response(id, value));
        }
        other => Err(anyhow::anyhow!("unknown method: {other}")),
    };

    if is_notification {
        None
    } else {
        Some(match result {
            Ok(value) => make_ok_response(id, value),
            Err(e) => make_error_response(id, SERVER_ERROR_CODE, format!("{e}")),
        })
    }
}

fn make_ok_response(id: Option<Value>, result: Value) -> Value {
    match id {
        Some(id) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        None => json!({ "jsonrpc": "2.0", "result": result }),
    }
}

fn make_error_response(id: Option<Value>, code: i32, message: String) -> Value {
    match id {
        Some(id) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": code, "message": message }
        }),
        None => json!({
            "jsonrpc": "2.0",
            "error": { "code": code, "message": message }
        }),
    }
}

async fn write_line<W: AsyncWriteExt + Unpin>(w: &mut W, v: &Value) -> Result<()> {
    let mut s = serde_json::to_string(v)?;
    s.push('\n');
    w.write_all(s.as_bytes()).await?;
    w.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp_opencode::events::OpencodeEvent;
    use crate::mcp_opencode::runner::{ScriptedRunner, ScriptedStep};
    use std::time::Duration;

    fn test_runner() -> Arc<dyn OpencodeRunner> {
        let steps = vec![ScriptedStep {
            event: OpencodeEvent::Done {
                message: "ok".into(),
            },
            delay: Duration::from_millis(1),
        }];
        Arc::new(ScriptedRunner::new(steps))
    }

    fn registry() -> TaskRegistry {
        TaskRegistry::new()
    }

    #[tokio::test]
    async fn initialize_advertises_capabilities() {
        let resp = handle_request(
            json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {} }),
            test_runner(),
            PathBuf::from("opencode"),
            Arc::new("0.1.0".to_string()),
            &registry(),
        )
        .await
        .expect("response");
        assert_eq!(resp["id"], 1);
        assert_eq!(resp["result"]["protocolVersion"], PROTOCOL_VERSION);
        assert_eq!(resp["result"]["serverInfo"]["name"], "crow-mcp-opencode");
        assert_eq!(
            resp["result"]["capabilities"]["tools"]["listChanged"],
            false
        );
    }

    #[tokio::test]
    async fn initialized_notification_yields_no_response() {
        let resp = handle_request(
            json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
            test_runner(),
            PathBuf::from("opencode"),
            Arc::new("0.1.0".to_string()),
            &registry(),
        )
        .await;
        assert!(resp.is_none(), "notifications must produce no response");
    }

    #[tokio::test]
    async fn tools_list_returns_seven_tools() {
        let resp = handle_request(
            json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {} }),
            test_runner(),
            PathBuf::from("opencode"),
            Arc::new("0.1.0".to_string()),
            &registry(),
        )
        .await
        .expect("response");
        let tools = resp["result"]["tools"].as_array().expect("array");
        assert_eq!(tools.len(), 7);
    }

    #[tokio::test]
    async fn tools_call_delegate_returns_content() {
        let resp = handle_request(
            json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "tools/call",
                "params": { "name": "opencode_delegate", "arguments": { "prompt": "hi" } }
            }),
            test_runner(),
            PathBuf::from("opencode"),
            Arc::new("0.1.0".to_string()),
            &registry(),
        )
        .await
        .expect("response");
        assert_eq!(resp["result"]["isError"], false);
        assert_eq!(resp["result"]["content"][0]["type"], "text");
    }

    #[tokio::test]
    async fn unknown_method_returns_error_envelope() {
        let resp = handle_request(
            json!({ "jsonrpc": "2.0", "id": 9, "method": "frobnicate", "params": {} }),
            test_runner(),
            PathBuf::from("opencode"),
            Arc::new("0.1.0".to_string()),
            &registry(),
        )
        .await
        .expect("response");
        assert_eq!(resp["id"], 9);
        assert_eq!(resp["error"]["code"], SERVER_ERROR_CODE);
        assert!(resp["error"]["message"]
            .as_str()
            .unwrap()
            .contains("frobnicate"));
    }

    #[tokio::test]
    async fn ping_returns_empty_object() {
        let resp = handle_request(
            json!({ "jsonrpc": "2.0", "id": 4, "method": "ping" }),
            test_runner(),
            PathBuf::from("opencode"),
            Arc::new("0.1.0".to_string()),
            &registry(),
        )
        .await
        .expect("response");
        assert_eq!(resp["result"], json!({}));
    }
}
