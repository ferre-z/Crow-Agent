### Task 5.5 — Chat pane (event stream rendering)

**Files:**
- New: `crates/crow-desktop/src/frontend/components/chat.ts` (chat pane custom element)
- Modify: `crates/crow-desktop/src/frontend/main.ts`
- Modify: `crates/crow-desktop/src/lib.rs` (Tauri command: `submit`, event listener for `Event` messages)

**Why this exists:** the chat pane is the heart of the app. It renders the live event stream: text deltas, tool calls, bash output, file diffs.

**Spec references:** v0 spec §11 (agent loop — events are the primary output), §18 (acceptance criteria 1-3).

**Event rendering rules (per `AgentEvent` variant):**

| Variant | Render as |
|---|---|
| `RunStarted` | (no render; just internal state) |
| `ModelStarted` | small "thinking..." indicator |
| `TextDelta { text }` | append text to current `<p>` (the streaming assistant message) |
| `ReasoningDelta { text }` | append text to a collapsible "Reasoning" section above the final answer |
| `ToolStarted { name, args }` | a tool card with the tool name + args preview (5.6 adds the diff for write/edit) |
| `ToolOutput { stream, chunk }` | append to a `<pre>` inside the tool card (with stdout/stderr tabs if both) |
| `ToolFinished { result }` | close the tool card; show the result summary |
| `ModelFinished` | close the streaming `<p>`, replace the "thinking" indicator with the final answer |
| `RunFinished` | run a final "completed" notification |
| `RunCancelled` | show a "cancelled" indicator |
| `RunFailed { code, message }` | show the error in red |

**Procedure:**
1. Build the `crow-chat` custom element in `chat.ts`. It maintains a list of "blocks" (one per logical event), each with a renderer.
2. The Tauri event listener receives `Response::Event` messages and dispatches them to the active chat element.
3. The chat element dispatches `crow://user_input` events when the user submits a message.
4. Tests: a unit test that runs the chat with a recorded event stream and checks the rendered HTML.

**Acceptance:**
- Manual test: new chat, submit "hello" → the chat renders the model's response.
- Manual test: tool call → the tool card appears with the bash output streaming in.
- `cargo build --workspace` is clean.
- A unit test for the chat element (snapshot test against a known event stream).

**Forbidden:**
- No `innerHTML = ...` with untrusted content (use `textContent` for text, or sanitise).
- No re-rendering the entire chat on every event (only update the changed block).
- No `eval()` or `Function()` in the frontend (CSP violation).

**Dependency:** none new (vanilla TS).
