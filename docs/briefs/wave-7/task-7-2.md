### Task 7.2 — Activity pane

**Files:**
- New: `crates/crow-desktop/src/frontend/components/activity-pane.ts`
- Modify: `crates/crow-desktop/src/frontend/main.ts` (right-side panel toggle)
- New: `src/server/replay_store.rs` (persist full event stream to a SQLite-ish store — or just JSONL, in v0)

**Why this exists:** the chat pane shows the narrative. The activity pane shows the audit trail: every event, every tool call, every token count. Filterable by event type. Persists across sessions.

**Spec references:** spec §11 (the full event stream is the source of truth for what the agent did).

**Behavior:**
- Right-side panel in the desktop, toggled with Cmd+Alt+A.
- Shows a chronological list of events for the current session. Each event is one row.
- Filter dropdown: `All` / `Tools` / `Bash` / `Text` / `Reasoning` / `Errors`.
- Click an event → jumps to that event in the chat (highlighted for 2s).
- Persists: events are stored in a sidecar file `~/.local/share/crow/sessions/<id>.events.jsonl` (one event per line, full AgentEvent). The session file (from wave 1) only stores the projected SessionEntry view; the sidecar stores the full stream.

**Procedure:**
1. Add a `events.jsonl` sidecar to the session writer. The agent loop emits events to BOTH the session writer (as `SessionEntry`) and the events writer (as raw `AgentEvent` JSONL).
2. The desktop reads the events.jsonl on `SessionLoad`.
3. Build the activity pane component.
4. Tests: the sidecar writer and reader round-trip an event stream.

**Acceptance:**
- Manual test: open a past session → activity pane shows all events in order.
- Manual test: filter by `Bash` → only bash events remain.
- Manual test: click an event → chat jumps to it.
- `cargo build --workspace` is clean.

**Forbidden:**
- No re-emitting events from the chat pane (the activity pane reads from the sidecar).
- No compressing the sidecar (let it grow — disk is cheap; the user can prune later).

**Dependency:** none new.
