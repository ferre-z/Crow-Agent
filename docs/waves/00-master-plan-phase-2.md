---
type: master-plan
status: draft
updated: 2026-07-14
owner: ferre
audience: future Ferre + future Hermes sessions
---

# Crow — Phase 2 (Desktop App) Master Plan

> Phase 1 = the kernel (waves 1-3): provider, agent loop, tools, sessions, recovery.
> Phase 2 = the desktop app that humans actually use.

## Inspiration: what we take from the Codex desktop app

Codex is the current best-in-class agent desktop. We don't copy; we take the *patterns* that work and adapt them to Crow's kernel:

1. **Local app-server, not monolith.** The desktop is a thin client over a local stdio/JSON-RPC service. The same `crow` binary serves headless CLI, the desktop, and a future phone client. Wave 4 builds the service; wave 5-7 build the clients.
2. **Tauri 2 + a small web frontend.** Rust backend, ~50KB of frontend (no React/Vue). HTML + a small custom-element framework or just `<div>` + event handlers. Wave 5.
3. **Project-scoped session list, not a global one.** The desktop groups sessions by project (git root), with a sidebar that shows them newest-first. Same data the CLI `crow sessions` already exposes — just a different view.
4. **Tool-call approval cards.** Every `AgentEvent::ToolStarted` is rendered as a card. The user can allow / deny per call. The kernel stays "autonomous by default" (spec §4) — the desktop *layers* an approval policy on top.
5. **Streaming everything.** Text deltas, reasoning deltas, tool output chunks, and bash stdout/stderr all stream live. The kernel already emits `AgentEvent` deltas; the desktop just renders them.
6. **@-mentions + slash-commands in the composer.** `@src/main.rs` attaches a file; `/compact` runs a slash command; `/model nemotron-3-ultra` switches the provider model.
7. **Activity pane.** A side panel that shows the full event stream (transcript + tool calls + diffs + token usage) without leaving the chat.
8. **Settings + keyring.** API key stored in OS keyring (macOS Keychain, Windows Credential Manager, Linux Secret Service). Theme, telemetry opt-in, default approval policy.
9. **Notifications.** System notifications when a long-running task finishes.
10. **Plan mode.** A toggle in the composer that runs the agent in read-only mode (only `read`, `glob`, `grep` tools) and asks for confirmation before any mutation.

## Wave 4 — App-server + approvals (the protocol layer)

**Why first:** the desktop is one client of many. Building the app-server first means the CLI `crow exec`, the TUI, and the desktop all share one wire format. The TUI and the CLI are then thin re-implementations of the same client (wave 5+).

**Goal:** `crow serve` exposes the same kernel the headless CLI uses, over a JSON-RPC-over-stdio protocol. Plus an approval policy system that the kernel checks before running tool calls.

### Tasks
- **4.1 App-server skeleton.** `crow serve` starts, prints a version banner on stdout, accepts one JSON-RPC request per line on stdin, prints one JSON-RPC response per line on stdout. Logging goes to stderr. Library: `jsonrpsee` (or hand-rolled if simpler — see brief).
- **4.2 Request/response types.** `Initialize`, `SessionStart { project_root }`, `SessionList { project_root }`, `SessionLoad { session_id }`, `Submit { session_id, user_message }`, `Interrupt { session_id }`, `Shutdown`. Response carries a `RunId` + a stream of `AgentEvent`s.
- **4.3 Event streaming.** `Submit` returns 1 `SubmitAck` message immediately, then emits 0..N `AgentEvent` messages. Backpressure: client can slow the server by not reading. Cancellation: `Interrupt` sets the cancel token; no more events after the `RunCancelled` one.
- **4.4 Approval policy.** New `crow::policy` module: an `ApprovalPolicy` trait with `Allow`, `Deny`, `Ask` outcomes. The agent loop checks the policy before each tool execution. Default policy: `Ask` for `bash` and `edit`, `Allow` for `read`. The desktop can override the policy at session start.
- **4.5 Policy persistence.** Per-project policy stored in `~/.config/crow/policy.toml` with rules like `[{tool = "bash", command_starts_with = "rm -rf", decision = "deny"}]`. v0 ships with sensible defaults; users can override.
- **4.6 Replay.** `SessionLoad` returns the full event history of a session. Combined with the session list, this powers the desktop sidebar.
- **4.7 Integration tests.** 8+ tests in `tests/app_server.rs` covering request/response, streaming, cancellation, replay, policy decisions.

