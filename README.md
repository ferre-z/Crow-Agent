# Crow

A small autonomous coding agent in Rust. Mirrors the workflow of Pi, Claude Code, Codex CLI, and OpenCode: receive a task, reason against a large language model, call tools, observe results, modify a repository, and continue until the task is answered.

The first milestone is a reliable personal tool, not a platform. Keep the kernel small and extensible.

---

## Quick start

**Install (one line):**

```bash
curl -sSf https://raw.githubusercontent.com/ferre-z/Crow-Agent/main/scripts/install.sh | sh
```

**Test from a fresh clone (one line):**

```bash
git clone https://github.com/ferre-z/Crow-Agent.git /tmp/crow && cd /tmp/crow && make test
```

**Try it:**

```bash
crow --version
crow doctor
```

The installer **auto-installs missing dependencies** (Rust via rustup, `git` / `make` / `curl` via your package manager) so it works on a clean box. Linux and macOS only (Windows blocked by the `nix` crate). Pass `--no-bootstrap` to opt out.

Default install uses the **debug** profile so it fits on disk-quota boxes (no `quota exceeded` errors). For an optimised binary, pass `--release`.

### All commands at a glance

| Goal | Command |
|---|---|
| Install | `curl -sSf https://raw.githubusercontent.com/ferre-z/Crow-Agent/main/scripts/install.sh \| sh` |
| Test | `git clone https://github.com/ferre-z/Crow-Agent.git /tmp/crow && cd /tmp/crow && make test` |
| Build release | `cargo build --release` (or `make build`) |
| Install release | `curl -sSf .../install.sh \| sh -s -- --release` |
| Verify config | `crow doctor` |
| Build the kernel + run all tests | `make ci` |
| See every Makefile target | `make help` |
| Launch the TUI | `crow tui` |
| Resume a past session | `crow tui --resume <id>` |
| Plan mode (read-only) | `crow tui --plan` |

## Interactive terminal UI

`crow tui` runs an interactive streaming REPL against the same
kernel the headless `crow exec` uses. The visual surface matches
Claude Code's `tui`:

- Streaming assistant text with **markdown** (bold, italic,
  inline code, fenced code, lists) via `pulldown-cmark`
- **Tool cards** with diffs:
  - `read` — line-numbered file preview
  - `write` — file body preview
  - `edit` — unified red/green diff via `similar`
  - `bash` — command + stdout/stderr + status
- **Session picker** overlay (`/resume`): arrow keys / PageUp-Down
  to navigate, Enter to select, Esc to cancel
- **Approval overlay** for policy-driven asks (`y` allow,
  `a` allow for the rest of the session, `n` deny)
- **Plan mode** (`--plan` or `/plan`): only `read` is available;
  the agent can inspect code but cannot mutate
- **Inline error banners** on `RunFailed` so failures are visible
  after scrolling, not just in the status bar
- **`--no-color`** for screen readers, dumb terminals, CI logs
- **`/help` `/clear` `/doctor` `/model` `/quit`** slash commands
- `PageUp` / `PageDown` / `End` to scroll the chat; tail-anchored
  by default
- `Esc` / `Ctrl+C` interrupt a run; `Ctrl+D` on empty input quits

The TUI shares session storage with the headless CLI: each TUI run
writes a JSONL log under `<project>/.crow/sessions/` that `crow
sessions` lists and `crow tui --resume <id>` reuses.

The Tauri 2 desktop app in `apps/desktop/` is still available
for users who want a native window; it drives the same kernel
through `crow serve`.

---

## Status

**v0 in active development.** Shipped so far:

- Cargo binary crate, Rust toolchain pinned via `rust-toolchain.toml`
- CI on GitHub Actions (`cargo fmt`, `clippy -D warnings`, `cargo test`)
- Provider-neutral event accumulator and message types (`src/event.rs`, `src/message.rs`)
- `genai = 0.6.5` provider adapter against NVIDIA Nemotron 3 Ultra (`src/provider/genai.rs`)
- Project-root-confined tool registry with `read`, `write`, `edit`, `bash` shipped (`src/tool/`)
- Agent state machine + tool-call loop with stop-reason handling, event sink, and durable `RunFailed` records (`src/agent.rs`)
- Hierarchical `AGENTS.md` discovery + context compiler (`src/context.rs`)
- Layered config (CLI > env > user file > defaults) + clap CLI with `exec`, `sessions`, `resume`, `doctor`, `serve`, `mcp-opencode`, `tui` subcommands (`src/cli.rs`, `src/config.rs`)
- Session recovery: trailing-sequence recovery, stale-lock eviction, crash-tail detection (`src/session.rs`)
- `Agent::resume_into` for `crow --resume <id>` (`src/agent.rs`)
- Interactive terminal UI: `crow tui` (`src/tui/`). Streaming REPL against the kernel, per-tool rich rendering with diffs, session picker, approval overlay with session-scoped "always allow", plan mode (`--plan`), markdown rendering in chat, `--no-color` for axe readers, inline error banners.
- App-server (`crow serve`) JSON-RPC over stdio for the desktop shell and external CLIs.
- Tauri 2 desktop shell (`apps/desktop/`) backed by `crow serve` as a sidecar.

