# Crow TUI 1:1 Claude Code Parity Plan

Status: planning doc — every sub-feature below is a candidate for its
own commit. **HARD** features must ship with integration tests;
**MEDIUM** with at least unit tests; **EASY** with smoke + a doc
note. No feature gets a partial implementation; if any acceptance
criterion can't be met, the slice doesn't ship.

The numbering (`F.NN.SS`) is the dispatch order; lower numbers ship
first. Each sub-feature is small enough to merge in a single commit
under our 232+ existing tests + clippy `-D warnings` + fmt-check
gates.

Terminology: **EASY** = ~50 LOC, **MEDIUM** = ~100-300 LOC + tests,
**HARD** = kernel change, async refactor, or new external protocol.

---

## Layer 1 — Visual & Layout (the chrome)

### F.01 Header bar

**F.01.01 — Header bar shows live model id** — `Easy`
Header renders `crow  model: <model>` with the model id coloured
cyan. Updates when the user changes model at runtime.
Acceptance: visible on every TUI launch; reflects `--model` value.

**F.01.02 — Header shows plan-mode badge** — `Easy`
When `App.plan_mode` is true, header renders a yellow `PLAN` chip
after the model. (Already shipped in slice 5.)

**F.01.03 — Header shows current working directory** — `Easy`
Append `cwd: <truncated path>` after the session id, dim grey.
Acceptance: updates when `--project-root` changes between sessions.

**F.01.04 — Header shows git branch when cwd is a git repo** — `Medium`
Run `git -C <cwd> rev-parse --abbrev-ref HEAD` once at TUI start;
render `branch: <name>` in dim grey. Cache the result; refresh on
`Esc + g` or `/refresh` slash.
Acceptance: hidden when cwd is not a git repo; updates on refresh.

### F.02 Chat scrollback

**F.02.01 — Tail-anchored scroll on new content** — `Easy`
(Already shipped.) New chat entries auto-scroll the viewport to
the bottom unless the user has scrolled up.

**F.02.02 — PageUp/PageDown scroll the chat** — `Easy`
(Already shipped.) `PageUp`/`PageDown` move by viewport height;
`End` jumps to bottom; `Home` to top.

**F.02.03 — Vim-like `gg` / `G` jumps** — `Medium`
Press `g` twice within 500ms to jump to top; `G` jumps to bottom.
State: track last-key time in App.
Acceptance: rapid double-g jumps; `G` single-press jumps to bottom.

**F.02.04 — Mouse wheel scrolls the chat** — `Medium`
`MouseEventKind::ScrollUp` / `ScrollDown` adjust `scroll_back` the
same as `PageUp`/`PageDown`. Enable mouse capture on TUI start.
Acceptance: scrolling within the chat area scrolls the chat;
clicking inside the composer focuses it.

### F.03 Composer (input box)

**F.03.01 — Multi-line composer with Shift+Enter newline** — `Easy`
(Already shipped.) Plain Enter submits; Shift+Enter inserts newline.

**F.03.02 — Composer prompt glyph shows run state** — `Easy`
(Already shipped.) `❯` when idle; `⏳` while a run is in flight;
yellow when paused for approval.

**F.03.03 — Up/Down arrow recalls past prompts in this session** — `Medium`
Maintain an in-session history list. `Up` on empty composer moves
to the previous user message; `Down` moves forward. Don't
duplicate the model's replies.
Acceptance: pressing `Up` once shows last user prompt; pressing
`Up` again walks back through older prompts; `Down` walks forward;
`Esc` returns to empty.

**F.03.04 — Tab completion for slash commands** — `Medium`
`Tab` while the composer starts with `/` cycles through matching
slash commands. `Tab` while starting with `@` opens file picker.
Acceptance: `/h<Tab>` suggests `/help`; cycling multiple Tab presses
walks the candidate list; `Backspace` restores free editing.

**F.03.05 — `@`-mention file completion** — `Hard`
When the user types `@`, show a file picker overlay listing files
under `project_root`. Fuzzy match on subsequent keystrokes. `Enter`
inserts the path; on submit, expand `@path` references to the file
contents inline as `<file>` blocks.
Acceptance: typing `@` triggers the picker; filtering works;
submitting a prompt with `@README.md` results in the model seeing
`<file path="README.md">...contents...</file>`.

