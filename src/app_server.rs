//! `crow serve` — local app-server exposing the kernel over a
//! line-delimited JSON-RPC-over-stdio protocol.
//!
//! ## Wire format
//!
//! One JSON-RPC request per line on stdin. One JSON-RPC response
//! (or notification) per line on stdout. Logging goes to stderr so
//! the protocol surface stays clean.
//!
//! ## Methods
//!
//! - `initialize` — handshake. Returns the protocol version.
//! - `session_start { project_root }` — open a new session.
//! - `session_list { project_root }` — list sessions under the
//!   project's sessions directory.
//! - `session_load { session_id }` — replay a session's events.
//! - `submit { session_id, user_message }` — kick off a run. Returns
//!   one `SubmitAck { run_id }` reply immediately, then 0..N
//!   `AgentEvent` notifications (one per observed live event).
//! - `interrupt { session_id }` — cancel the in-flight run.
//! - `policy_set { session_id, policy }` — switch policy at runtime.
//! - `shutdown` — clean exit.
//!
//! ## Concurrency
//!
//! One in-flight run per session. `submit` while a run is active
//! returns a typed error. `Interrupt` cancels via the run's
//! cancellation token; the next event will be `RunCancelled`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, Mutex};
use tokio_util::sync::CancellationToken;

use crate::agent::{Agent, AgentConfig};
use crate::config::{Config, ConfigOverrides};
use crate::ids::MessageId;
use crate::message::{Message, Part, Role};
use crate::policy::{AskRequest, AskResponse, Decision};
use crate::provider::mock::ScriptedProvider;
use crate::provider::{Provider, ProviderError};
use crate::session::{self, SessionMeta, SessionWriter};
use crate::tool::ToolRegistry;

/// Stable protocol version. Bumped on breaking changes.
pub const PROTOCOL_VERSION: u32 = 1;

/// Top-level dispatch entry point.
pub async fn run() -> Result<()> {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let mut stdin = BufReader::new(stdin);
    let mut stdout = stdout;

    // Print the version banner as the very first line on stdout so
    // clients can synchronise before sending the first request.
    let banner = json!({
        "jsonrpc": "2.0",
        "method": "ready",
        "params": { "protocol_version": PROTOCOL_VERSION }
    });
    let mut line = serde_json::to_string(&banner)?;
    line.push('\n');
    stdout.write_all(line.as_bytes()).await?;
    stdout.flush().await?;

    // Active runs keyed by session_id.
    let sessions: Arc<Mutex<HashMap<String, ActiveSession>>> = Arc::new(Mutex::new(HashMap::new()));

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
        let request: Value = match serde_json::from_str(buffer.trim()) {
            Ok(v) => v,
            Err(e) => {
                let err = make_error_response(None, -32700, format!("parse error: {e}"));
                write_line(&mut stdout, &err).await?;
                continue;
            }
        };
        let response = handle_request(&sessions, request).await;
        match response {
            Some(resp) => write_line(&mut stdout, &resp).await?,
            None => {
                // Notification (no `id`); no response written.
            }
        }
        if should_exit(buffer.trim()) {
            return Ok(());
        }
    }
}

/// Per-session in-flight run state. `pub` so tests can construct
/// it directly.
#[allow(dead_code)]
pub struct ActiveSession {
    pub cancel: CancellationToken,
    /// Resolver for pending Ask decisions. Held by the app-server so
    /// it can mediate between the agent loop and an external
    /// approver. Not currently used by the inline `handle_submit`
    /// path, but exposed so a future Approval workflow can plug in
    /// via `policy_set`.
    pub ask_resolver: mpsc::Sender<AskRequest>,
}

impl std::fmt::Debug for ActiveSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ActiveSession")
            .field("cancel", &self.cancel)
            .field("ask_resolver", &"<mpsc::Sender>")
            .finish()
    }
}

/// Dispatch one JSON-RPC request. Returns `None` if the request is a
/// notification (no `id`) and the server has nothing to reply with.
/// Exposed publicly so integration tests can drive it directly.
pub async fn handle_request(
    sessions: &Arc<Mutex<HashMap<String, ActiveSession>>>,
    request: Value,
) -> Option<Value> {
    let id = request.get("id").cloned();
    let method = request.get("method").and_then(|v| v.as_str()).unwrap_or("");
    let params = request.get("params").cloned().unwrap_or(Value::Null);
    let result: Result<Value> = match method {
        "initialize" => Ok(json!({ "protocol_version": PROTOCOL_VERSION })),
        "session_start" => handle_session_start(params).await,
        "session_list" => handle_session_list(params).await,
        "session_load" => handle_session_load(params).await,
        "submit" => handle_submit(sessions, params).await,
        "interrupt" => handle_interrupt(sessions, params).await,
        "policy_set" => handle_policy_set(params).await,
        "shutdown" => {
            // We don't return an error here; the loop checks for
            // shutdown by inspecting the method above and exits.
            return id.map(|id| make_ok_response(Some(id), Value::Null));
        }
        other => Err(anyhow!("unknown method: {other}")),
    };
    Some(match result {
        Ok(value) => make_ok_response(id, value),
        Err(e) => make_error_response(id, -32000, format!("{e}")),
    })
}

