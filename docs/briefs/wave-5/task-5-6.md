### Task 5.6 — Composer (multiline input + slash-commands + @-mentions)

**Files:**
- New: `crates/crow-desktop/src/frontend/components/composer.ts`
- New: `crates/crow-desktop/src/frontend/components/slash-popup.ts`
- New: `crates/crow-desktop/src/frontend/components/mention-popup.ts`
- Modify: `crates/crow-desktop/src/frontend/main.ts`

**Why this exists:** the composer is how the user sends messages. Multiline input, slash-commands, and @-mentions are the three interactions that make the composer powerful.

**Spec references:** none direct — UX polish.

**Features:**
- **Multiline `<textarea>`.** Enter = submit, Shift+Enter = newline. The send button (or Cmd+Enter) also submits.
- **Slash-command popup.** Typing `/` shows a popup with: `/compact`, `/model`, `/login`, `/diff`, `/help`, `/clear`, `/resume`. Filtered as the user types.
- **@-mention popup.** Typing `@` shows a popup with files from the project root. Fuzzy match. Selecting inserts `@path/to/file` (which the kernel attaches as context).
- **Submit** dispatches a `crow://submit` event with the text + any attached files.

**Procedure:**
1. Build the `crow-composer` custom element. It's a wrapper around `<textarea>` with the popup overlays.
2. Build the `crow-slash-popup` and `crow-mention-popup` as separate elements that take a `position` and a `query`.
3. File listing: at composer mount, the frontend asks the backend (via Tauri command) for a directory listing of the project root (capped at depth 4, 200 files). Cache the listing.
4. Tests: a unit test that simulates typing `/mo` and confirms the popup filters correctly.
5. Tests: a unit test that simulates typing `@src/m` and confirms the popup shows matching files.

**Acceptance:**
- Manual test: type `/` → popup appears, type `mo` → only `/model` remains, Enter → inserts `/model` in the textarea.
- Manual test: type `@` → popup appears with the project files, type a few characters → filters, Enter → inserts `@path/to/file`.
- Manual test: type Enter → submits. Type Shift+Enter → inserts newline.
- `cargo build --workspace` is clean.

**Forbidden:**
- No `eval()` in the popup.
- No running external commands in the file listing (no `find`, no `git ls-files`).
- No fuzzy search library (write a 20-line Levenshtein or use a simple substring match).

**Dependency:** none new.