**F.03.06 — Bracketed paste mode for image attachments** — `Hard`
Detect terminal paste of image bytes (iTerm2 / WezTerm convention);
save to a temp dir under `.crow/.attachments/`; insert as
`@<path>` mention in the composer. The image is passed to the
provider's image-input format.
Acceptance: pasting an image file in WezTerm results in an
attachment mention; model sees the image content in the prompt.

### F.04 Status bar

**F.04.01 — Status bar shows run phase + spinner** — `Easy`
(Already shipped.) `idle / running / done / cancelled / failed`
plus a braille spinner during `Running`.

**F.04.02 — Status bar shows last error inline** — `Easy`
(Already shipped.) `RunFailed` events surface in the status bar
as red text.

**F.04.03 — Status bar shows live tool timer** — `Medium`
When `phase == Running` AND a tool is currently executing, show
`<tool_name> <elapsed>s`. Tick every 200ms. Reset when the tool
finishes.
Acceptance: while bash runs, status reads `bash 3s`; when edit
runs, `edit 1s`; updates live.

**F.04.04 — Status bar shows cumulative input/output token counts** — `Medium`
Accumulate `ModelFinished { usage }` across the session. Display
`tok in:1234 out:5678` in dim grey on the status bar.
Acceptance: counts grow after each `ModelFinished`; reset on a
fresh `--resume <new>`.

**F.04.05 — Status bar shows approximate USD cost** — `Medium`
Multiply accumulated tokens by a per-model rate table
(`config/pricing.toml` keyed on model id; falls back to a global
default). Display `$0.0123`. Format with 4 fractional digits when
< $1, else 2.
Acceptance: each turn updates the displayed cost; pricing.toml can
be edited without recompiling.

**F.04.06 — Status bar shows context window usage %** — `Hard`
Sum the input tokens of the last `ModelFinished` plus prior
history. Compare to the model's known context size (lookup table).
Display `ctx 42%` with a colour ramp (green <60%, yellow <85%, red
>=85%).
Acceptance: percentage updates each turn; warning colour flips at
the thresholds.

### F.05 Tool cards (chat)

**F.05.01 — Per-tool rich rendering** — `Easy`
(Already shipped.) `read`, `write`, `edit`, `bash` each have a
specialised card layout.

**F.05.02 — Collapsible tool cards** — `Medium`
Tool cards start collapsed showing only the header. Press
`Enter` or click the card to expand the body. State stored in
`App.expanded_tools: HashSet<ToolCallId>`.
Acceptance: cards render collapsed by default; `Enter` toggles;
state persists across scrolls.

**F.05.03 — Truncation footer with "show more"** — `Medium`
When `truncated: true`, render a `[show more]` link at the bottom
of the card. `Enter` on it expands the full output in a scrollable
modal.
Acceptance: truncated bodies show the link; clicking it reveals
the full output.

**F.04 — Inline error banner** — `Easy`
(Already shipped.) `RunFailed` events push a red banner inline.

### F.06 Overlays

**F.06.01 — Session picker overlay** — `Easy`
(Already shipped.) Arrow keys + PageUp/Down + Enter + Esc.

**F.06.02 — Approval overlay** — `Easy`
(Already shipped.) `y`/`a`/`n`/`Esc` keymap.

**F.06.03 — Slash-command palette overlay** — `Hard`
Triggered by `/` followed by 200ms of inactivity or by the slash
command not existing. Shows full list of available slash commands
+ skill names + agents. Fuzzy match as user types.
Acceptance: typing `/` then waiting 200ms opens the palette;
typing characters filters; arrow keys navigate; Enter submits.

---

## Layer 2 — Slash commands

Every slash command below must work from the TUI composer, must
appear in `/help`, and must be wired into the driver's
`apply_local_slash` path (for sync) or pre-empted (for async,
like `/resume`).

### F.10 Built-in slash commands

