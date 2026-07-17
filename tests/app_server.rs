//! Phase-6 integration tests for `crow serve`.
//!
//! The app-server reads JSON-RPC requests from stdin and writes
//! responses/notifications to stdout. We exercise the protocol handler
//! directly (`crow::app_server::handle_request`) so the tests don't
//! need to fork subprocesses. Each handler now takes an outbound
//! channel (the same one the real server drains to stdout); `submit`
//! streams its ack and events through it, so streaming tests read the
//! channel to observe the live event sequence.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crow::app_server::{handle_request, ActiveSession};
use crow::ids::new_id;
use crow::session;
use serde_json::{json, Value};
use tokio::sync::{mpsc, Mutex};
use tokio_util::sync::CancellationToken;

fn empty_sessions() -> Arc<Mutex<HashMap<String, ActiveSession>>> {
    Arc::new(Mutex::new(HashMap::new()))
}

/// A fresh outbound channel standing in for the server's stdout writer.
fn outbound() -> (mpsc::Sender<Value>, mpsc::Receiver<Value>) {
    mpsc::channel::<Value>(256)
}

/// Receive one outbound line with a generous timeout so a hung run
/// fails the test loudly instead of blocking forever.
async fn recv(rx: &mut mpsc::Receiver<Value>) -> Value {
    tokio::time::timeout(Duration::from_secs(10), rx.recv())
        .await
        .expect("timed out waiting for an outbound line")
        .expect("outbound channel closed unexpectedly")
}

/// Drive `session_start` and return `(session_id, path)`.
async fn start_session(
    sessions: &Arc<Mutex<HashMap<String, ActiveSession>>>,
    out: &mpsc::Sender<Value>,
    project_root: &std::path::Path,
) -> (String, String) {
    let resp = handle_request(
        sessions,
        out,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "session_start",
            "params": { "project_root": project_root.display().to_string() }
        }),
    )
    .await
    .expect("response");
    (
        resp["result"]["session_id"].as_str().unwrap().to_string(),
        resp["result"]["path"].as_str().unwrap().to_string(),
    )
}

/// Drive `session_start` returning `(tempdir, session_id, path,
/// project_root)` so callers can keep the tempdir alive for the
/// duration of the test (its `Drop` deletes the directory, which
/// invalidates the canonicalized paths).
async fn start_session_full(
    sessions: &Arc<Mutex<HashMap<String, ActiveSession>>>,
    out: &mpsc::Sender<Value>,
) -> (tempfile::TempDir, String, String, PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project_root = tmp.path().canonicalize().unwrap();
    let (session_id, path) = start_session(sessions, out, &project_root).await;
    (tmp, session_id, path, project_root)
}

#[tokio::test]
async fn initialize_returns_protocol_version() {
    let sessions = empty_sessions();
    let (out, _rx) = outbound();
    let req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {}
    });
    let resp = handle_request(&sessions, &out, req)
        .await
        .expect("response");
    assert_eq!(resp["id"], 1);
    assert_eq!(
        resp["result"]["protocol_version"],
        crow::app_server::PROTOCOL_VERSION
    );
}

#[tokio::test]
async fn unknown_method_returns_error() {
    let sessions = empty_sessions();
    let (out, _rx) = outbound();
    let req = json!({
        "jsonrpc": "2.0",
        "id": 7,
        "method": "frobnicate",
        "params": {}
    });
    let resp = handle_request(&sessions, &out, req)
        .await
        .expect("response");
    assert_eq!(resp["id"], 7);
    assert!(resp["error"]["message"]
        .as_str()
        .unwrap()
        .contains("frobnicate"));
}

#[tokio::test]
async fn session_start_creates_log_file() {
    let sessions = empty_sessions();
    let (out, _rx) = outbound();
    let tmp = tempfile::tempdir().expect("tempdir");
    let project_root = tmp.path().canonicalize().unwrap();

    let req = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "session_start",
        "params": {
            "project_root": project_root.display().to_string()
        }
    });
    let resp = handle_request(&sessions, &out, req)
        .await
        .expect("response");
    let result = &resp["result"];
    assert!(result["session_id"].is_string());
    let path: PathBuf = result["path"].as_str().unwrap().into();
    assert!(
        path.exists(),
        "session log file must exist after session_start"
    );
    // The file starts with a SessionStarted entry so list_sessions
    // can find it. We assert that.
    let entries = session::read_entries(&path).await.expect("read");
    assert_eq!(entries.len(), 1);
    assert!(matches!(
        entries[0],
        crow::session_entry::SessionEntry::SessionStarted { .. }
    ));
}

