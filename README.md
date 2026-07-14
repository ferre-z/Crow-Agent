# Crow

A small autonomous coding agent in Rust. Mirrors the workflow of Pi, Claude Code, Codex CLI, and OpenCode: receive a task, reason against a large language model, call tools, observe results, modify a repository, and continue until the task is answered.

The first milestone is a reliable personal tool, not a platform. Keep the kernel small and extensible.

## Status

**v0 in active development.** Current shipped slice:

- Cargo binary crate, Rust toolchain pinned via `rust-toolchain.toml`
- CI on GitHub Actions (`cargo fmt`, `clippy -D warnings`, `cargo test`)
- Provider-neutral event accumulator and message types (`src/event.rs`, `src/message.rs`)
- `genai = 0.6.5` provider adapter against NVIDIA Nemotron 3 Ultra (`src/provider.rs`)
- Project-root-confined tool registry with the `read` tool shipped (`src/tool/`)
- Agent state machine and tool-call loop (`src/agent.rs`)
- Hierarchical `AGENTS.md` discovery + context compiler (`src/context.rs`)

In progress: headless CLI entry, JSONL session persistence, remaining tools (`write`, `edit`, `bash`), Ratatui TUI.

## Stack

| Concern | Choice |
|---|---|
| Async runtime | `tokio` + `tokio-util` |
| Model client | `genai = 0.6.5` (OpenAI-compatible NVIDIA endpoint) |
| Serialization | `serde`, `serde_json`, `schemars` |
| CLI | `clap` |
| TUI | `ratatui`, `crossterm`, `tui-textarea` |
| Errors | `thiserror`, `anyhow` |
| Diagnostics | `tracing`, `tracing-subscriber` |
| IDs | `ulid` |
| Secrets | `secrecy` |
| Tests | `tempfile`, scripted mock provider |

## Architecture

One binary crate. Module boundaries (from `src/`):

```
cli.rs        # clap entry + subcommands
config.rs     # layered config (CLI > env > user > defaults)
ids.rs        # session, run, message, tool-call IDs (ULID)
message.rs    # provider-neutral conversation data
event.rs      # AgentEvent + SessionEntry
provider.rs   # Provider trait + genai adapter
agent.rs      # state machine, tool-call loop, limits, cancellation
context.rs    # system prompt + AGENTS.md discovery
session.rs    # JSONL writer/reader, list, resume
tool/
  mod.rs      # Tool trait, registry, limits
  path.rs     # project-root path resolution
  read.rs     # (shipped) line-numbered read
  write.rs    # (pending) atomic file write
  edit.rs     # (pending) exact-string replacement
  bash.rs     # (pending) shell execution with timeout + truncation
tui/          # (pending) Ratatui frontend
tests/        # deterministic scripted-provider integration tests
```

## Building

```bash
cargo build --release
cargo test --all-targets --all-features
```

Requires Rust toolchain pinned in `rust-toolchain.toml`.

## Running

```bash
cargo run --release                    # open TUI in cwd
cargo run --release -- sessions list   # list sessions
cargo run --release -- --resume <id>   # resume a session
cargo run --release -- doctor          # validate config + endpoint
```

Live Nemotron requires `NVIDIA_API_KEY` in the environment. The repository ships with a scripted mock provider so deterministic tests run without network access.

## Trust model

Crow is autonomous. There is no permission engine or confirmation prompt. Run it only in environments where user-level command execution is acceptable. Cancellation, command timeouts, bounded output, atomic file replacement, log redaction, and project-root confinement are still enforced.

## Spec

The full design specification lives in the `ferre-z/ob-vault` repo at `30 Projects/Agent & ecosystem/08-Personal-Agent-v0-Spec.md`. It is the source of truth for behavior, dependencies, message/event schemas, failure handling, tests, and acceptance criteria.
