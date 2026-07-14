### Task 5.2 — Window chrome

**Files:**
- Modify: `crates/crow-desktop/src/lib.rs` (Tauri builder config)
- Modify: `crates/crow-desktop/tauri.conf.json` (window config)
- Modify: `crates/crow-desktop/src/frontend/main.ts` (menu bar / tray)
- New: `crates/crow-desktop/src/menu.rs` (native menu definitions)

**Why this exists:** the desktop app feels native when it has a real menu bar, dock badge, and global hotkey. Codex nails this; we copy the pattern.

**Spec references:** none direct — UX polish, no spec requirement.

**Features:**
- **Native menu bar.** File: New Chat, Open Project, Recent Projects (submenu), Quit. Edit: standard. View: Toggle Sidebar, Toggle Activity, Reload. Help: About, Open Docs.
- **Global hotkey.** Cmd+Shift+C (mac) / Ctrl+Shift+C (win/linux) opens/focuses the app. Use `tauri-plugin-global-shortcut`.
- **System tray icon** (optional in v0). macOS shows it; Windows shows it; Linux depends on DE. Click → show/hide window. Right-click → menu.
- **Dock badge for running tasks** (macOS only). Show "1" or "2" while tasks run. Clear on idle.
- **Window state persistence.** Last position, last size, last project.

**Interfaces (exact):**

```rust
// crates/crow-desktop/src/menu.rs
use tauri::menu::{Menu, MenuItem, Submenu, PredefinedMenuItem};

pub fn build_app_menu(app: &tauri::AppHandle) -> tauri::Result<Menu<tauri::Wry>>;
pub fn build_tray_menu(app: &tauri::AppHandle) -> tauri::Result<Menu<tauri::Wry>>;
```

```rust
// crates/crow-desktop/src/lib.rs additions
pub fn setup_window_chrome(app: &tauri::App) -> tauri::Result<()> {
    let menu = build_app_menu(&app.handle())?;
    app.set_menu(menu)?;
    app.on_menu_event(|app, event| { /* dispatch to frontend */ });
    Ok(())
}

pub fn set_dock_badge(count: u32) {
    // macOS only. Use tauri::api::dock.
}

pub fn setup_global_shortcut(app: &tauri::App) -> tauri::Result<()> {
    use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut, ShortcutState};
    let shortcut = Shortcut::new(Some(Modifiers::SUPER | Modifiers::SHIFT), Code::KeyC);
    app.global_shortcut().on_shortcut(shortcut, |app, _shortcut, event| {
        if event.state == ShortcutState::Pressed {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }
    })?;
    Ok(())
}
```

**Procedure:**
1. Add `tauri-plugin-global-shortcut` and `tauri-plugin-window-state` to `Cargo.toml`.
2. Build the menu in `menu.rs` using tauri's menu API.
3. Wire the menu event handler to send `crow://menu` events to the frontend.
4. Implement `setup_window_chrome` and call it in the app's `setup` callback.
5. Frontend: listen for `crow://menu` events, route to the right action.
6. Test: in dev, the menu bar appears, the hotkey works.

**Acceptance:**
- Manual test: `cargo tauri dev` → window appears with native menu bar.
- Manual test: Cmd+Shift+C (or Ctrl+Shift+C) toggles the window.
- Manual test: closing and reopening the app restores window position.
- `cargo build --workspace` is clean.

**Forbidden:**
- No custom keymap. Use the OS's native convention (Cmd vs Ctrl).
- No tray icon on Linux without DE detection.
- No production-quality icon assets in v0 (use a simple SVG/PNG placeholder).

**Dependency:** `tauri-plugin-global-shortcut = "2"`, `tauri-plugin-window-state = "2"`.