#[tokio::test]
async fn session_list_returns_known_sessions() {
    let sessions = empty_sessions();
    let (out, _rx) = outbound();
    let tmp = tempfile::tempdir().expect("tempdir");
    let project_root = tmp.path().canonicalize().unwrap();
    let (id_a, _) = start_session(&sessions, &out, &project_root).await;
    let (id_b, _) = start_session(&sessions, &out, &project_root).await;

    let list = handle_request(
        &sessions,
        &out,
        json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "session_list",
            "params": { "project_root": project_root.display().to_string() }
        }),
    )
    .await
    .unwrap();
    let entries = list["result"]["sessions"].as_array().expect("array");
    assert_eq!(entries.len(), 2);
    let ids: Vec<&str> = entries
        .iter()
        .map(|e| e["session_id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&id_a.as_str()));
    assert!(ids.contains(&id_b.as_str()));
}

#[tokio::test]
async fn session_load_replays_events() {
    let sessions = empty_sessions();
    let (out, mut rx) = outbound();
    let tmp = tempfile::tempdir().expect("tempdir");
    let project_root = tmp.path().canonicalize().unwrap();
    let (session_id, path) = start_session(&sessions, &out, &project_root).await;

    // Submit a text-only script so the run reaches RunFinished and the
    // user message lands in the log.
    let script = json!([
        {"type": "ModelStarted"},
        {"type": "TextDelta", "text": "hi"},
        {"type": "ModelFinished", "usage": {"input_tokens": 1, "output_tokens": 1}, "stop_reason": "EndTurn"}
    ]);
    let submit = handle_request(
        &sessions,
        &out,
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "submit",
            "params": {
                "session_id": session_id,
                "path": path,
                "project_root": project_root.display().to_string(),
                "user_message": "replay me",
                "__script": script
            }
        }),
    )
    .await;
    assert!(submit.is_none(), "submit streams its ack out-of-band");
    // Drain until the run finishes so the log is fully written.
    drain_until_run_end(&mut rx).await;

    let load = handle_request(
        &sessions,
        &out,
        json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "session_load",
            "params": {
                "session_id": session_id,
                "path": path,
                "project_root": project_root.display().to_string()
            }
        }),
    )
    .await
    .unwrap();
    let events = load["result"]["events"].as_array().expect("events array");
    let kinds: Vec<&str> = events.iter().map(|e| e["kind"].as_str().unwrap()).collect();
    assert!(
        kinds.contains(&"user_message"),
        "replay missing user_message event; got: {kinds:?}"
    );
    assert!(
        kinds.contains(&"run_finished"),
        "replay missing run_finished event; got: {kinds:?}"
    );
}

#[tokio::test]
async fn interrupt_unknown_session_reports_not_cancelled() {
    let sessions = empty_sessions();
    let (out, _rx) = outbound();
    let resp = handle_request(
        &sessions,
        &out,
        json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "interrupt",
            "params": { "session_id": "does-not-exist" }
        }),
    )
    .await
    .unwrap();
    assert_eq!(resp["result"]["cancelled"], false);
}

#[tokio::test]
async fn interrupt_cancels_active_session() {
    let sessions = empty_sessions();
    let (out, _rx) = outbound();
    let cancel = CancellationToken::new();
    let (ask_tx, _ask_rx) = tokio::sync::mpsc::channel::<crow::policy::AskRequest>(8);
    let session_key = format!("active-{}", new_id());
    sessions.lock().await.insert(
        session_key.clone(),
        ActiveSession {
            cancel: cancel.clone(),
            ask_resolver: ask_tx,
            pending_asks: Arc::new(Mutex::new(HashMap::new())),
        },
    );
    assert!(!cancel.is_cancelled());

    let resp = handle_request(
        &sessions,
        &out,
        json!({
            "jsonrpc": "2.0",
            "id": 6,
            "method": "interrupt",
            "params": { "session_id": session_key }
        }),
    )
    .await
    .unwrap();
    assert_eq!(resp["result"]["cancelled"], true);
    assert!(cancel.is_cancelled());
}

