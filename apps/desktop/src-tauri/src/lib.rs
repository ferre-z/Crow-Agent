//! Crow desktop shell — Tauri 2 backend.
//!
//! This process owns the `crow serve` **sidecar** and bridges its
//! line-delimited JSON-RPC stdio to the webview:
//!
//! - On startup it spawns `crow serve`, then a reader task line-buffers
//!   the sidecar's stdout, parses each JSON value, and routes it:
//!     * id-correlated `result`/`error` → resolves the matching pending
//!       request (see [`call`]).
//!     * `event` / `ask` notifications → forwarded to the webview via
//!       the [`tauri::ipc::Channel`] registered by the `connect` command.
//!     * `ready` → marks the sidecar connected.
//! - Tauri commands (`session_start`, `session_list`, `session_load`,
//!   `submit`, `interrupt`, `ask_resolve`) each write one JSON-RPC line
//!   to the sidecar and await the correlated reply.
//!
//! Wire contract: `apps/desktop/src/ipc/contract.md`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use tauri::ipc::Channel;
use tauri::Manager;
use tauri_plugin_shell::process::CommandEvent;
use tauri_plugin_shell::ShellExt;

/// Shared, cloneable application state. Registered with Tauri via
/// `manage` and also cloned into the sidecar reader task.
type AppState = Arc<Inner>;

#[derive(Default)]
struct Inner {
    /// The sidecar's stdin handle. `write` appends one JSON line.
    child: Mutex<Option<tauri_plugin_shell::process::CommandChild>>,
    /// Monotonic JSON-RPC request id source.
    next_id: AtomicU64,
    /// Pending request responders keyed by request id. The reader task
    /// removes and fires the matching sender when a reply arrives.
    pending: Mutex<HashMap<u64, tauri::async_runtime::Sender<Value>>>,
    /// The webview channel for pushed `event`/`ask` notifications,
    /// registered by the `connect` command.
    sink: Mutex<Option<Channel<Value>>>,
    /// Set once the sidecar emits its `ready` banner.
    ready: Mutex<bool>,
}

/// Write one JSON-RPC request line to the sidecar's stdin. Synchronous
/// because `CommandChild::write` is synchronous.
fn write_line(state: &AppState, value: &Value) -> Result<(), String> {
    let mut line = serde_json::to_string(value).map_err(|e| e.to_string())?;
    line.push('\n');
    let mut guard = state.child.lock().map_err(|_| "state poisoned")?;
    let child = guard.as_mut().ok_or("sidecar not started")?;
    child.write(line.as_bytes()).map_err(|e| e.to_string())
}

/// Send a JSON-RPC request and await the correlated response. Returns
/// the `result` value, or an `Err` carrying the server's error message.
async fn call(state: &AppState, method: &str, params: Value) -> Result<Value, String> {
    let id = state.next_id.fetch_add(1, Ordering::Relaxed);
    let (tx, mut rx) = tauri::async_runtime::channel::<Value>(1);
    {
        let mut pending = state.pending.lock().map_err(|_| "state poisoned")?;
        pending.insert(id, tx);
    }
    let request = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
    if let Err(e) = write_line(state, &request) {
        state.pending.lock().ok().and_then(|mut p| p.remove(&id));
        return Err(e);
    }
    let response = rx
        .recv()
        .await
        .ok_or_else(|| "sidecar closed before responding".to_string())?;
    if let Some(err) = response.get("error") {
        let msg = err
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("unknown error");
        return Err(msg.to_string());
    }
    Ok(response.get("result").cloned().unwrap_or(Value::Null))
}

/// Route one parsed line from the sidecar: either a correlated response
/// or a pushed notification.
async fn route(state: &AppState, value: Value) {
    let has_id = value.get("id").is_some_and(|v| !v.is_null());
    let is_response = value.get("result").is_some() || value.get("error").is_some();
    if has_id && is_response {
        if let Some(id) = value.get("id").and_then(Value::as_u64) {
            let responder = state
                .pending
                .lock()
                .ok()
                .and_then(|mut pending| pending.remove(&id));
            if let Some(tx) = responder {
                let _ = tx.send(value).await;
            }
            return;
        }
    }
    match value.get("method").and_then(Value::as_str).unwrap_or("") {
        // Forward the whole notification object; the frontend switches
        // on `.method` ("event" | "ask").
        "event" | "ask" => {
            let channel = state.sink.lock().ok().and_then(|guard| guard.clone());
            if let Some(channel) = channel {
                let _ = channel.send(value);
            }
        }
        "ready" => {
            if let Ok(mut ready) = state.ready.lock() {
                *ready = true;
            }
        }
        _ => {}
    }
}

