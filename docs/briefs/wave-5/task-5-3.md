### Task 5.3 — Project picker

**Files:**
- New: `crates/crow-desktop/src/recent.rs` (read/write `~/.local/share/crow/recent.toml`)
- Modify: `crates/crow-desktop/src/frontend/index.html` (replace "Hello" with project picker)
- Modify: `crates/crow-desktop/src/frontend/main.ts` (render the project list, handle clicks)
- Modify: `crates/crow-desktop/src/lib.rs` (Tauri commands: `recent_list`, `recent_add`, `pick_project`)

**Why this exists:** the desktop app groups sessions by project. The project picker is the first screen the user sees. The recent list persists across app restarts.

**Interfaces (exact):**

```rust
// crates/crow-desktop/src/recent.rs
use std::path::PathBuf;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RecentList {
    pub projects: Vec<RecentProject>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentProject {
    pub path: PathBuf,
    pub display_name: String,
    pub last_opened: chrono::DateTime<chrono::Utc>,
}

pub fn load_recent() -> RecentList;
pub fn save_recent(list: &RecentList) -> Result<(), RecentError>;
pub fn touch(path: &Path, display_name: &str) -> Result<RecentList, RecentError>;
```

```rust
// In src/lib.rs, Tauri commands
#[tauri::command]
async fn recent_list() -> Result<RecentList, String>;

#[tauri::command]
async fn recent_add(path: PathBuf, display_name: String) -> Result<RecentList, String>;

#[tauri::command]
async fn pick_project() -> Result<Option<PathBuf>, String>;
```

**Procedure:**
1. Add `chrono` and `dirs` to `Cargo.toml` (already in for wave 4 task 4.5).
2. Implement `recent.rs` with the file format `~/.local/share/crow/recent.toml`:
   ```toml
   [[project]]
   path = "/Users/ferre/crow"
   display_name = "crow"
   last_opened = "2026-07-14T09:00:00Z"
   ```
3. Tauri commands: `recent_list`, `recent_add`, `pick_project` (uses `tauri-plugin-dialog` to show an OS folder picker).
4. Frontend: render the recent list. Each item is a clickable card showing the display_name + relative path + "last opened X ago". Top of the list has a "Pick another project..." button.
5. On click: call `recent_add` to update the timestamp, then dispatch a `crow://project_selected` event to switch the app into session-picker mode.
6. Frontend test: the list renders, the buttons work.

**Acceptance:**
- Manual test: launch app → see the project picker with no projects.
- Manual test: click "Pick another project..." → OS folder picker → select a folder → that folder appears in the list.
- Manual test: click an existing project → dispatches `crow://project_selected`.
- `cargo build --workspace` is clean.

**Forbidden:**
- No `unwrap`/`expect` in library code.
- No synchronous file IO in Tauri command handlers — use `tokio::fs`.
- No git operations in this task (we add git info in 5.5).

**Dependency:** `chrono` (for timestamps), `dirs` (for `~/.local/share/crow`).