#[tokio::test]
async fn policy_set_acknowledges() {
    let sessions = empty_sessions();
    let (out, _rx) = outbound();
    let resp = handle_request(
        &sessions,
        &out,
        json!({
            "jsonrpc": "2.0",
            "id": 9,
            "method": "policy_set",
            "params": { "session_id": "x", "policy": "allow_all" }
        }),
    )
    .await
    .unwrap();
    assert_eq!(resp["result"]["ok"], true);
}

#[tokio::test]
async fn session_load_for_unknown_path_surfaces_error() {
    let sessions = empty_sessions();
    let (out, _rx) = outbound();
    let tmp = tempfile::tempdir().expect("tempdir");
    let project_root = tmp.path().canonicalize().unwrap();
    // A valid project_root but a path that doesn't exist on disk.
    let bogus_path = project_root
        .join(".crow")
        .join("sessions")
        .join("missing.jsonl");
    let resp = handle_request(
        &sessions,
        &out,
        json!({
            "jsonrpc": "2.0",
            "id": 8,
            "method": "session_load",
            "params": {
                "session_id": "missing",
                "path": bogus_path.display().to_string(),
                "project_root": project_root.display().to_string()
            }
        }),
    )
    .await
    .unwrap();
    assert!(resp.get("error").is_some());
    let msg = resp["error"]["message"].as_str().unwrap();
    // Path validation canonicalises first; a missing file is
    // surfaced as `canonicalize path …` (I/O) before we ever
    // attempt the read.
    assert!(
        msg.contains("canonicalize") || msg.contains("I/O"),
        "expected canonicalize/I/O error, got: {msg}"
    );
}

#[tokio::test]
async fn submit_without_provider_returns_typed_error() {
    // Neutralise any ambient provider key so this exercises the
    // no-provider path deterministically. Safe here: this is the only
    // test that reaches the env-provider path (the others use
    // `__script`, which bypasses config/env entirely).
    std::env::remove_var("CROW_API_KEY");
    std::env::remove_var("NVIDIA_API_KEY");

    let sessions = empty_sessions();
    let (out, _rx) = outbound();
    let tmp = tempfile::tempdir().expect("tempdir");
    let project_root = tmp.path().canonicalize().unwrap();
    let (session_id, path) = start_session(&sessions, &out, &project_root).await;
    let resp = handle_request(
        &sessions,
        &out,
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "submit",
            "params": {
                "session_id": session_id,
                "path": path,
                "project_root": project_root.display().to_string(),
                "user_message": "hi"
                // no __script / __model → no provider
            }
        }),
    )
    .await
    .expect("submit error is a direct response");
    assert!(resp.get("error").is_some());
    let msg = resp["error"]["message"].as_str().unwrap();
    assert!(
        msg.contains("no provider configured"),
        "expected typed error, got: {msg}"
    );
}

// ---------------------------------------------------------------------------
// Streaming behaviour (the M0 fix): submit pushes ordered events, interrupt
// yields RunCancelled, and a denied ask yields a policy_denied ToolFinished.
// ---------------------------------------------------------------------------

/// Read event `type` tags until a terminal one, returning the sequence.
async fn drain_until_run_end(rx: &mut mpsc::Receiver<Value>) -> Vec<String> {
    let mut tags = Vec::new();
    loop {
        let v = recv(rx).await;
        if v["method"] == "event" {
            let tag = v["params"]["event"]["type"].as_str().unwrap().to_string();
            let terminal = matches!(tag.as_str(), "RunFinished" | "RunCancelled" | "RunFailed");
            tags.push(tag);
            if terminal {
                return tags;
            }
        }
    }
}

