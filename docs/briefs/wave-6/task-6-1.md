### Task 6.1 — Approval card UI

**Files:**
- New: `crates/crow-desktop/src/frontend/components/approval-card.ts`
- Modify: `crates/crow-desktop/src/frontend/components/chat.ts` (render approval cards inline)

**Why this exists:** the spec says the agent is autonomous by default. But the desktop user expects to gate tool calls. The approval card is a per-tool prompt that fits inline in the chat (not a modal).

**Spec references:** spec §4 (the policy seam, layer in the desktop on top), §3.2 (no permission prompts in the kernel — the desktop adds them).

**Behavior:**
- When the chat receives a `ToolStarted` event, it renders an `<crow-approval-card>` inline below the assistant message that produced it.
- The card shows: tool name, args (formatted), and (for `bash`) the command. For `write`/`edit` it shows a diff (task 6.2).
- Buttons: `Allow once`, `Allow for session`, `Deny`.
- Default timeout: 60s, then auto-deny (configurable per session).
- The agent loop blocks on the approval. The kernel's cancel token is honoured (so a Deny or timeout cancels the tool).

**IPC:**
- Frontend: when the user clicks a button, sends `crow://approval/{session_id}/{call_id}` with the decision.
- Backend: writes the decision to a per-call channel (the kernel's `Policy` trait needs a new `ask()` method that returns a `Future<Decision>`).
- The new `AskPolicy` (replaces the v0 default) holds a `HashMap<CallId, oneshot::Sender<Decision>>` and the IPC bridge writes to those channels.

**Procedure:**
1. Add `crow::policy::AskPolicy` (replaces `NoOpPolicy` as the default for desktop sessions).
2. Add a new Tauri command `approval_resolve(call_id, decision) -> ()`.
3. Build the `crow-approval-card` custom element.
4. Wire the chat to render the card on `ToolStarted` and listen for button clicks.
5. The 60s timeout is enforced in the kernel (the `oneshot::Sender` is held by a `tokio::time::timeout`).

**Acceptance:**
- Manual test: submit a task that calls `bash` → the chat shows the approval card.
- Click `Deny` → the tool returns a denied result; the agent continues.
- Click `Allow once` → the tool runs; the same tool is denied if called again.
- Click `Allow for session` → the same tool is auto-allowed for the rest of the session.
- Manual test: wait 60s → auto-deny.
- `cargo build --workspace` is clean.

**Forbidden:**
- No `setTimeout` on the frontend for the 60s timeout — the kernel enforces it.
- No modal dialogs (inline cards only).
- No `unwrap`/`expect` in library code.

**Dependency:** none new.
