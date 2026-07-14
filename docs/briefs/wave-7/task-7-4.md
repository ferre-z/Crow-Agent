### Task 7.4 — Slash command suite

**Files:**
- New: `crates/crow-desktop/src/frontend/components/slash-popup.ts` (extend with the full suite)
- Modify: `crates/crow-desktop/src/frontend/components/composer.ts` (handle the new commands)

**Why this exists:** the slash command popup from 5.6 was a stub. This task fills it in with the full suite.

**Spec references:** none — UX polish.

**Commands:**

| Command | Action |
|---|---|
| `/compact` | Run manual compaction (placeholder for the future auto-compactor). For now: just append "Please summarize our conversation so far." to the current session. |
| `/login` | Open the API key entry dialog (writes to the keyring). |
| `/model <name>` | Switch the model for the current session. The desktop dispatches a `crow://session/set_model` event. |
| `/diff` | Show the working-tree diff in a side panel. |
| `/clear` | Start a new session in the current project. |
| `/resume <id>` | Resume a specific session by ID. |
| `/help` | Show a help dialog. |
| `/settings` | Open the settings pane. |
| `/quit` | Close the app. |

**Implementation:**
- Each command is a string-to-action map. The composer dispatches the action as a Tauri event.
- Most actions are pure frontend (`/clear`, `/help`, `/settings`). Some need backend support (`/login`, `/model`, `/resume`).

**Procedure:**
1. Build the command map in `slash-popup.ts`.
2. Wire each command to a Tauri event or direct handler.
3. Tests: a unit test for the command parser.
4. Manual: each command works.

**Acceptance:**
- All 9 commands work.
- The popup filters as the user types.
- `cargo build --workspace` is clean.

**Forbidden:**
- No new commands not in the table (we can add more in v1).
- No `eval()` in command parsing.

**Dependency:** none new.