### Acceptance
- `crow serve` is a stable, documented binary.
- A python (or any) client can connect, submit a task, and receive the event stream.
- The policy module is hot-pluggable: a new policy can be registered without changing the agent loop.
- All existing wave-1-3 tests still pass.

## Wave 5 — Tauri shell + web frontend

**Why now:** with the app-server stable, the desktop is one client. Tauri gives us native cross-platform packaging for free (Windows + macOS + Linux).

**Goal:** a Tauri 2 desktop app that talks to `crow serve` over a local socket. Single window, project picker, session sidebar, chat pane, composer.

### Tasks
- **5.1 Tauri scaffold.** Add `crates/crow-desktop` with `tauri = "2"`. Frontend: vanilla TS + a tiny custom-element layer (no React). Backend: spawns `crow serve` as a child process, connects via the JSON-RPC socket.
- **5.2 Window chrome.** Native menu bar, Cmd+Shift+C global hotkey (open crow), system tray icon (optional, v0.5), dock badge for running tasks (macOS only).
- **5.3 Project picker.** On startup, show a list of recent projects (read from `~/.local/share/crow/recent.toml`). User picks one, the app loads that project's sessions.
- **5.4 Session sidebar.** New chat button → starts a session, streams the first user message in. Recent sessions show the last user message and timestamp. Click a session to load it.
- **5.5 Chat pane.** Renders the event stream: text deltas as a streaming `<p>`, tool calls as cards (with expand/collapse), bash output as a `<pre>`. Reasoning deltas (if present) render in a collapsible section above the final answer.
- **5.6 Composer.** Multiline `<textarea>` with: Enter to send, Shift+Enter for newline. Send button (also Enter). Slash-command popup (`/compact`, `/model`, `/login`, `/diff`, `/help`). @-mention autocomplete for files (read from project root, fuzzy match).
- **5.7 IPC bridge.** Tauri commands: `connect`, `list_sessions`, `load_session`, `submit`, `interrupt`. These call into the spawned `crow serve` child process. Events from the server arrive via Tauri events (`crow://event`).
- **5.8 Native packaging.** `tauri build` produces `.app` (macOS), `.msi` (Windows), `.deb` + `.AppImage` (Linux). Code-signed on macOS (development cert is fine for v0).

### Acceptance
- `cargo tauri dev` opens a window that talks to `crow serve`.
- New chat, submit, see streaming response, click a tool card to expand.
- Slash commands work: `/model nemotron-3-ultra` switches the model.
- All existing tests still pass; new Tauri app has its own test suite (`cargo tauri test` for Rust, `vitest` for TS).

## Wave 6 — Approvals UX + keyring + image attachments

**Why this wave:** wave 5's chat is fully functional but "autonomous" in the spec sense. Most users want a per-tool approval step. Plus, image attachments (the model can see PNGs) are table stakes for an agent desktop.

### Tasks
- **6.1 Approval card UI.** Every `ToolStarted` event renders as a card. The card shows the tool name, args, and (for `bash`) the command. Buttons: `Allow once`, `Allow for session`, `Deny`. Default timeout: 60s, then auto-deny. The card sits inline in the chat, not as a modal.
- **6.2 Diff preview.** For `write` and `edit`, the card shows a `similar`-powered diff (red/green) before approval. The user can edit the proposed change in a textarea and approve the edited version.
- **6.3 OS keyring for API keys.** Add `keyring = "3"` to Cargo.toml. New CLI subcommand `crow login` prompts for the NVIDIA API key, stores it in the OS keyring, prints success. `crow serve` reads from keyring first, then env var. Tauri frontend also reads from keyring (via IPC).
- **6.4 Image attachments.** Composer gets a `+` button. User picks a PNG/JPEG (max 5MB). Image is base64-encoded and sent as a `User` message with a `Part::Image { data, mime }`. New `Part::Image` variant in `message.rs`. The provider adapter detects this and sends as `image_url` (OpenAI-compatible).
- **6.5 Voice input (optional, defer if too expensive).** Browser `MediaRecorder` API records audio, sends to a Whisper endpoint (or a local whisper.cpp). Transcribed text populates the composer. v0 ships with the API surface, the actual Whisper endpoint can be a follow-up.