#[tokio::test]
async fn submit_streams_ordered_events_and_ack() {
    let sessions = empty_sessions();
    let (out, mut rx) = outbound();
    let tmp = tempfile::tempdir().expect("tempdir");
    let project_root = tmp.path().canonicalize().unwrap();
    let (session_id, path) = start_session(&sessions, &out, &project_root).await;

    let script = json!([
        {"type": "ModelStarted"},
        {"type": "TextDelta", "text": "Hello"},
        {"type": "ModelFinished", "usage": {"input_tokens": 1, "output_tokens": 1}, "stop_reason": "EndTurn"}
    ]);
    let submit = handle_request(
        &sessions,
        &out,
        json!({
            "jsonrpc": "2.0",
            "id": 42,
            "method": "submit",
            "params": {
                "session_id": session_id,
                "path": path,
                "project_root": project_root.display().to_string(),
                "user_message": "hi",
                "__script": script
            }
        }),
    )
    .await;
    if let Some(resp) = &submit {
        panic!("submit returned Some: {resp}");
    }

    // The ack is the first outbound line, correlated by request id.
    let ack = recv(&mut rx).await;
    assert_eq!(ack["id"], 42);
    assert!(ack["result"]["run_id"].as_str().is_some());
    assert_eq!(ack["result"]["session_id"].as_str().unwrap(), session_id);

    // Then the ordered event stream.
    let tags = drain_until_run_end(&mut rx).await;
    assert_eq!(
        tags,
        vec![
            "RunStarted",
            "ModelStarted",
            "TextDelta",
            "ModelFinished",
            "RunFinished"
        ],
        "unexpected event order"
    );
}

#[tokio::test]
async fn submit_envelopes_carry_correlated_ids_and_seq() {
    let sessions = empty_sessions();
    let (out, mut rx) = outbound();
    let tmp = tempfile::tempdir().expect("tempdir");
    let project_root = tmp.path().canonicalize().unwrap();
    let (session_id, path) = start_session(&sessions, &out, &project_root).await;

    let script = json!([
        {"type": "ModelStarted"},
        {"type": "ModelFinished", "usage": {"input_tokens": 1, "output_tokens": 1}, "stop_reason": "EndTurn"}
    ]);
    handle_request(
        &sessions,
        &out,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "submit",
            "params": {
                "session_id": session_id,
                "path": path,
                "project_root": project_root.display().to_string(),
                "user_message": "hi",
                "__script": script
            }
        }),
    )
    .await;

    let ack = recv(&mut rx).await;
    let run_id = ack["result"]["run_id"].as_str().unwrap().to_string();

    let mut expected_seq = 0u64;
    loop {
        let v = recv(&mut rx).await;
        if v["method"] == "event" {
            assert_eq!(v["params"]["session_id"].as_str().unwrap(), session_id);
            assert_eq!(v["params"]["run_id"].as_str().unwrap(), run_id);
            assert_eq!(v["params"]["seq"].as_u64().unwrap(), expected_seq);
            expected_seq += 1;
            if v["params"]["event"]["type"] == "RunFinished" {
                break;
            }
        }
    }
    assert!(expected_seq >= 2, "expected several correlated events");
}

#[tokio::test]
async fn interrupt_mid_run_yields_run_cancelled() {
    let sessions = empty_sessions();
    let (out, mut rx) = outbound();
    let tmp = tempfile::tempdir().expect("tempdir");
    let project_root = tmp.path().canonicalize().unwrap();
    let (session_id, path) = start_session(&sessions, &out, &project_root).await;

    // A bash tool call makes DefaultPolicy ask, blocking the run until
    // we interrupt — a deterministic mid-run window.
    let call_id = new_id().to_string();
    let script = json!([
        {"type": "ModelStarted"},
        {"type": "ToolStarted", "call_id": call_id, "name": "bash", "args": {"command": "ls"}},
        {"type": "ModelFinished", "usage": {"input_tokens": 1, "output_tokens": 1}, "stop_reason": "ToolUse"}
    ]);
    handle_request(
        &sessions,
        &out,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "submit",
            "params": {
                "session_id": session_id,
                "path": path,
                "project_root": project_root.display().to_string(),
                "user_message": "run ls",
                "__script": script
            }
        }),
    )
    .await;

    let _ack = recv(&mut rx).await;

    // Read until the ask notification arrives (run is now parked).
    loop {
        let v = recv(&mut rx).await;
        if v["method"] == "ask" {
            break;
        }
    }

    // Interrupt while parked.
    let resp = handle_request(
        &sessions,
        &out,
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "interrupt",
            "params": { "session_id": session_id }
        }),
    )
    .await
    .unwrap();
    assert_eq!(resp["result"]["cancelled"], true);

    // The next terminal event must be RunCancelled.
    loop {
        let v = recv(&mut rx).await;
        if v["method"] == "event" {
            let tag = v["params"]["event"]["type"].as_str().unwrap();
            if matches!(tag, "RunFinished" | "RunCancelled" | "RunFailed") {
                assert_eq!(tag, "RunCancelled", "expected cancellation to win");
                return;
            }
        }
    }
}