In progress: OS keyring, mcp-opencode server hardening.

## Stack

| Concern | Choice |
|---|---|
| Async runtime | `tokio` + `tokio-util` |
| Model client | `genai = 0.6.5` (OpenAI-compatible NVIDIA endpoint) |
| Serialization | `serde`, `serde_json`, `schemars`, `toml` |
| CLI | `clap` |
| Errors | `thiserror`, `anyhow` |
| Diagnostics | `tracing`, `tracing-subscriber` |
| IDs | `ulid` |
| Secrets | `secrecy` |
| Tests | `tempfile`, `filetime`, scripted mock provider |

## Architecture

One binary crate. Module boundaries (from `src/`):

```
cli.rs        # clap entry + subcommands (exec, sessions, resume, doctor, tui, serve, mcp-opencode)
config.rs     # layered config (CLI > env > user > defaults)
ids.rs        # session, run, message, tool-call IDs (ULID)
message.rs    # provider-neutral conversation data
event.rs      # AgentEvent + SessionEntry + AgentEventSink
provider/
  mod.rs      # Provider trait
  stream.rs   # StreamAccumulator + ProviderChunk
  mock.rs     # ScriptedProvider for tests
  genai.rs    # genai 0.6.5 adapter (NVIDIA endpoint)
agent.rs      # state machine, tool-call loop, limits, cancellation, resume
context.rs    # system prompt + AGENTS.md discovery
session.rs    # JSONL writer/reader, recovery, stale-lock detection
policy.rs     # ApprovalPolicy + AskResolver (drives the TUI approval overlay)
tool/
  mod.rs      # Tool trait, registry, ToolSpec, limits
  path.rs     # project-root path resolution
  read.rs     # line-numbered read (sniff-then-read, bounded)
  write.rs    # atomic temp+rename
  edit.rs     # exact-match replacement with diff summary
  bash.rs     # shell exec with process-group kill on timeout
tui/          # interactive terminal UI (crow tui)
  mod.rs      # driver: terminal setup, worker task, channels, main loop
  app.rs      # App model + AgentEvent reducer + keymap
  ui.rs       # layout, chat scrollback, header/status, picker, approval card
  commands.rs # slash-command parser (/help, /clear, /resume, /plan, /quit, ...)
  tools.rs    # per-tool rich rendering (read, write, edit, bash, generic)
  picker.rs   # session picker state machine
  approval.rs # approval overlay state + session allowlist
  markdown.rs # pulldown-cmark -> ratatui Line
app_server.rs # crow serve JSON-RPC over stdio
mcp_opencode.rs # crow mcp-opencode (MCP server that delegates to opencode)
tests/        # integration + gate tests
```

The desktop app (Tauri 2) is a separate crate that talks to the same kernel via the future `crow serve` JSON-RPC service.

## Building

```bash
make test     # offline test suite, no API key needed
make build    # release build (target/release/crow)
make lint     # clippy with -D warnings
make ci       # fmt + lint + build + test (matches GitHub Actions)
```

Raw cargo equivalents work too — the Makefile just wraps them:

```bash
cargo build --release
cargo test --all-targets --all-features
cargo clippy --all-targets --all-features -- -D warnings
```

Requires the Rust toolchain pinned in `rust-toolchain.toml` (1.88). rustup picks it up automatically on `cd`.

## Running

```bash
make run -- --version           # forwards args to cargo run
make run -- doctor              # validate config
make run -- sessions            # list sessions
make run -- exec "..."          # one-shot task
make run -- resume <id> "..."   # resume a session
```

Or directly via the installed binary: `crow --version`, `crow doctor`, etc.

Live Nemotron requires `NVIDIA_API_KEY` (or `CROW_API_KEY`) in the environment. The repository ships with a scripted mock provider so deterministic tests run without network access.

## Trust model

Crow is autonomous. There is no permission engine or confirmation prompt. Run it only in environments where user-level command execution is acceptable. Cancellation, command timeouts, bounded output, atomic file replacement, log redaction, and project-root confinement are still enforced.

## Spec

The full design specification lives in the `ferre-z/ob-vault` repo at `30 Projects/Agent & ecosystem/08-Personal-Agent-v0-Spec.md`. It is the source of truth for behavior, dependencies, message/event schemas, failure handling, tests, and acceptance criteria.