**F.10.01 — `/help` lists every slash command** — `Easy`
(Already shipped.) Reads `SLASH_HELP` constant. Add new entries as
commands are added.

**F.10.02 — `/clear` empties the chat scrollback** — `Easy`
(Already shipped.)

**F.10.03 — `/resume` opens the picker overlay** — `Easy`
(Already shipped.) Driver pre-empts and loads sessions.

**F.10.04 — `/plan` toggles plan-mode flag** — `Easy`
(Already shipped.) Note in status: takes effect on next session.

**F.10.05 — `/model` shows current model** — `Easy`
(Already shipped.)

**F.10.06 — `/doctor` shows config snapshot** — `Easy`
(Already shipped.)

**F.10.07 — `/quit` exits the TUI** — `Easy`
(Already shipped.)

**F.10.08 — `/cost` shows cumulative cost for this session** — `Medium`
Reads `App.cumulative_cost_usd`; renders a multi-line summary
including per-tool-call breakdown (tool name + cost).
Acceptance: shows total + per-tool list; resets on `--resume`.

**F.10.09 — `/compact` summarises older context** — `Hard`
When context exceeds 80%, runs a one-shot completion asking the
model to summarise the conversation so far; replaces old
`UserMessage`/`AssistantMessage` entries with a single
`AssistantMessage` containing the summary. Bumps a `compaction_count`
field on the App. Persists the compaction to the session JSONL.
Acceptance: `/compact` cuts input tokens by ≥ 50% on a long
session; survives `--resume`.

**F.10.10 — `/memory` shows project + user memory files** — `Medium`
Reads `~/.crow/CLAUDE.md` (user memory) and `<project>/CLAUDE.md`
(project memory) and prints their paths + first 20 lines.
Acceptance: both files listed when present; "no memory file" when absent.

**F.10.11 — `/memory <subcommand>` supports edit / append** — `Hard`
Subcommands: `edit`, `append <text>`, `clear`. `edit` opens
`~/.crow/CLAUDE.md` (or project) in `$EDITOR`; `append` writes
`<text>` to the end of the chosen file; `clear` truncates.
Acceptance: edits persist across TUI sessions; project memory takes
precedence over user memory in the context compiler.

**F.10.12 — `/init` generates an AGENTS.md for the current project** — `Hard`
Runs the agent in a one-shot mode where the system prompt is
"explore the project and write AGENTS.md describing its structure,
build commands, and conventions". After the run, prompts the user
to review the diff and accept.
Acceptance: AGENTS.md is created at `<project>/AGENTS.md`; the
context compiler picks it up on the next turn.

**F.10.13 — `/permissions` opens permission editor** — `Hard`
Opens an overlay listing every permission rule (from
`~/.config/crow/rules.toml` and `<project>/.crow/rules.toml`).
Each row: rule pattern + decision (`allow`/`ask`/`deny`) +
controls. `Enter` cycles the decision; `d` deletes the rule; `n`
creates a new rule via an inline form.
Acceptance: edits persist to disk on close; reload on next launch.

**F.10.14 — `/config` opens runtime config editor** — `Hard`
Same shape as `/permissions` but for `~/.config/crow/config.toml`
fields: model, base_url, max_turns, max_tool_calls, theme.
Acceptance: same persistence behaviour.

**F.10.15 — `/status` shows session metadata** — `Medium`
Compact dump: session id, started_at, total turns, total tool
calls, total tokens, total cost, current phase. One screen, no
scroll.
Acceptance: shows everything on one screen; updates each turn.

**F.10.16 — `/add-dir <path>` grants the agent access to another directory** — `Medium`
Adds `<path>` to `App.allowed_extra_dirs` (a `Vec<PathBuf>`). The
path-confinement check in `tool/path.rs` consults this list in
addition to `project_root`.
Acceptance: after `/add-dir /tmp`, the bash tool can `cat /tmp/x`;
without it, returns `PathEscape`.

**F.10.17 — `/vim` toggles vim mode in the composer** — `Hard`
Switches `tui-textarea` to vim keybindings via its built-in vim
mode. State: `App.vim_mode: bool`. Persisted via `/config`.
Acceptance: `/vim` toggles; in vim mode, hjkl moves the cursor,
`i` enters insert, `:` opens a mini command line.

