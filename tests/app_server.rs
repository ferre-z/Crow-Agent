//! Phase-6 integration tests for `crow serve`.
//!
//! The app-server reads JSON-RPC requests from stdin and writes
//! responses to stdout. We exercise the protocol handler directly
//! (`crow::app_server::handle_request`) so the tests don't need to
//! fork subprocesses — each test drives one request at a time and
//! asserts on the response value.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use crow::app_server::{handle_request, ActiveSession};
use crow::ids::new_id;
use crow::session;
use serde_json::json;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

fn empty_sessions() -> Arc<Mutex<HashMap<String, ActiveSession>>> {
    Arc::new(Mutex::new(HashMap::new()))
}

#[tokio::test]
async fn initialize_returns_protocol_version() {
    let sessions = empty_sessions();
    let req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {}
    });
    let resp = handle_request(&sessions, req).await.expect("response");
    assert_eq!(resp["id"], 1);
    assert_eq!(
        resp["result"]["protocol_version"],
        crow::app_server::PROTOCOL_VERSION
    );
}

#[tokio::test]
async fn unknown_method_returns_error() {
    let sessions = empty_sessions();
    let req = json!({
        "jsonrpc": "2.0",
        "id": 7,
        "method": "frobnicate",
        "params": {}
    });
    let resp = handle_request(&sessions, req).await.expect("response");
    assert_eq!(resp["id"], 7);
    assert!(resp["error"]["message"]
        .as_str()
        .unwrap()
        .contains("frobnicate"));
}

#[tokio::test]
async fn session_start_creates_log_file() {
    let sessions = empty_sessions();
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
    let resp = handle_request(&sessions, req).await.expect("response");
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
    let tmp = tempfile::tempdir().expect("tempdir");
    let project_root = tmp.path().canonicalize().unwrap();
    // Plant two sessions.
    let start_a = handle_request(
        &sessions,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "session_start",
            "params": { "project_root": project_root.display().to_string() }
        }),
    )
    .await
    .unwrap();
    let start_b = handle_request(
        &sessions,
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "session_start",
            "params": { "project_root": project_root.display().to_string() }
        }),
    )
    .await
    .unwrap();

    let list = handle_request(
        &sessions,
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
    // Both ids from the starts must appear.
    let ids: Vec<&str> = entries
        .iter()
        .map(|e| e["session_id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&start_a["result"]["session_id"].as_str().unwrap()));
    assert!(ids.contains(&start_b["result"]["session_id"].as_str().unwrap()));
}

#[tokio::test]
async fn session_load_replays_events() {
    let sessions = empty_sessions();
    let tmp = tempfile::tempdir().expect("tempdir");
    let project_root = tmp.path().canonicalize().unwrap();

    // Start a session.
    let start = handle_request(
        &sessions,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "session_start",
            "params": { "project_root": project_root.display().to_string() }
        }),
    )
    .await
    .unwrap();
    let session_id = start["result"]["session_id"].as_str().unwrap().to_string();
    let path = start["result"]["path"].as_str().unwrap().to_string();

    // Submit a prompt. The scripted mock provider yields empty events
    // → typed failure → typed error from the agent loop, which the
    // app-server surfaces. We don't care about the precise error,
    // only that the session log now contains the user's message.
    let submit = handle_request(
        &sessions,
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "submit",
            "params": {
                "session_id": session_id,
                "path": path,
                "user_message": "replay me",
                "__model": "mock"
            }
        }),
    )
    .await;
    // The submit may error because the scripted mock yields no
    // events; that's fine. The point is that the user's prompt is
    // now in the log.
    let _ = submit;

    let load = handle_request(
        &sessions,
        json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "session_load",
            "params": {
                "session_id": session_id,
                "path": start["result"]["path"].as_str().unwrap()
            }
        }),
    )
    .await
    .unwrap();
    let events = load["result"]["events"].as_array().expect("events array");
    assert!(!events.is_empty(), "replay must return at least one event");
    // The first replayed event should be the user's message.
    let kinds: Vec<&str> = events.iter().map(|e| e["kind"].as_str().unwrap()).collect();
    assert!(
        kinds.contains(&"user_message"),
        "replay missing user_message event; got: {kinds:?}"
    );
}

#[tokio::test]
async fn interrupt_unknown_session_reports_not_cancelled() {
    let sessions = empty_sessions();
    let resp = handle_request(
        &sessions,
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
    let cancel = CancellationToken::new();
    let (ask_tx, _ask_rx) = tokio::sync::mpsc::channel::<crow::policy::AskRequest>(8);
    sessions.lock().await.insert(
        format!("active-{}", new_id()),
        ActiveSession {
            cancel: cancel.clone(),
            ask_resolver: ask_tx,
        },
    );
    assert!(!cancel.is_cancelled());

    let resp = handle_request(
        &sessions,
        json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "interrupt",
            "params": { "session_id": "doesnt-matter" }
        }),
    )
    .await
    .unwrap();
    // The resp just tells us the loop's lookup; the side effect
    // (cancelling a specific session_id) needs us to look up the
    // real id. We iterate the map and assert every active session
    // is cancelled OR use the actual id from the map.
    // For the test we just check the response shape.
    assert!(resp["result"].get("cancelled").is_some());

    // And: trigger a manual cancel to confirm the wired cancel
    // token works (this is what the production handler does when
    // it finds the session).
    let id_from_map = sessions.lock().await.keys().next().cloned().unwrap();
    let _ = handle_request(
        &sessions,
        json!({
            "jsonrpc": "2.0",
            "id": 6,
            "method": "interrupt",
            "params": { "session_id": id_from_map }
        }),
    )
    .await
    .unwrap();
    // Every active session has now been cancelled.
    let s = sessions.lock().await;
    for active in s.values() {
        assert!(active.cancel.is_cancelled());
    }
}

#[tokio::test]
async fn policy_set_acknowledges() {
    // v0 returns `ok: true` without applying anything; the test
    // exists to lock in the API contract.
    let sessions = empty_sessions();
    let resp = handle_request(
        &sessions,
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
    let resp = handle_request(
        &sessions,
        json!({
            "jsonrpc": "2.0",
            "id": 8,
            "method": "session_load",
            "params": {
                "session_id": "x",
                "path": "/nonexistent/path/that/does/not/exist.jsonl"
            }
        }),
    )
    .await
    .unwrap();
    assert!(resp.get("error").is_some());
    assert!(resp["error"]["message"].as_str().unwrap().contains("I/O"));
}

#[tokio::test]
async fn submit_without_provider_returns_typed_error() {
    let sessions = empty_sessions();
    let tmp = tempfile::tempdir().expect("tempdir");
    let project_root = tmp.path().canonicalize().unwrap();
    let start = handle_request(
        &sessions,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "session_start",
            "params": { "project_root": project_root.display().to_string() }
        }),
    )
    .await
    .unwrap();
    let resp = handle_request(
        &sessions,
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "submit",
            "params": {
                "session_id": start["result"]["session_id"],
                "path": start["result"]["path"],
                "user_message": "hi"
                // no __model → no provider
            }
        }),
    )
    .await
    .unwrap();
    assert!(resp.get("error").is_some());
    let msg = resp["error"]["message"].as_str().unwrap();
    assert!(
        msg.contains("no provider configured"),
        "expected typed error, got: {msg}"
    );
}
