//! `crow serve` — local app-server exposing the kernel over a
//! line-delimited JSON-RPC-over-stdio protocol.
//!
//! ## Wire format
//!
//! One JSON-RPC request per line on stdin. One JSON-RPC response
//! (or notification) per line on stdout. Logging goes to stderr so
//! the protocol surface stays clean. All stdout writes are serialised
//! through a single writer task so response lines and pushed
//! notifications never interleave mid-line.
//!
//! ## Methods
//!
//! - `initialize` — handshake. Returns the protocol version.
//! - `session_start { project_root }` — open a new session.
//! - `session_list { project_root }` — list sessions under the
//!   project's sessions directory.
//! - `session_load { session_id, path, project_root }` — replay a
//!   session's events (as `kind`-tagged objects).
//! - `submit { session_id, path, user_message, project_root }` — kick
//!   off a run. Replies with one `SubmitAck { run_id, session_id }`
//!   immediately, then pushes 0..N `event` notifications (one per
//!   live [`AgentEvent`]) and 0..N `ask` notifications for pending
//!   approvals.
//! - `interrupt { session_id }` — cancel the in-flight run. The next
//!   pushed event is `RunCancelled`.
//! - `ask_resolve { session_id, ask_id, decision }` — answer a pending
//!   approval (`decision` is `"allow"` or `"deny"`).
//! - `policy_set { session_id, policy }` — no-op in v0; accepted for
//!   forward compatibility. Approval is fixed to `DefaultPolicy`.
//! - `shutdown` — clean exit (cancels all in-flight runs).
//!
//! ## Server-pushed notifications
//!
//! - `{"jsonrpc":"2.0","method":"ready","params":{"protocol_version":1}}`
//!   — emitted once on startup before any request is read.
//! - `{"jsonrpc":"2.0","method":"event","params":{session_id,run_id,seq,event}}`
//!   — one per live `AgentEvent`; `event` is the `type`-tagged frame.
//! - `{"jsonrpc":"2.0","method":"ask","params":{ask_id,call:{call_id,name,args}}}`
//!   — a tool call is awaiting an `ask_resolve`.
//!
//! ## Concurrency
//!
//! One in-flight run per session. `submit` while a run is active
//! returns a typed error. The run executes on a spawned task so the
//! stdin loop keeps reading — `interrupt` and `ask_resolve` are
//! processed while the run is live.
//!
//! ## Path validation
//!
//! `submit` and `session_load` require `project_root` and validate
//! that the caller-supplied `path` is the expected
//! `<project_root>/.crow/sessions/<session_id>.jsonl`. The `project_root`
//! is the source of truth for the agent's working directory.
//!
//! ## Test hooks
//!
//! `submit` recognises `__script`, `__model`, `__max_turns`, and
//! `__max_tool_calls` parameters. These are **only** present when the
//! crate is built with the `serve-test-hooks` cargo feature (or under
//! `cfg(test)`); production builds ignore them, so a shipped `crow
//! serve` can never run a scripted provider or override safety limits.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use secrecy::ExposeSecret;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio_util::sync::CancellationToken;

use crate::agent::{Agent, AgentConfig};
use crate::config::{Config, ConfigOverrides};
use crate::event::{AgentEvent, ChannelSink};
use crate::ids::MessageId;
use crate::message::{Message, Part, Role};
use crate::policy::{AskRequest, AskResponse};
use crate::provider::genai::GenaiProvider;
#[cfg(any(test, feature = "serve-test-hooks"))]
use crate::provider::mock::ScriptedProvider;
use crate::provider::Provider;
use crate::session::{self, SessionMeta, SessionWriter};
use crate::tool::{BashTool, EditTool, ReadTool, ToolRegistry, WriteTool};

/// Stable protocol version. Bumped on breaking changes.
pub const PROTOCOL_VERSION: u32 = 1;

/// Capacity of the outbound stdout channel and per-run event sink.
/// Large enough to absorb short bursts of tool output (terminal events
/// are guaranteed by the backstop below, not by capacity).
const CHANNEL_CAP: usize = 1024;

/// A sender for one JSON line to stdout. Cloned into the run's event
/// and ask forwarders so every producer writes through the same task.
type OutboundTx = mpsc::Sender<Value>;

