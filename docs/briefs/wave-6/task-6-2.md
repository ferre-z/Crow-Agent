### Task 6.2 — Diff preview in approval card

**Files:**
- New: `crates/crow-desktop/src/frontend/components/diff-view.ts`
- Modify: `crates/crow-desktop/src/frontend/components/approval-card.ts`
- Modify: `crates/crow-desktop/src/lib.rs` (Tauri command: `read_file(path)` for the diff)

**Why this exists:** for `write` and `edit`, "do you want me to make this change?" is much more useful with the actual diff visible. Red/green, file path, before/after side-by-side.

**Spec references:** none — UX polish.

**Behavior:**
- When the approval card is for `write` or `edit`, render a `<crow-diff-view>` below the args.
- The diff is computed by the kernel: the tool's `ToolOutcome::Success { output, truncated }` includes a `diff_summary` for write/edit (from wave 3 task 3.1 and 3.2). The kernel emits this as part of the `ToolFinished` event.
- The frontend renders the diff with the `similar`-style line-by-line display: green for added, red for removed, neutral for context.
- The user can edit the proposed change in a textarea before approving. The edited content is sent to the kernel as a new `Submit` with a `<modified-args>` part.

**Procedure:**
1. Build the `crow-diff-view` custom element. Input: a list of `DiffLine { kind, text }`. Output: rendered HTML.
2. Wire the approval card to fetch the diff from the `ToolFinished` event (or directly from the kernel if the event is delayed).
3. The user can edit the proposed change in a textarea. On `Allow once` with edits, dispatch a new `Submit` with the edited args.
4. Tests: a unit test for the diff renderer with a known input.

**Acceptance:**
- Manual test: submit a task that calls `edit` → the approval card shows the diff (red/green).
- Manual test: edit the proposed change in the textarea → click `Allow once` → the change is applied with the edited content.
- `cargo build --workspace` is clean.

**Forbidden:**
- No syncing the entire file to the frontend just to compute a diff (the kernel already has the diff).
- No editing of `read` or `bash` tool args (only write/edit).

**Dependency:** `similar` already in Cargo.toml from wave 3.