#[tokio::test]
async fn ask_resolve_deny_yields_policy_denied_tool_finished() {
    let sessions = empty_sessions();
    let (out, mut rx) = outbound();
    let tmp = tempfile::tempdir().expect("tempdir");
    let project_root = tmp.path().canonicalize().unwrap();
    let (session_id, path) = start_session(&sessions, &out, &project_root).await;

    let call_id = new_id().to_string();
    let script = json!([
        {"type": "ModelStarted"},
        {"type": "ToolStarted", "call_id": call_id, "name": "bash", "args": {"command": "ls"}},
        {"type": "ModelFinished", "usage": {"input_tokens": 1, "output_tokens": 1}, "stop_reason": "ToolUse"}
    ]);
    // Cap tool calls at 1 so the run terminates after the denied call.
    handle_request(
        &sessions,
        &out,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "submit",
            "params": {
                "session_id": session_id,
                "path": path,
                "project_root": project_root.display().to_string(),
                "user_message": "run ls",
                "__script": script,
                "__max_tool_calls": 1
            }
        }),
    )
    .await;

    let _ack = recv(&mut rx).await;

    // Capture the ask_id from the pushed ask notification.
    let ask_id = loop {
        let v = recv(&mut rx).await;
        if v["method"] == "ask" {
            break v["params"]["ask_id"].as_str().unwrap().to_string();
        }
    };

    // Deny it.
    let resp = handle_request(
        &sessions,
        &out,
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "ask_resolve",
            "params": { "session_id": session_id, "ask_id": ask_id, "decision": "deny" }
        }),
    )
    .await
    .unwrap();
    assert_eq!(resp["result"]["resolved"], true);

    // A ToolFinished carrying an Error{code: "policy_denied"} must arrive.
    loop {
        let v = recv(&mut rx).await;
        if v["method"] == "event" && v["params"]["event"]["type"] == "ToolFinished" {
            let code = &v["params"]["event"]["result"]["Error"]["code"];
            assert_eq!(code.as_str(), Some("policy_denied"), "got event: {v}");
            return;
        }
    }
}

// ---------------------------------------------------------------------------
// R1 regression tests: the GUI must never hang waiting for a terminal event.
// The agent's silent exits (max_tool_calls, empty_stream, …) and the
// app_server's backstop together guarantee exactly one terminal
// `event` reaches the client.
// ---------------------------------------------------------------------------

/// R1 regression: a tool-call cap that fires must end the run with a
/// terminal `RunFailed` carrying `code: "max_tool_calls"`. Before R1
/// the agent's silent exit on this path left the GUI hanging waiting
/// for a terminal event.
#[tokio::test]
async fn submit_with_max_tool_calls_emits_terminal_event() {
    let sessions = empty_sessions();
    let (out, mut rx) = outbound();
    let (_tmp, session_id, path, project_root) = start_session_full(&sessions, &out).await;

    // One tool call + cap of 0 → the `max_tool_calls` check fires on
    // turn 1 before any tool actually executes. Before R1 the stream
    // went silent; now a terminal `RunFailed` must arrive.
    let call_id = new_id().to_string();
    let script = json!([
        {"type": "ModelStarted"},
        {"type": "ToolStarted", "call_id": call_id, "name": "bash", "args": {"command": "ls"}},
        {"type": "ModelFinished", "usage": {"input_tokens": 1, "output_tokens": 1}, "stop_reason": "ToolUse"}
    ]);
    handle_request(
        &sessions,
        &out,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "submit",
            "params": {
                "session_id": session_id,
                "path": path,
                "project_root": project_root.display().to_string(),
                "user_message": "run ls",
                "__script": script,
                "__max_tool_calls": 0
            }
        }),
    )
    .await;

    let _ack = recv(&mut rx).await;
    // Wait for the first terminal event; the tool flow (Ask or deny)
    // may or may not be exercised before max_tool_calls fires.
    let terminal = wait_for_terminal(&mut rx).await;
    assert_eq!(
        terminal["params"]["event"]["type"], "RunFailed",
        "expected terminal RunFailed from max_tool_calls; got: {terminal}"
    );
    assert_eq!(
        terminal["params"]["event"]["code"], "max_tool_calls",
        "expected code max_tool_calls; got: {terminal}"
    );
}