/// Top-level dispatch entry point.
pub async fn run() -> Result<()> {
    let stdin = tokio::io::stdin();
    let mut stdin = BufReader::new(stdin);

    // Single writer task: everything reaches stdout through this
    // channel, so response lines and pushed notifications can never
    // interleave within a line. Serialization errors are logged to
    // stderr so a bug doesn't silently disappear.
    let (out_tx, mut out_rx) = mpsc::channel::<Value>(CHANNEL_CAP);
    let writer = tokio::spawn(async move {
        let mut stdout = tokio::io::stdout();
        while let Some(value) = out_rx.recv().await {
            let mut line = match serde_json::to_string(&value) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("app-server: failed to serialise outbound value: {e}");
                    continue;
                }
            };
            line.push('\n');
            if stdout.write_all(line.as_bytes()).await.is_err() {
                break;
            }
            let _ = stdout.flush().await;
        }
    });

    // Ready banner: first line out, before any request is read.
    let banner = json!({
        "jsonrpc": "2.0",
        "method": "ready",
        "params": { "protocol_version": PROTOCOL_VERSION }
    });
    let _ = out_tx.send(banner).await;

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
            // EOF: clean shutdown — cancel in-flight runs so their
            // child processes unwind before the process exits.
            cancel_all_runs(&sessions).await;
            break;
        }
        let trimmed = buffer.trim();
        if trimmed.is_empty() {
            continue;
        }
        let request: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(e) => {
                let _ = out_tx
                    .send(make_error_response(
                        None,
                        -32700,
                        format!("parse error: {e}"),
                    ))
                    .await;
                continue;
            }
        };
        let is_shutdown = request.get("method").and_then(|m| m.as_str()) == Some("shutdown");
        if let Some(resp) = handle_request(&sessions, &out_tx, request).await {
            let _ = out_tx.send(resp).await;
        }
        if is_shutdown {
            cancel_all_runs(&sessions).await;
            break;
        }
    }

    // Close the channel and let the writer drain any queued lines.
    drop(out_tx);
    let _ = writer.await;
    Ok(())
}

/// Cancel every active run so their child processes unwind on
/// shutdown. Idempotent: `CancellationToken::cancel` is.
async fn cancel_all_runs(sessions: &Arc<Mutex<HashMap<String, ActiveSession>>>) {
    let s = sessions.lock().await;
    for active in s.values() {
        active.cancel.cancel();
    }
}

/// Per-session in-flight run state. `pub` so tests can construct it
/// directly.
pub struct ActiveSession {
    /// Cancels the run when `interrupt` fires.
    pub cancel: CancellationToken,
    /// Resolver the agent loop sends pending Ask requests through. Held
    /// so the sender side stays alive for the run's duration.
    pub ask_resolver: mpsc::Sender<AskRequest>,
    /// Oneshot response senders for asks awaiting an `ask_resolve`,
    /// keyed by `ask_id`. The ask forwarder inserts; `ask_resolve`
    /// removes and fires.
    pub pending_asks: Arc<Mutex<HashMap<String, oneshot::Sender<AskResponse>>>>,
}

impl std::fmt::Debug for ActiveSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ActiveSession")
            .field("cancel", &self.cancel)
            .field("ask_resolver", &"<mpsc::Sender>")
            .field("pending_asks", &"<Mutex<HashMap>>")
            .finish()
    }
}