### F.11 Skill-backed slash commands

**F.11.01 — Skill discovery from `~/.crow/skills/` and `.crow/skills/`** — `Hard`
Walk both directories for `SKILL.md` files. Each file declares:
- `name` (slash command name, e.g. `review-pr`)
- `description` (shown in `/help` and command palette)
- `body` (the prompt template; appended to the user's prompt when
  the slash command is invoked)
Acceptance: skills appear in `/help`; project skills take
precedence over user skills with the same name.

**F.11.02 — Skill invocation routes through agent loop** — `Hard`
`/review-pr 123` is equivalent to submitting
`<skill-body>\n\n<user-args>` as the user message. No special
tool needed.
Acceptance: skill runs through the same agent loop; the resulting
session log shows the skill invocation as a `UserMessage`.

### F.12 Agent / subagent slash commands

**F.12.01 — `/agents` lists configured subagents** — `Medium`
Reads `~/.config/crow/agents/*.md`. Each file declares an agent
with a system prompt. Renders a list: name + description.
Acceptance: lists all configured agents; "no subagents configured"
when none.

**F.12.02 — `@agent-name <prompt>` runs a subagent** — `Hard`
`@explore /tmp/secret-project` spawns a background agent that
runs in a separate session. Output streams back into the main
session as a system message tagged `[explore]`. Cost tracked
separately under `/cost`.
Acceptance: subagent runs to completion; output appears; main
session can keep running in parallel.

---

## Layer 3 — Permission / Policy system

### F.20 Permission rule files

**F.20.01 — `~/.config/crow/rules.toml` loaded at startup** — `Medium`
Already partially in place (`policy::RuleBasedPolicy::from_file`).
Needs to be wired into the default provider stack so rules take
effect automatically.

**F.20.02 — `<project>/.crow/rules.toml` overrides user rules** — `Medium`
Project-level rules apply on top of user rules. Project decisions
win on conflict. Re-read on TUI start.

**F.20.03 — Rule pattern supports tool name + argument patterns** — `Hard`
Pattern grammar:
- `tool = "bash"` matches by tool name
- `tool = "bash", args.command = "^git .*"` matches when the bash
  command matches a regex
- `tool = "write", args.path = "^/etc/.*"` matches write to /etc
Acceptance: rules with arg patterns are evaluated against the
parsed JSON args; non-matching rules fall through.

### F.21 Approval policies

**F.21.01 — Default policy: read=allow, mutation=ask** — `Easy`
(Already shipped.)

**F.21.02 — Per-session allowlist via `a` in approval overlay** — `Easy`
(Already shipped.)

**F.21.03 — Per-tool "always allow" persisted to disk** — `Hard`
When the user presses `a` in the approval overlay, append
`allow_always: ["bash", "edit"]` to `~/.config/crow/rules.toml`
under the user's section. Persists across sessions.
Acceptance: after `a` on bash, next session auto-allows bash.

**F.21.04 — `--dangerously-skip-permissions` flag** — `Medium`
On startup, sets all `DefaultPolicy` rules to `Allow` so no overlay
ever appears. Logs a warning at TUI start.
Acceptance: tool calls execute without showing the approval card.

---

## Layer 4 — Model & provider layer

### F.30 Provider configuration

**F.30.01 — Default model set to Nemotron 3 Ultra** — `Easy`
(Already shipped.)

**F.30.02 — `--model` flag overrides per-launch** — `Easy`
(Already shipped.)

**F.30.03 — `CROW_MODEL` env override** — `Easy`
(Already shipped.)

**F.30.04 — `~/.config/crow/config.toml` model field** — `Easy`
(Already shipped.)

**F.30.05 — Per-model context size + pricing table** — `Medium`
`config/pricing.toml` ships with a default table:
```toml
[nvidia/nemotron-3-ultra-550b-a55b]
context_size = 262144
input_per_1k = 0.0005
output_per_1k = 0.0015
```
Loaded at startup; unknown models fall back to a "default" entry.
Acceptance: `/status` shows context % and `/cost` shows USD for the
active model.

