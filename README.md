# Crow

A small autonomous coding agent in Rust. Mirrors the workflow of Pi, Claude Code, Codex CLI, and OpenCode: receive a task, reason against a large language model, call tools, observe results, modify a repository, and continue until the task is answered.

The first milestone is a reliable personal tool, not a platform. Keep the kernel small and extensible.

## Quick start

Install (one line):

```bash
curl -sSf https://raw.githubusercontent.com/ferre-z/Crow-Agent/main/scripts/install.sh | sh
```

The installer clones the repo, builds a release binary, and drops `crow` into `~/.cargo/bin/crow`. **It auto-installs any missing dependencies** — Rust via rustup, basic build tools via your system package manager — so it works on a clean box with no prior setup. Linux and macOS only (Windows is blocked upstream by the `nix` crate; pass `--no-bootstrap` to opt out).

Test from a fresh clone (one line):

```bash
git clone https://github.com/ferre-z/Crow-Agent.git /tmp/crow && cd /tmp/crow && make test
```

The test suite is fully deterministic — no API key, no network required.

Try it:

```bash
crow --version
crow doctor
```

Other useful targets: `make build`, `make lint`, `make install`, `make smoke`, `make ci`. Run `make help` for the full list.

## Status

**v0 in active development.** Shipped so far:

- Cargo binary crate, Rust toolchain pinned via `rust-toolchain.toml`
- CI on GitHub Actions (`cargo fmt`, `clippy -D warnings`, `cargo test`)
- Provider-neutral event accumulator and message types (`src/event.rs`, `src/message.rs`)
- `genai = 0.6.5` provider adapter against NVIDIA Nemotron 3 Ultra (`src/provider/genai.rs`)
- Project-root-confined tool registry with `read`, `write`, `edit`, `bash` shipped (`src/tool/`)
- Agent state machine + tool-call loop with stop-reason handling, event sink, and durable `RunFailed` records (`src/agent.rs`)
- Hierarchical `AGENTS.md` discovery + context compiler (`src/context.rs`)
- Layered config (CLI > env > user file > defaults) + clap CLI with `exec`, `sessions`, `resume`, `doctor` subcommands (`src/cli.rs`, `src/config.rs`)
- Session recovery: trailing-sequence recovery, stale-lock eviction, crash-tail detection (`src/session.rs`)
- `Agent::resume_into` for `crow --resume <id>` (`src/agent.rs`)

In progress: app-server (`crow serve`), Tauri 2 desktop shell, approval cards, plan mode, OS keyring.

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
cli.rs        # clap entry + subcommands (exec, sessions, resume, doctor)
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
tool/
  mod.rs      # Tool trait, registry, ToolSpec, limits
  path.rs     # project-root path resolution
  read.rs     # line-numbered read (sniff-then-read, bounded)
  write.rs    # atomic temp+rename
  edit.rs     # exact-match replacement with diff summary
  bash.rs     # shell exec with process-group kill on timeout
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
