### Task 5.4 — Session sidebar

**Files:**
- New: `crates/crow-desktop/src/frontend/components/sidebar.ts` (sidebar custom element)
- Modify: `crates/crow-desktop/src/frontend/main.ts` (render the sidebar when a project is selected)
- Modify: `crates/crow-desktop/src/lib.rs` (Tauri commands: `session_list`, `session_load` — bridge to `crow serve`)
- New: `crates/crow-desktop/src/spawn.rs` (spawn `crow serve` as a child process when the app starts)

**Why this exists:** once a project is selected, the user sees the list of past sessions. Clicking a session loads it. The "New chat" button starts a new session.

**Spec references:** v0 spec §15 (CLI — `crow sessions` is the source of truth for this list), §18 (acceptance criterion 6: "Closing and reopening the program resumes completed conversation history").

**Interfaces (exact):**

```rust
// crates/crow-desktop/src/spawn.rs
use std::process::Stdio;
use tokio::process::{Child, Command};

pub struct CrowServer {
    pub child: Child,
    pub port: u16,
}

pub async fn spawn_crow_serve(project_root: &Path) -> Result<CrowServer, SpawnError> {
    // Spawn `crow serve --port 0` and wait for the server to print its port.
    // Connect a JSON-RPC client.
}

pub async fn stop(mut self) -> Result<(), SpawnError>;
```

```rust
// In src/lib.rs, Tauri commands (talk to the spawned crow serve)
#[tauri::command]
async fn session_list(project_root: PathBuf) -> Result<Vec<SessionMeta>, String>;

#[tauri::command]
async fn session_load(session_id: SessionId) -> Result<SessionReplay, String>;

#[tauri::command]
async fn session_new(project_root: PathBuf) -> Result<SessionId, String>;
```

**Frontend (`sidebar.ts`):**

```typescript
class CrowSidebar extends HTMLElement {
  private projectRoot: string | null = null;
  private sessions: SessionMeta[] = [];

  connectedCallback() {
    this.render();
  }

  setProject(path: string) { /* call session_list, render */ }

  private render() {
    this.innerHTML = `
      <button class="new-chat">+ New chat</button>
      <ul class="sessions">
        ${this.sessions.map(s => `
          <li data-id="${s.id}">
            <div class="title">${s.first_user_message ?? '(empty)'}</div>
            <div class="meta">${s.message_count} messages · ${formatRelative(s.last_opened)}</div>
          </li>
        `).join('')}
      </ul>
    `;
  }
}
customElements.define('crow-sidebar', CrowSidebar);
```

**Procedure:**
1. Implement `spawn.rs` to spawn `crow serve --port 0` (or fall back to a Unix socket). Wait for the server to print its port. Return the `Child` + the port.
2. Tauri command `session_list` opens a JSON-RPC connection to the spawned server, sends `SessionList`, returns the metadata.
3. Tauri command `session_load` sends `SessionLoad`, returns the `SessionReplay`.
4. Tauri command `session_new` sends `SessionStart`, returns the `SessionId`.
5. Frontend: a `<crow-sidebar>` custom element. When `setProject(path)` is called, it calls the Tauri command and renders.
6. On click, dispatches a `crow://session_selected` event with the session ID.
7. Tests: a Playwright-style screenshot test for the sidebar (defer to wave 7 if too complex).

**Acceptance:**
- Manual test: launch app, select a project → sidebar shows the past sessions.
- Manual test: click "New chat" → a new session appears at the top of the list and is auto-selected.
- Manual test: click a past session → chat pane shows the event history.
- `cargo build --workspace` is clean.
- The `crow serve` child process is killed when the app exits.

**Forbidden:**
- No `unwrap`/`expect` in library code.
- No blocking IO in the Tauri command handler (use `tokio`).
- No running `crow serve` outside the spawn lifecycle (it dies with the app).

**Dependency:** `tokio` (process), `crow::server` (from wave 4). `tauri-plugin-dialog` for `pick_project`.