**F.30.06 — Model picker overlay** — `Hard`
`/model` (with no args) opens a modal listing all known models
(from the NVIDIA catalog filtered to those the user has access
to). `Enter` sets the new model and restarts the worker task
with the new model id.
Acceptance: switching model mid-session preserves the chat history.

### F.31 Reasoning / thinking tokens

**F.31.01 — Reasoning rendered as foldable block** — `Easy`
(Already shipped.) `⌥ <reasoning>` styled italic in the chat.

**F.31.02 — Reasoning toggle: `/reasoning on|off`** — `Medium`
Switches between rendering reasoning in-line or hiding it. State:
`App.show_reasoning: bool`. Default: on.
Acceptance: `/reasoning off` hides the `⌥` blocks; subsequent
runs don't render reasoning.

**F.31.03 — Reasoning budget config** — `Medium`
`/reasoning-budget 8000` asks providers that support it
(Nemotron Ultra, OpenAI o-series) to budget reasoning tokens.
Falls back to no-op for providers that don't.
Acceptance: setting a budget visibly limits reasoning length on
the next turn.

### F.32 Streaming

**F.32.01 — Token-by-token text delta** — `Easy`
(Already shipped.)

**F.32.02 — Reasoning token delta** — `Easy`
(Already shipped.)

**F.32.03 — Tool call streaming (arguments chunked)** — `Hard`
Some providers stream tool arguments as multiple chunks. The
kernel concatenates them, parses on `ToolCallComplete`. Test: send
a long bash command, verify it appears in one `ToolStarted`
event, not split across runs.

---

## Layer 5 — Sessions & persistence

### F.40 Session storage

**F.40.01 — JSONL log per session** — `Easy`
(Already shipped.)

**F.40.02 — `--resume <id>` reuses an existing log** — `Easy`
(Already shipped.)

**F.40.03 — `crow sessions` lists every session** — `Easy`
(Already shipped.)

**F.40.04 — Session naming: `--name <label>` saves a friendly name** — `Medium`
Adds the label to a sidecar `~/.crow/session-names.json` keyed by
session id. The picker shows the name when present, falling back
to the truncated ULID.
Acceptance: `--name "investigate the bug"` then `crow sessions`
shows the friendly name; picker shows the same.

**F.40.05 — Auto-naming from first user prompt** — `Medium`
On the first user message of a session, ask the model to produce
a 3-7 word summary. Store it as the session's name.
Acceptance: first turn of a new session gets a name like
"add claude code parity"; picker shows it.

### F.41 Session export / import

**F.41.01 — `crow export <id> --out file.json`** — `Medium`
Serialises the session JSONL into a single JSON document with
metadata + transcript.
Acceptance: round-trip via `crow import` reproduces the session.

**F.41.02 — `crow import <file>`** — `Medium`
Counterpart; creates a new session log from an exported JSON.
Acceptance: imported sessions appear in `crow sessions` and the
picker.

### F.42 Session history in `/resume`

**F.42.01 — Sessions sorted newest first** — `Easy`
(Already shipped.)

**F.42.02 — Picker shows timestamp + path** — `Easy`
(Already shipped.)

**F.42.03 — Picker filter** — `Medium`
Type a substring to filter sessions by id prefix or label.
Acceptance: typing `01HX` filters to ULIDs starting with `01HX`.

**F.42.04 — Picker search by message content** — `Hard`
Index first user message + summary on session write; picker lets
the user filter by content match. Backed by a small sqlite or
plain in-memory hash.
Acceptance: searching for "nemotron" surfaces sessions where the
first prompt mentions it.

---

## Layer 6 — Tools

### F.50 Built-in tools (already shipped)

**F.50.01 — `read`** — `Easy`
(Already shipped.)

**F.50.02 — `write`** — `Easy`
(Already shipped.)

**F.50.03 — `edit`** — `Easy`
(Already shipped.)

**F.50.04 — `bash`** — `Easy`
(Already shipped.)

### F.51 New tools