### Acceptance
- Every `bash` and `edit` tool call in the chat shows an approval card with a diff.
- The user can approve once, deny, or allow for the rest of the session.
- API key entry via Settings → "Sign in" → keyring. Never on disk in plaintext.
- Image attachment: drag a PNG, send a message that includes the image, the model can describe it.

## Wave 7 — Plan mode, Activity pane, notifications, polish

**Why this wave:** the core is done. Wave 7 is the "make it feel right" wave. Plan mode is a power user feature; the Activity pane is the audit trail; notifications handle the "I closed the laptop and want to know when it's done" case.

### Tasks
- **7.1 Plan mode.** Toggle in the composer (`Plan` vs `Build`). In plan mode, the agent runs read-only (only `read`, `glob`, `grep` available, others are denied with a clear error). When the model finishes, the response is presented as a "proposed plan" with a "Apply" button that re-runs the loop in build mode with the plan as a system prompt.
- **7.2 Activity pane.** Right-side panel that shows the full event log: every text delta, every tool call, every token count. Filterable by event type. Persists across sessions (so you can see "what did the model do last Tuesday").
- **7.3 Notifications.** On long-running task completion (>30s), the OS shows a notification. Click the notification to focus the app. Tauri has a `notification` plugin; use that.
- **7.4 Slash command suite.** `/compact` (manual compaction — placeholder for the future auto-compaction), `/login` (re-prompt for API key), `/model` (switch), `/diff` (show working-tree diff), `/help`, `/clear` (new session), `/resume <id>`.
- **7.5 Settings pane.** Default approval policy, theme (light/dark, system), telemetry opt-in (off by default), project picker recent-list management.
- **7.6 Onboarding.** First-launch flow: pick a project, set the API key, run a "hello world" task. Designed to take <2 minutes.
- **7.7 E2E test suite.** Playwright tests that drive the full Tauri webview: open app, click "new chat", submit a task, see the response, click an approval card. Recorded as video for the demo.

### Acceptance
- Plan mode → model proposes a plan → user clicks Apply → agent runs in build mode.
- Activity pane shows every event of the last 10 sessions.
- Closing the laptop, running a long task, reopening → notification fired.
- First-time setup takes <2 minutes.

## What we are NOT building in phase 2

- **Multi-host scheduling.** Spec §3.2 explicitly excludes it.
- **MCP, plugins, hooks.** Deferred per spec.
- **Phone client.** Deferred per spec §3.2.
- **Subagents / swarms.** Deferred per spec.
- **Sandbox (bubblewrap/landlock).** Spec §3.2 explicitly excludes; v0 is trusted-user only.
- **Auto-compaction.** Deferred per spec §3.2.

## What changes in the kernel during phase 2

The kernel grows four small seams:
- `policy::ApprovalPolicy` trait (wave 4)
- `message::Part::Image` variant (wave 6)
- `AgentConfig::approval_policy: Arc<dyn ApprovalPolicy>` (wave 4)
- `crow serve` binary (wave 4)

Everything else (provider, tools, session, recovery) stays unchanged. The TUI/CLI/headless clients all keep working — they just opt into the policy at session start.

## Decision points for Ferre

1. **Tauri 2 vs Electron vs raw webview.** Tauri is the right call: small binary, Rust backend we can share with the kernel, native cross-platform packaging. Electron adds 100MB+ of Chromium and a Node runtime. Raw webview loses native packaging.
2. **Frontend framework.** Vanilla TS + custom elements (smallest), or Preact (~3KB), or Svelte (~5KB compiled). Codex uses a custom-element approach. v0 ships with vanilla TS + a tiny custom-element base class.
3. **Plan mode before or after image attachments.** I'd argue plan mode is more valuable to a power user; image attachments are a nice-to-have. But wave ordering puts plan mode in wave 7 because it depends on the approval UX from wave 6.
4. **Voice input.** Skip in v0; it adds Whisper dependency + browser-API surface for not-much-gain.
5. **Onboarding vs settings.** Both are wave 7. Onboarding is the "first time you open the app" flow; settings is the "I want to change something" pane. Two separate tasks.

## Plan for next session

The user said they can't test right now. Next session, Ferre returns and:
1. Reads this plan
2. Approves the wave ordering (or moves things around)
3. I dispatch wave 4 (app-server + approvals) first — the kernel layer
4. Then wave 5 (Tauri shell) — the desktop
5. Then waves 6 and 7 in parallel where possible

I will NOT start dispatching now per the user's "can't test right now" message. The plan is written and committed; the work resumes when Ferre is back.