/// R1 regression (empty_stream path): a scripted provider that yields
/// no events at all must still end in a terminal `RunFailed` with
/// `code: "empty_stream"`.
#[tokio::test]
async fn submit_with_empty_stream_emits_terminal_event() {
    let sessions = empty_sessions();
    let (out, mut rx) = outbound();
    let (_tmp, session_id, path, project_root) = start_session_full(&sessions, &out).await;

    // A ModelStarted followed by a stream-end is the closest we can
    // get to a no-event script: the agent consumes ModelStarted
    // (sets `last_event_seen = true`) and then `None` is treated as
    // a normal stream end, not `empty_stream`. So we don't emit
    // anything beyond ModelStarted → ModelFinished → RunFinished.
    // This test instead exercises a script whose stream ends
    // *without* a ModelFinished — the agent's stream loop's `None`
    // arm runs and emits `empty_stream` only if `!last_event_seen`.
    // We can force that by giving the script no events at all.
    let resp = handle_request(
        &sessions,
        &out,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "submit",
            "params": {
                "session_id": session_id,
                "path": path,
                "project_root": project_root.display().to_string(),
                "user_message": "hi",
                "__script": []
            }
        }),
    )
    .await;
    // Even if the run errors before the ack can be sent, we
    // expect a direct response; if the run starts, we expect None.
    if let Some(r) = resp {
        let msg = r["error"]["message"].as_str().unwrap_or("");
        assert!(
            msg.contains("__script") || msg.contains("agent"),
            "unexpected direct error: {r}"
        );
        return;
    }
    let _ack = recv(&mut rx).await;
    let terminal = wait_for_terminal(&mut rx).await;
    assert_eq!(terminal["params"]["event"]["type"], "RunFailed");
}

/// R4 — `submit` rejects a `path` that doesn't match
/// `<project_root>/.crow/sessions/<session_id>.jsonl`.
#[tokio::test]
async fn submit_rejects_path_outside_project_root() {
    let sessions = empty_sessions();
    let (out, _rx) = outbound();
    let tmp = tempfile::tempdir().expect("tempdir");
    let project_root = tmp.path().canonicalize().unwrap();
    let (session_id, _path) = start_session(&sessions, &out, &project_root).await;

    // Forge a path under a different root entirely.
    let other = tempfile::tempdir().expect("tempdir");
    let forged = other.path().join("not-the-real-session.jsonl");
    std::fs::write(&forged, "").expect("seed forged file");

    let resp = handle_request(
        &sessions,
        &out,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "submit",
            "params": {
                "session_id": session_id,
                "path": forged.display().to_string(),
                "project_root": project_root.display().to_string(),
                "user_message": "hi",
                "__model": "mock"
            }
        }),
    )
    .await
    .expect("typed error before any work begins");
    assert!(resp.get("error").is_some());
    let msg = resp["error"]["message"].as_str().unwrap();
    assert!(
        msg.contains("path mismatch"),
        "expected path-mismatch error, got: {msg}"
    );
}

/// R4 — `session_load` rejects the same kind of path mismatch.
#[tokio::test]
async fn session_load_rejects_path_outside_project_root() {
    let sessions = empty_sessions();
    let (out, _rx) = outbound();
    let tmp = tempfile::tempdir().expect("tempdir");
    let project_root = tmp.path().canonicalize().unwrap();
    let (session_id, _path) = start_session(&sessions, &out, &project_root).await;

    let other = tempfile::tempdir().expect("tempdir");
    let forged = other.path().join("not-the-real-session.jsonl");
    std::fs::write(&forged, "").expect("seed forged file");

    let resp = handle_request(
        &sessions,
        &out,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "session_load",
            "params": {
                "session_id": session_id,
                "path": forged.display().to_string(),
                "project_root": project_root.display().to_string()
            }
        }),
    )
    .await
    .expect("typed error before any work begins");
    assert!(resp.get("error").is_some());
    let msg = resp["error"]["message"].as_str().unwrap();
    assert!(
        msg.contains("path mismatch"),
        "expected path-mismatch error, got: {msg}"
    );
}

/// Helpers used by the regression tests above.
async fn wait_for_terminal(rx: &mut mpsc::Receiver<Value>) -> Value {
    loop {
        let v = recv(rx).await;
        if v["method"] == "event" {
            let tag = v["params"]["event"]["type"].as_str().unwrap();
            if matches!(tag, "RunFinished" | "RunCancelled" | "RunFailed") {
                return v;
            }
        }
    }
}