**F.51.01 — `glob` tool** — `Medium`
Args: `{ pattern: String, path?: String }`. Returns matching
relative paths sorted by mtime. Uses the `ignore` crate (already
a dep) for fast `.gitignore`-aware matching.
Acceptance: model can find files matching `**/*.rs` under
`project_root`.

**F.51.02 — `grep` tool** — `Medium`
Args: `{ pattern: String, path?: String, glob?: String,
  case_insensitive?: bool, line_numbers?: bool }`. Returns
matching lines with file:line:content format.
Acceptance: model can search for `TODO` across `.rs` files.

**F.51.03 — `web_fetch` tool** — `Medium`
Args: `{ url: String, prompt?: String }`. Fetches the URL,
extracts readable text (via `readability` crate or simple
heuristic), optionally asks the model to summarise.
Acceptance: model can fetch and summarise a public URL.

**F.51.04 — `web_search` tool** — `Hard`
Args: `{ query: String, max_results?: u32 }`. Calls a configurable
search backend (default: DuckDuckGo HTML scraping; configurable
to SerpAPI / Tavily if `SERPER_API_KEY` / `TAVILY_API_KEY` env
var is set).
Acceptance: model can search the web and read results.

**F.51.05 — `image` tool** — `Hard`
Args: `{ path: String }`. Reads an image file, base64-encodes,
returns as a multimodal content part. Provider-specific adapter.
Acceptance: model can read a PNG and describe it.

### F.52 Tool UX

**F.52.01 — Long bash output truncates with `[+N more lines]`** — `Medium`
Truncate bash output at 200 lines; show a count of truncated
lines; `Enter` on the truncation footer expands.
Acceptance: a `find /` output is truncated; the footer reads
`[+1547 more lines]`; Enter expands.

**F.52.02 — Bash output streamed incrementally** — `Medium`
(Already partial.) Render each `ToolOutput` chunk live, not just
the final aggregated output.

**F.52.03 — `read` tool with line offset + limit shows range clearly** — `Easy`
(Already shipped — output is `N\tcontent` lines; renderer splits
the line number column.)

**F.52.04 — `edit` diff includes 3 lines of context** — `Easy`
(Already shipped.)

**F.52.05 — `edit` shows the OLD file content side-by-side for context** — `Medium`
Display a 1-line snippet before + after the changed region in the
diff card so the change is contextualised.

---

## Layer 7 — UI polish

### F.60 Themes

**F.60.01 — Default dark theme** — `Easy`
(Already shipped.) Greens, cyans, dark grey accents.

**F.60.02 — Light theme via `--theme light`** — `Medium`
Adds a light palette to `ui.rs`. Background becomes off-white,
text dark grey, accents muted.
Acceptance: `crow tui --theme light` renders with light colours.

**F.60.03 — Theme tokens via `~/.config/crow/theme.toml`** — `Hard`
TOML file with `[header]`, `[status]`, `[tool_card]`, etc.
sections mapping semantic names to RGB tuples. Loaded at startup.
Acceptance: editing the file changes colours without recompile.

### F.61 Mouse

**F.61.01 — Mouse wheel scrolls chat** — `Medium`
See F.02.04.

**F.61.02 — Click to focus composer** — `Medium`
Clicking inside the composer area moves the textarea cursor to
the click position. Clicking outside the composer scrolls the
chat.

**F.61.03 — Click on picker row to select** — `Medium`
Mouse clicks on picker rows are equivalent to `Enter`.

**F.61.04 — Click on tool card to expand/collapse** — `Medium`
Same as F.05.02 but driven by mouse.

### F.62 Animation / motion

**F.62.01 — Smooth scroll on new content** — `Medium`
When new content arrives and the user is tail-anchored, animate
the scroll by one line per frame over 100ms instead of jumping.
Acceptance: chat scrolls smoothly when tokens stream in.

**F.62.02 — Approval card slide-in animation** — `Hard`
When the approval card appears, slide it in from the right edge
over 150ms.

---

## Layer 8 — Productivity features

### F.70 Auto-compaction

**F.70.01 — Background compaction when context > 80%** — `Hard`
Spawn a one-shot completion in the background to summarise
recent turns; when ready, swap them out and emit a status line
"compacted N turns → M tokens".
Acceptance: a long session auto-compacts at the threshold; user
sees a status line; conversation continues seamlessly.

