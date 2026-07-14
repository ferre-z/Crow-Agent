### Task 7.3 — System notifications

**Files:**
- New: `crates/crow-desktop/src/notify.rs` (Tauri notification wrapper)
- Modify: `crates/crow-desktop/src/lib.rs` (trigger on long-running task completion)

**Why this exists:** "I closed the laptop and want to know when the task is done" is a real use case. The OS notification system is the right tool.

**Spec references:** none — UX polish.

**Behavior:**
- When a task takes >30s, register a notification: "Crow finished: <last assistant message>". Click → focus the app + scroll to the run.
- If the app is already focused, suppress the notification (a `toast` in the chat is enough).
- Configurable threshold (default 30s) and "always notify / never notify / notify on long tasks only" mode.

**Implementation:**
- Use `tauri-plugin-notification` (cross-platform: macOS Notification Center, Windows toast, Linux libnotify).
- A long-running task detector in `crow-desktop/src/lib.rs` watches the `RunFinished` event timestamps.

**Procedure:**
1. Add `tauri-plugin-notification` to `Cargo.toml`.
2. Register the plugin in the Tauri builder.
3. Implement `notify.ts` in the frontend: a tiny "do not disturb" toggle.
4. The backend watches `RunStarted` and `RunFinished` events. If `RunFinished - RunStarted > threshold`, fire the notification.
5. Clicking the notification focuses the window and dispatches a `crow://scroll_to_run` event with the run ID.
6. Settings pane (7.5) has the threshold + mode controls.

**Acceptance:**
- Manual test: submit a long task, close the app, wait 30s → OS notification appears.
- Click the notification → app focuses, chat scrolls to the run.
- `cargo build --workspace` is clean.

**Forbidden:**
- No notification on every tool call (only on run completion).
- No notifications in DND mode.

**Dependency:** `tauri-plugin-notification = "2"`.