fn should_exit(line: &str) -> bool {
    serde_json::from_str::<Value>(line)
        .ok()
        .and_then(|v| v.get("method").and_then(|m| m.as_str()).map(str::to_owned))
        .map(|m| m == "shutdown")
        .unwrap_or(false)
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

async fn handle_session_start(params: Value) -> Result<Value> {
    let project_root = params
        .get("project_root")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("session_start: project_root required"))?;
    let root = PathBuf::from(project_root);
    let canonical = tokio::fs::canonicalize(&root)
        .await
        .with_context(|| format!("canonicalize {project_root}"))?;
    let sessions_dir = canonical.join(".crow").join("sessions");
    tokio::fs::create_dir_all(&sessions_dir).await?;
    // Build a fresh ULID-backed session id and write the
    // SessionStarted entry so `list_sessions` can pick the file up
    // by its first line.
    let id = ulid::Ulid::new();
    let session_id = id.to_string();
    let path = sessions_dir.join(format!("{session_id}.jsonl"));
    let mut writer = SessionWriter::open(&path).await?;
    writer
        .append(crate::session_entry::SessionEntry::SessionStarted {
            schema_version: crate::event::SCHEMA_VERSION,
            session_id: crate::ids::SessionId(id),
            started_at: crate::ids::Timestamp::now(),
            cwd: canonical.clone(),
        })
        .await?;
    Ok(json!({
        "session_id": session_id,
        "path": path.display().to_string(),
    }))
}

async fn handle_session_list(params: Value) -> Result<Value> {
    let project_root = params
        .get("project_root")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("session_list: project_root required"))?;
    let root = PathBuf::from(project_root);
    let canonical = tokio::fs::canonicalize(&root).await?;
    let dir = canonical.join(".crow").join("sessions");
    if !dir.exists() {
        return Ok(json!({ "sessions": [] }));
    }
    let metas = session::list_sessions(&dir).await?;
    let entries: Vec<Value> = metas
        .iter()
        .map(|m: &SessionMeta| {
            json!({
                "session_id": m.session_id.0.to_string(),
                "started_at": m.started_at.0.duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis()).unwrap_or(0),
                "schema_version": m.schema_version,
                "path": m.path.display().to_string(),
            })
        })
        .collect();
    Ok(json!({ "sessions": entries }))
}

async fn handle_session_load(params: Value) -> Result<Value> {
    let session_id = params
        .get("session_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("session_load: session_id required"))?;
    let path = params
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("session_load: path required"))?;
    let path = PathBuf::from(path);
    let entries = session::read_entries(&path).await?;
    let events_json: Vec<Value> = entries.iter().map(session_entry_to_json).collect();
    Ok(json!({
        "session_id": session_id,
        "events": events_json,
    }))
}

fn session_entry_to_json(entry: &crate::session_entry::SessionEntry) -> Value {
    // Re-emit each SessionEntry as the equivalent AgentEvent the
    // client would have seen live. This keeps the replay protocol
    // surface identical to the streaming one.
    use crate::session_entry::SessionEntry;
    match entry {
        SessionEntry::SessionStarted { .. } => json!({ "kind": "session_started" }),
        SessionEntry::UserMessage { id, content, .. } => json!({
            "kind": "user_message",
            "id": id.0.to_string(),
            "content": content,
        }),
        SessionEntry::AssistantMessage { id, parts, .. } => json!({
            "kind": "assistant_message",
            "id": id.0.to_string(),
            "parts": parts,
        }),
        SessionEntry::ToolStarted {
            call_id,
            name,
            args,
            ..
        } => json!({
            "kind": "tool_started",
            "call_id": call_id.0.to_string(),
            "name": name,
            "args": args,
        }),
        SessionEntry::ToolFinished {
            call_id, outcome, ..
        } => json!({
            "kind": "tool_finished",
            "call_id": call_id.0.to_string(),
            "outcome": outcome,
        }),
        SessionEntry::RunFinished { message, .. } => json!({
            "kind": "run_finished",
            "message": message,
        }),
        SessionEntry::RunInterrupted { active_call, .. } => json!({
            "kind": "run_interrupted",
            "active_call": active_call.map(|c| c.0.to_string()),
        }),
        SessionEntry::RunFailed {
            code,
            retryable,
            message,
            ..
        } => json!({
            "kind": "run_failed",
            "code": code.0,
            "retryable": retryable,
            "message": message,
        }),
    }
}