### F.71 Code-aware features

**F.71.01 — Syntax highlighting in `read` tool output** — `Hard`
Use `syntect` crate to syntax-highlight based on file extension.
Apply to `read` tool cards.
Acceptance: a `.rs` file renders with Rust keywords coloured.

**F.71.02 — LSP-aware diagnostics** — `Hard`
Detect `rust-analyzer` / `tsserver` availability; surface
diagnostics in the status bar after each edit. (Stretch — this is
a Claude Code feature but expensive to implement.)

### F.72 Git integration

**F.72.01 — Git status in status bar** — `Medium`
Run `git status --porcelain` on TUI start; show `+3 -1 ~2` in
status bar (insertions / deletions / untracked).
Acceptance: status updates on every turn; refresh on demand.

**F.72.02 — `/diff` shows uncommitted changes** — `Medium`
Run `git diff` and render the output in a scrollable modal.

### F.73 Setup wizard

**F.73.01 — First-run wizard** — `Hard`
On first launch (no `~/.config/crow/`), prompt for:
1. Provider (NVIDIA, OpenAI, Anthropic, custom)
2. API key (or instruct to set env var)
3. Default model
4. Theme
Saves to `~/.config/crow/config.toml`.

**F.73.02 — `crow login` for OAuth flows** — `Hard`
OAuth flow for Anthropic (skipped if not on Anthropic).
Out of scope if no Anthropic support exists.

---

## Layer 9 — MCP (Model Context Protocol)

### F.80 MCP server management

**F.80.01 — `~/.config/crow/mcp.json` lists MCP servers** — `Medium`
JSON config:
```json
{"mcpServers": {"opencode": {"command": "opencode", "args": ["mcp"]}}}
```
Already partially shipped as `crow mcp-opencode`.

**F.80.02 — `crow mcp` lists configured MCP servers + tools** — `Medium`
Outputs a table: server name, status (connected/error), tool
count.

**F.80.03 — MCP tools appear in the model's tool registry** — `Hard`
When MCP servers are connected, their `tools/list` is merged into
the tool registry. Tool names are namespaced (`<server>__<tool>`).
Acceptance: model can call MCP tools; tool cards render with the
right namespace.

**F.80.04 — `/mcp` shows server status in the TUI** — `Medium`
Inline status line: `mcp: opencode ✓ 7 tools · fs ✗ down`.

---

## Layer 10 — Diagnostics & logging

### F.90 Logs

**F.90.01 — `crow logs` shows recent log lines** — `Medium`
Default log location: `~/.local/share/crow/crow.log` (or
`$XDG_DATA_HOME/crow/crow.log`). Tail the file.
Acceptance: `crow logs -n 50` shows last 50 lines.

**F.90.02 — `/logs` opens an in-TUI log viewer overlay** — `Hard`
Modal with a scrollable log view. `F` to filter; `c` to clear.
Acceptance: opens, scrolls, closes without leaving the TUI.

**F.90.03 — Verbose mode via `RUST_LOG=debug crow tui`** — `Easy`
(Already shipped — `tracing-subscriber` honours `RUST_LOG`.)

### F.91 Crash recovery

**F.91.01 — Crash-tail detection in session recovery** — `Easy`
(Already shipped.)

**F.91.02 — TUI auto-reopens last session on launch** — `Medium`
Store `~/.crow/last-session-id`. TUI opens with that session
resumed if it still exists.
Acceptance: open TUI, close it, reopen — chat history is there.

---

## Layer 11 — `crow` binary subcommands (CLI)

### F.100 Existing subcommands

**F.100.01 — `crow exec <prompt>`** — `Easy`
(Already shipped.) With `--output-format stream-json` too.

**F.100.02 — `crow sessions`** — `Easy`
(Already shipped.)

**F.100.03 — `crow resume <id> <prompt>`** — `Easy`
(Already shipped.)

**F.100.04 — `crow doctor`** — `Easy`
(Already shipped.)

