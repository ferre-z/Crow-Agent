### Task 5.1 — Tauri scaffold

**Files:**
- New: `crates/crow-desktop/Cargo.toml` (workspace member)
- New: `crates/crow-desktop/src/main.rs`
- New: `crates/crow-desktop/src/lib.rs`
- New: `crates/crow-desktop/tauri.conf.json`
- New: `crates/crow-desktop/src/frontend/index.html`
- New: `crates/crow-desktop/src/frontend/main.ts`
- New: `crates/crow-desktop/src/frontend/styles.css`
- Modify: `Cargo.toml` (workspace declaration with `members = [".", "crates/crow-desktop"]`)

**Why this exists:** the desktop app is its own binary (`crow-desktop`), built with Tauri 2, talking to `crow serve` over a local socket. This task scaffolds the project so the next 7 tasks can build the actual UI and IPC.

**Spec references:** v0 spec §19 (deferred extension seams — "server/phone clients consume a future protocol derived from `AgentEvent` and commands"). Wave 4 builds that protocol; wave 5 builds the desktop client.

**Tauri 2 setup:**
- `crates/crow-desktop/Cargo.toml` declares `tauri = "2"` and `tauri-build = "2"`.
- `tauri.conf.json` declares the window, the icon, the bundle config (mac/win/linux).
- `src/main.rs` is a thin wrapper that calls `crow_desktop::run()`.
- `src/lib.rs` is the Tauri app: `pub fn run() { tauri::Builder::default()...run() }`.
- `src/frontend/` is a small static frontend: index.html + a tiny `main.ts` + a styles.css.

**Frontend stack:**
- Vanilla TypeScript, no React/Vue/Svelte. ~50KB compiled.
- A small `Component` base class for custom elements.
- Tauri IPC for backend communication.

**Procedure:**
1. Add `crates/crow-desktop/Cargo.toml` with workspace-inherited deps + tauri/tauri-build.
2. Add `tauri.conf.json` with the right bundle config (we'll refine in 5.8).
3. Write `src/main.rs` (calls `crow_desktop::run()`).
4. Write `src/lib.rs` (the Tauri app skeleton with a single empty window).
5. Write the frontend: a "Hello, Crow" page with the project picker placeholder.
6. Add `tauri-build` to `[build-dependencies]` so the frontend assets get baked into the binary at compile time.
7. Run `cargo tauri dev` — should open a window.
8. Commit.

**Acceptance:**
- `cargo tauri dev` opens a window.
- The window shows "Crow" and a placeholder project picker.
- `cargo tauri build` produces a binary (we don't need the full installer for this task).
- `cargo build --workspace` is clean.

**Forbidden:**
- No React/Vue/Svelte/Preact. Vanilla TS only.
- No `tauri-plugin-*` other than the bare minimum (we add plugins in later tasks).
- No production build configuration in this task (5.8).

**Dependency:** add `tauri = "2"`, `tauri-build = "2"`, `serde = "1"`, `serde_json = "1"`, `tokio = "1"` (with relevant features) to `crates/crow-desktop/Cargo.toml`. No new top-level deps.