/// Dispatch one JSON-RPC request. Returns `Some(response)` for a
/// request/response method, or `None` when the reply is streamed
/// out-of-band (`submit` sends its ack directly) or there is nothing
/// to reply with. Exposed publicly so integration tests can drive it
/// directly.
pub async fn handle_request(
    sessions: &Arc<Mutex<HashMap<String, ActiveSession>>>,
    out: &OutboundTx,
    request: Value,
) -> Option<Value> {
    let id = request.get("id").cloned();
    let method = request.get("method").and_then(|v| v.as_str()).unwrap_or("");
    let params = request.get("params").cloned().unwrap_or(Value::Null);

    // `submit` streams its own ack through `out` (before any events),
    // so it returns no direct response on success.
    if method == "submit" {
        return match handle_submit(sessions, out, id.clone(), params).await {
            Ok(()) => None,
            Err(e) => Some(make_error_response(id, -32000, format!("{e}"))),
        };
    }
    // `shutdown` replies (if it has an id); the run loop handles exit.
    if method == "shutdown" {
        return id.map(|id| make_ok_response(Some(id), Value::Null));
    }

    let result: Result<Value> = match method {
        "initialize" => Ok(json!({ "protocol_version": PROTOCOL_VERSION })),
        "session_start" => handle_session_start(params).await,
        "session_list" => handle_session_list(params).await,
        "session_load" => handle_session_load(params).await,
        "interrupt" => handle_interrupt(sessions, params).await,
        "ask_resolve" => handle_ask_resolve(sessions, params).await,
        "policy_set" => handle_policy_set(params).await,
        other => Err(anyhow!("unknown method: {other}")),
    };
    Some(match result {
        Ok(value) => make_ok_response(id, value),
        Err(e) => make_error_response(id, -32000, format!("{e}")),
    })
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
    // Drop the writer explicitly so its lockfile is released before a
    // subsequent `submit` reopens the same log.
    drop(writer);
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

/// Validate that `path` (canonicalised) equals the expected
/// `<project_root>/.crow/sessions/<session_id>.jsonl`. Returns the
/// canonical `project_root` on success — this is the working
/// directory the agent uses for tool execution.
fn validate_session_path(project_root: &str, session_id: &str, path: &str) -> Result<PathBuf> {
    let root = PathBuf::from(project_root);
    let canonical_root = std::fs::canonicalize(&root)
        .with_context(|| format!("canonicalize project_root {project_root}"))?;
    let expected = canonical_root
        .join(".crow")
        .join("sessions")
        .join(format!("{session_id}.jsonl"));
    let provided = std::fs::canonicalize(PathBuf::from(path))
        .with_context(|| format!("canonicalize path {path}"))?;
    if provided != expected {
        return Err(anyhow!(
            "path mismatch: expected {expected:?}, got {provided:?}"
        ));
    }
    Ok(canonical_root)
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
    let project_root = params
        .get("project_root")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("session_load: project_root required"))?;
    let _ = validate_session_path(project_root, session_id, path)?;
    let path = PathBuf::from(path);
    let entries = session::read_entries(&path).await?;
    let events_json: Vec<Value> = entries.iter().map(session_entry_to_json).collect();
    Ok(json!({
        "session_id": session_id,
        "events": events_json,
    }))
}