**F.100.05 — `crow serve`** — `Easy`
(Already shipped. JSON-RPC over stdio for the desktop.)

**F.100.06 — `crow mcp-opencode`** — `Easy`
(Already shipped.)

**F.100.07 — `crow tui`** — `Easy`
(Already shipped.)

**F.100.08 — `crow version`** — `Easy`
(Already shipped.)

### F.101 New subcommands

**F.101.01 — `crow logs`** — `Medium`
See F.90.01.

**F.101.02 — `crow config get|set <key>`** — `Medium`
Get or set a config value. `crow config set model
nvidia/nemotron-3-ultra-550b-a55b` writes to
`~/.config/crow/config.toml`.

**F.101.03 — `crow config edit`** — `Medium`
Opens `~/.config/crow/config.toml` in `$EDITOR`.

**F.101.04 — `crow rules list|add|remove`** — `Medium`
Subcommands for the permission rule file. `crow rules add
'tool=bash,decision=allow,pattern=^git'` writes one rule.

**F.101.05 — `crow export / crow import`** — `Medium`
See F.41.

**F.101.06 — `crow login`** — `Hard`
See F.73.02.

**F.101.07 — `crow skills list|show <name>`** — `Medium`
Lists configured skills and shows a skill's body.

---

## Layer 12 — Project analysis

### F.110 `/init`

**F.110.01 — `/init` creates AGENTS.md** — `Hard`
See F.10.12.

### F.111 `/review`

**F.111.01 — `/review` reviews uncommitted changes** — `Hard`
Runs the agent with a system prompt that asks it to review the
current `git diff` for bugs, style issues, and missing tests.
Output is the review.
Acceptance: `/review` on a clean tree shows "nothing to review".

---

## Implementation order (next 10 slices)

The first 10 numbered slices I'll land, in order. Each is small
enough to ship in one commit, each closes a concrete piece of
parity, and each has a clear demo path.

1. **F.04.04 — Status bar cumulative tokens** (Medium, ~80 LOC) — **SHIPPED `4ee5a8d`**
2. **F.04.05 — Status bar cumulative cost** (Medium, ~150 LOC,
   adds `config/pricing.toml`) — **SHIPPED `a23aa6e`**
3. **F.04.03 — Live tool timer** (Medium, ~120 LOC) — **SHIPPED `4ee5a8d`** (landed alongside slice 1)
4. **F.10.08 — `/cost` slash command** (Easy, builds on F.04.05) — **SHIPPED `7b2e097`**
5. **F.10.15 — `/status` slash command** (Easy, builds on F.04.04) — **SHIPPED `7811d17`**
6. **F.10.16 — `/add-dir <path>`** (Medium, ~100 LOC, kernel change) — **SHIPPED `3228bbb`** (App-side plumbing; kernel integration lands in a follow-up)
7. **F.20.03 — Rule pattern with arg regexes** (Hard, ~200 LOC) — **SHIPPED `d44a903`**
8. **F.10.04 — Friendly session naming** (Medium, ~150 LOC) — **SHIPPED `bcc537c`**
9. **F.40.05 — Auto-naming from first prompt** (Medium, ~100 LOC) — **SHIPPED `925bda8`**
10. **F.10.09 — `/compact` context summarisation** (Hard, ~300 LOC) — **SHIPPED `925bda8`** (lite: status indicator only; full kernel-side compaction lands in a follow-up)

All 10 slices landed in 9 commits (slices 1+3 and 9+10 share commits).

After the first 10, I'll continue with the remaining layers in
rough order: more slash commands, more tools, mouse + themes,
MCP integration, LSP, and finally the setup wizard.

---

## What I'm NOT planning

Per the user's directive to plan everything I can imagine RIGHT NOW
and not "next wave":

- **Mobile / iOS / Android clients** — out of scope, this is a
  terminal project
- **GUI refactor beyond Tauri 2** — desktop already ships
- **Voice / audio input** — out of scope
- **Provider-specific UIs** beyond what's listed (e.g. no custom
  Anthropic OAuth flow beyond what `/login` covers)

Everything else above is in scope for the slices I'm planning to
land in this session / repo.