/// Reader task: line-buffer the sidecar stdout and route each JSON value.
async fn read_sidecar(
    state: AppState,
    mut rx: tauri::async_runtime::Receiver<CommandEvent>,
) {
    let mut buffer: Vec<u8> = Vec::new();
    while let Some(event) = rx.recv().await {
        match event {
            CommandEvent::Stdout(bytes) => {
                buffer.extend_from_slice(&bytes);
                while let Some(pos) = buffer.iter().position(|b| *b == b'\n') {
                    let line: Vec<u8> = buffer.drain(..=pos).collect();
                    let trimmed = &line[..line.len().saturating_sub(1)];
                    if trimmed.is_empty() {
                        continue;
                    }
                    if let Ok(value) = serde_json::from_slice::<Value>(trimmed) {
                        route(&state, value).await;
                    }
                }
            }
            CommandEvent::Stderr(_) => { /* sidecar logs; ignore */ }
            CommandEvent::Terminated(_) => break,
            _ => {}
        }
    }
}

// --- Commands ---------------------------------------------------------------

/// Register the webview channel that receives pushed `event`/`ask`
/// notifications. Call once on app load. Returns `true` if the
/// sidecar is already connected.
#[tauri::command]
async fn connect(
    state: tauri::State<'_, AppState>,
    on_event: Channel<Value>,
) -> Result<bool, String> {
    {
        let mut sink = state.sink.lock().map_err(|_| "state poisoned")?;
        *sink = Some(on_event);
    }
    let ready = state.ready.lock().map(|g| *g).unwrap_or(false);
    Ok(ready)
}

#[tauri::command]
async fn session_start(
    state: tauri::State<'_, AppState>,
    project_root: String,
) -> Result<Value, String> {
    call(&state, "session_start", json!({ "project_root": project_root })).await
}

#[tauri::command]
async fn session_list(
    state: tauri::State<'_, AppState>,
    project_root: String,
) -> Result<Value, String> {
    call(&state, "session_list", json!({ "project_root": project_root })).await
}

#[tauri::command]
async fn session_load(
    state: tauri::State<'_, AppState>,
    session_id: String,
    path: String,
    project_root: String,
) -> Result<Value, String> {
    call(
        &state,
        "session_load",
        json!({ "session_id": session_id, "path": path, "project_root": project_root }),
    )
    .await
}

#[tauri::command]
async fn submit(
    state: tauri::State<'_, AppState>,
    session_id: String,
    path: String,
    project_root: String,
    user_message: String,
) -> Result<Value, String> {
    call(
        &state,
        "submit",
        json!({
            "session_id": session_id,
            "path": path,
            "project_root": project_root,
            "user_message": user_message,
        }),
    )
    .await
}

#[tauri::command]
async fn interrupt(
    state: tauri::State<'_, AppState>,
    session_id: String,
) -> Result<Value, String> {
    call(&state, "interrupt", json!({ "session_id": session_id })).await
}

#[tauri::command]
async fn ask_resolve(
    state: tauri::State<'_, AppState>,
    session_id: String,
    ask_id: String,
    decision: String,
) -> Result<Value, String> {
    call(
        &state,
        "ask_resolve",
        json!({ "session_id": session_id, "ask_id": ask_id, "decision": decision }),
    )
    .await
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            let state: AppState = Arc::new(Inner::default());
            app.manage(Arc::clone(&state));

            // Spawn `crow serve` as a sidecar and start the reader task.
            let (rx, child) = app
                .handle()
                .shell()
                .sidecar("crow")?
                .args(["serve"])
                .spawn()?;
            {
                let mut guard = state.child.lock().expect("state poisoned");
                *guard = Some(child);
            }
            tauri::async_runtime::spawn(read_sidecar(Arc::clone(&state), rx));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            connect,
            session_start,
            session_list,
            session_load,
            submit,
            interrupt,
            ask_resolve,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