fn session_entry_to_json(entry: &crate::session_entry::SessionEntry) -> Value {
    // Re-emit each SessionEntry as a `kind`-tagged replay object. This
    // is intentionally distinct from the `type`-tagged live
    // `AgentEvent` frames — the GUI reducer handles both shapes.
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

/// Default tool registry for the app-server: `read`, `write`, `edit`,
/// `bash`. Mirrors the CLI so allowed tool calls actually execute.
fn default_registry() -> Arc<ToolRegistry> {
    let mut reg = ToolRegistry::new();
    reg.register(ReadTool);
    reg.register(WriteTool);
    reg.register(EditTool);
    reg.register(BashTool);
    Arc::new(reg)
}

/// Build the production provider from layered config/env. Returns
/// `None` when no API key is configured so the caller can surface a
/// typed "no provider" error instead of hanging on an empty mock.
async fn build_env_provider() -> Option<(Arc<dyn Provider>, String, u32, u32)> {
    let cfg: Config = load_config().await.ok()?;
    let key = cfg.api_key.expose_secret();
    if key.is_empty() {
        return None;
    }
    let provider = GenaiProvider::with_api_key(&cfg.base_url, &cfg.model, key.to_string());
    Some((
        Arc::new(provider),
        cfg.model,
        cfg.max_turns,
        cfg.max_tool_calls,
    ))
}

/// Kick off a run. Streams the ack through `out` (before any events),
/// then spawns the run plus the event and ask forwarders. The run
/// task guarantees exactly one terminal `event` reaches `out` — if
/// the sink dropped the agent's terminal event, a synthetic one is
/// pushed so the GUI never hangs. Returns `Ok(())` once the run is
/// launched; `Err` before launch surfaces as a JSON-RPC error.
async fn handle_submit(
    sessions: &Arc<Mutex<HashMap<String, ActiveSession>>>,
    out: &OutboundTx,
    id: Option<Value>,
    params: Value,
) -> Result<()> {
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
    let project_root = params
        .get("project_root")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("submit: project_root required"))?;

    // One in-flight run per session.
    {
        let s = sessions.lock().await;
        if s.contains_key(&session_id) {
            return Err(anyhow!(
                "submit: session {session_id} already has an active run"
            ));
        }
    }

    // Validate the path before we open anything.
    let canonical_root = validate_session_path(project_root, &session_id, &path)?;

    // Provider selection. Test hooks are compile-gated; in a release
    // build the only way to run a submit is with a real provider.
    #[cfg(any(test, feature = "serve-test-hooks"))]
    let scripted: Option<Arc<dyn Provider>> = {
        if let Some(script) = params.get("__script") {
            let events: Vec<AgentEvent> = serde_json::from_value(script.clone())
                .map_err(|e| anyhow!("submit: invalid __script: {e}"))?;
            Some(Arc::new(ScriptedProvider::from_events(events)))
        } else if params.get("__model").is_some() {
            Some(Arc::new(ScriptedProvider::from_events(Vec::new())))
        } else {
            None
        }
    };
    #[cfg(not(any(test, feature = "serve-test-hooks")))]
    let scripted: Option<Arc<dyn Provider>> = None;

    let (provider, model, max_turns, max_tool_calls): (Arc<dyn Provider>, String, u32, u32) =
        if let Some(s) = scripted {
            let max_turns = params
                .get("__max_turns")
                .and_then(Value::as_u64)
                .unwrap_or(50) as u32;
            let max_tool_calls = params
                .get("__max_tool_calls")
                .and_then(Value::as_u64)
                .unwrap_or(200) as u32;
            (s, "mock".to_string(), max_turns, max_tool_calls)
        } else {
            match build_env_provider().await {
                Some(tuple) => tuple,
                None => {
                    return Err(anyhow!(
                        "submit: no provider configured (set NVIDIA_API_KEY or pass __model)"
                    ))
                }
            }
        };

    let writer = SessionWriter::open(&path).await?;
    let path_buf = PathBuf::from(&path);

    let cancel = CancellationToken::new();
    let (ask_tx, ask_rx) = mpsc::channel::<AskRequest>(16);
    let cfg = AgentConfig::new(max_turns, max_tool_calls, model, canonical_root, writer)
        .with_ask_resolver(ask_tx.clone());

    // ChannelSink forwards every AgentEvent; `resume_into` rebuilds
    // history from the log and ADOPTS the persisted session_id, so the
    // streamed ids match what the client already holds.
    let (sink, sink_rx) = ChannelSink::new(CHANNEL_CAP);
    let (mut agent, _history) = Agent::resume_into(
        cfg,
        provider,
        default_registry(),
        cancel.clone(),
        Arc::new(sink),
        &path_buf,
    )
    .await
    .map_err(|e| anyhow!("agent: {e}"))?;

    let run_id = agent.run_id().0.to_string();

    // Stream the ack FIRST so it precedes any event notification.
    let ack = json!({ "run_id": run_id, "session_id": session_id });
    let _ = out.send(make_ok_response(id, ack)).await;

    // Register the active run before the agent can emit anything.
    let pending_asks: Arc<Mutex<HashMap<String, oneshot::Sender<AskResponse>>>> =
        Arc::new(Mutex::new(HashMap::new()));
    {
        let mut s = sessions.lock().await;
        s.insert(
            session_id.clone(),
            ActiveSession {
                cancel: cancel.clone(),
                ask_resolver: ask_tx.clone(),
                pending_asks: Arc::clone(&pending_asks),
            },
        );
    }

    // Shared terminal-seen flag + forwarder-done signal. The
    // forwarder flips `terminal_seen` when it pushes a terminal
    // `AgentEvent`, and signals `forwarder_done` when its drain task
    // exits (channel closed when the agent drops). The run task waits
    // for the forwarder to finish before deciding whether to
    // synthesize a backstop terminal — otherwise we race and emit
    // duplicates.
    let terminal_seen = Arc::new(AtomicBool::new(false));
    let forwarder_done = Arc::new(tokio::sync::Notify::new());

    // Event forwarder: AgentEvent → `event` notification.
    {
        let out = out.clone();
        let sid = session_id.clone();
        let rid = run_id.clone();
        let terminal_seen = Arc::clone(&terminal_seen);
        let forwarder_done = Arc::clone(&forwarder_done);
        let mut rx = sink_rx;
        tokio::spawn(async move {
            let mut seq: u64 = 0;
            while let Some(event) = rx.recv().await {
                let is_terminal = matches!(
                    event,
                    AgentEvent::RunFinished { .. }
                        | AgentEvent::RunCancelled
                        | AgentEvent::RunFailed { .. }
                );
                let note = json!({
                    "jsonrpc": "2.0",
                    "method": "event",
                    "params": { "session_id": sid, "run_id": rid, "seq": seq, "event": event }
                });
                seq += 1;
                if out.send(note).await.is_err() {
                    break;
                }
                if is_terminal {
                    terminal_seen.store(true, Ordering::Release);
                }
            }
            forwarder_done.notify_one();
        });
    }

    // Ask forwarder: AskRequest → `ask` notification, stashing the
    // oneshot responder for `ask_resolve` to fire.
    {
        let out = out.clone();
        let pending = Arc::clone(&pending_asks);
        let mut rx = ask_rx;
        tokio::spawn(async move {
            while let Some(req) = rx.recv().await {
                let AskRequest {
                    ask_id,
                    call,
                    response,
                } = req;
                pending.lock().await.insert(ask_id.clone(), response);
                let note = json!({
                    "jsonrpc": "2.0",
                    "method": "ask",
                    "params": {
                        "ask_id": ask_id,
                        "call": {
                            "call_id": call.call_id.0.to_string(),
                            "name": call.name,
                            "args": call.args,
                        }
                    }
                });
                if out.send(note).await.is_err() {
                    break;
                }
            }
        });
    }

    // The run itself. On completion the session is de-registered;
    // dropping the agent closes the event sink and the ask resolver,
    // which ends both forwarder tasks. The backstop below guarantees
    // the client sees a terminal event even if the forwarder dropped
    // one.
    {
        let sessions = Arc::clone(sessions);
        let out = out.clone();
        let session_id = session_id.clone();
        let run_id = run_id.clone();
        let terminal_seen = Arc::clone(&terminal_seen);
        let forwarder_done = Arc::clone(&forwarder_done);
        tokio::spawn(async move {
            let user_msg = Message {
                id: MessageId(crate::ids::new_id()),
                role: Role::User,
                parts: vec![Part::Text { text: user_message }],
            };
            let result = agent.submit(user_msg).await;
            sessions.lock().await.remove(&session_id);
            // Wait for the event forwarder to fully drain (it ends
            // when the agent is dropped and the sink channel closes).
            // This eliminates the race where the run task observes
            // `terminal_seen == false` just before the forwarder
            // pushes the agent's real terminal event.
            forwarder_done.notified().await;
            if !terminal_seen.load(Ordering::Acquire) {
                let event = match &result {
                    Ok(AgentEvent::RunFinished { message }) => json!({
                        "type": "RunFinished",
                        "message": message,
                    }),
                    Ok(other) => json!({
                        "type": "RunFailed",
                        "code": "internal",
                        "retryable": false,
                        "message": format!("unexpected non-terminal return: {other:?}"),
                    }),
                    Err(e) => json!({
                        "type": "RunFailed",
                        "code": "internal",
                        "retryable": false,
                        "message": e.to_string(),
                    }),
                };
                let note = json!({
                    "jsonrpc": "2.0",
                    "method": "event",
                    "params": {
                        "session_id": session_id,
                        "run_id": run_id,
                        "seq": u64::MAX,
                        "event": event,
                    }
                });
                let _ = out.send(note).await;
                terminal_seen.store(true, Ordering::Release);
            }
        });
    }

    Ok(())
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

async fn handle_ask_resolve(
    sessions: &Arc<Mutex<HashMap<String, ActiveSession>>>,
    params: Value,
) -> Result<Value> {
    let session_id = params
        .get("session_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("ask_resolve: session_id required"))?;
    let ask_id = params
        .get("ask_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("ask_resolve: ask_id required"))?;
    let decision = params
        .get("decision")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("ask_resolve: decision required"))?;
    let response = match decision {
        "allow" => AskResponse::Allow,
        "deny" => AskResponse::Deny,
        other => return Err(anyhow!("ask_resolve: unknown decision {other:?}")),
    };

    let s = sessions.lock().await;
    let Some(active) = s.get(session_id) else {
        return Ok(json!({ "resolved": false }));
    };
    let sender = active.pending_asks.lock().await.remove(ask_id);
    match sender {
        Some(tx) => {
            // A closed receiver means the run already moved on; treat
            // as unresolved rather than an error.
            let resolved = tx.send(response).is_ok();
            Ok(json!({ "resolved": resolved }))
        }
        None => Ok(json!({ "resolved": false })),
    }
}

async fn handle_policy_set(_params: Value) -> Result<Value> {
    // v0: accepted and echoed so clients can keep state in sync;
    // runtime policy switching is not yet wired. Approval is fixed to
    // `DefaultPolicy` (`read` auto-allows; `write`/`edit`/`bash`
    // produce an `ask` notification; unknown tools are denied).
    Ok(json!({ "ok": true }))
}

/// Convenience: load a [`Config`] from the environment so the
/// app-server can wire a real provider. Public so tests can exercise
/// the same path.
pub async fn load_config() -> Result<Config> {
    Config::load(ConfigOverrides::default())
        .await
        .map_err(|e| anyhow!("config: {e}"))
}