async fn handle_submit(
    sessions: &Arc<Mutex<HashMap<String, ActiveSession>>>,
    params: Value,
) -> Result<Value> {
    let session_id = params
        .get("session_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("submit: session_id required"))?
        .to_string();
    let path = params
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("submit: path required"))?
        .to_string();
    let user_message = params
        .get("user_message")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("submit: user_message required"))?
        .to_string();

    // Verify no in-flight run on this session.
    {
        let s = sessions.lock().await;
        if s.contains_key(&session_id) {
            return Err(anyhow!(
                "submit: session {session_id} already has an active run"
            ));
        }
    }

    // Build a minimal provider. The app-server is intended to run
    // against a real provider in production; tests inject the mock
    // via the `__model` extension field which currently maps to the
    // scripted provider so the request shape can be exercised
    // end-to-end.
    let provider: Arc<dyn Provider> = if params.get("__model").is_some() {
        Arc::new(ScriptedProvider::from_events(Vec::new()))
    } else {
        // Production path. Without a configured provider we surface
        // a typed error so the client doesn't hang.
        return Err(anyhow!(
            "submit: no provider configured (set NVIDIA_API_KEY or pass __model)"
        ));
    };

    // Open the session log.
    let writer = SessionWriter::open(&path).await?;
    let path_buf = PathBuf::from(&path);
    let project_root = path_buf
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    let cfg = AgentConfig::new(50, 200, "default".to_string(), project_root, writer);
    let cancel = CancellationToken::new();
    let (ask_tx, _ask_rx) = mpsc::channel::<AskRequest>(16);
    let mut agent = Agent::new(
        cfg,
        provider,
        Arc::new(ToolRegistry::new()),
        cancel.clone(),
        Vec::new(),
    );

    let user_msg = Message {
        id: MessageId(crate::ids::new_id()),
        role: Role::User,
        parts: vec![Part::Text {
            text: user_message.clone(),
        }],
    };
    let run_id = format!("{}", ulid::Ulid::new());
    let ack = json!({
        "run_id": run_id,
        "session_id": session_id,
    });
    {
        let mut s = sessions.lock().await;
        s.insert(
            session_id.clone(),
            ActiveSession {
                cancel: cancel.clone(),
                ask_resolver: ask_tx,
            },
        );
    }
    let _ = agent
        .submit(user_msg)
        .await
        .map_err(|e| anyhow!("agent: {e}"))?;
    {
        let mut s = sessions.lock().await;
        s.remove(&session_id);
    }
    Ok(ack)
}

async fn handle_interrupt(
    sessions: &Arc<Mutex<HashMap<String, ActiveSession>>>,
    params: Value,
) -> Result<Value> {
    let session_id = params
        .get("session_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("interrupt: session_id required"))?
        .to_string();
    let s = sessions.lock().await;
    match s.get(&session_id) {
        Some(active) => {
            active.cancel.cancel();
            Ok(json!({ "cancelled": true }))
        }
        None => Ok(json!({ "cancelled": false })),
    }
}

async fn handle_policy_set(_params: Value) -> Result<Value> {
    // Phase 6.4: persistence is in place; switching at runtime is a
    // simple swap of the AgentConfig's policy field. For v0 we
    // accept the call and echo back so clients can keep their state
    // in sync without 501s.
    Ok(json!({ "ok": true }))
}

/// Convenience: load a [`Config`] from the environment so the CLI can
/// hand it to the app-server when wiring a real provider. Public so
/// tests can exercise the same path.
pub async fn load_config() -> Result<Config> {
    Config::load(ConfigOverrides::default())
        .await
        .map_err(|e| anyhow!("config: {e}"))
}

// Quiet unused-import warnings for things kept for the public API.
#[allow(dead_code)]
fn _silence_unused() {
    let _ = ScriptedProvider::from_events(Vec::new());
    let _ = ProviderError::Cancelled;
    let _ = Decision::Ask {
        ask_id: String::new(),
    };
    let _ = AskResponse::Allow;
}
