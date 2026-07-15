# Repository Guidelines

## Project Structure & Module Organization

Crow is a single Rust 2021 crate with a library and the `crow` binary. Core behavior lives in `src/agent.rs`; messages and events are in `src/message.rs` and `src/event.rs`; provider adapters are under `src/provider/`; and project-confined tools are under `src/tool/`. Session persistence, cancellation, context compilation, and typed IDs have dedicated modules. The system prompt is in `prompts/system_prompt.md`.

Unit tests are colocated in `#[cfg(test)]` modules. Cross-module integration coverage lives in `tests/`, with scripted JSONL inputs in `tests/fixtures/`. Design history, implementation briefs, and decisions are documented under `docs/`. Utility scripts belong in `scripts/`.

## Build, Test, and Development Commands

- `cargo build` compiles a debug build using the pinned Rust 1.88 toolchain.
- `cargo run -- <args>` runs the local CLI; use `cargo run --release` for an optimized build.
- `cargo test --all-targets --all-features` runs the complete deterministic test suite.
- `cargo fmt --all -- --check` verifies formatting without changing files.
- `cargo clippy --all-targets --all-features -- -D warnings` applies the repository's lint policy and treats warnings as failures.

Run all three checks before opening a pull request. Tests use a scripted mock provider and require neither network access nor `NVIDIA_API_KEY`.

## Coding Style & Naming Conventions

Use `rustfmt` formatting (four-space indentation) and idiomatic Rust naming: `snake_case` for modules, functions, and tests; `CamelCase` for types and traits; and `SCREAMING_SNAKE_CASE` for constants. Keep modules narrow and expose shared APIs through `src/lib.rs` or the relevant `mod.rs`. Prefer typed library errors with `thiserror`.

## Testing Guidelines

Use `#[test]` for synchronous logic and `#[tokio::test]` for async behavior. Name tests after observable behavior, for example `read_rejects_path_outside_root`. Add regression tests alongside the affected module; reserve `tests/` for public, cross-module workflows. Extend fixtures when provider event sequences are important, keeping them deterministic.

## Commit & Pull Request Guidelines

History generally follows concise Conventional Commit-style subjects such as `feat(agent): ...`, `test(provider): ...`, and `chore(agent): ...`; merge commits use `merge: task ...`. Keep commits focused and use an accurate module scope. Pull requests should explain the behavior change, link the relevant issue or task brief, note tests run, and call out configuration or security implications. Include terminal output or screenshots only when CLI/TUI behavior changes materially.

## Security & Configuration

Never commit API keys, session data, or generated `.review-packages/`. Live provider use expects `NVIDIA_API_KEY`; keep secrets in the environment. Preserve project-root path confinement and cancellation/timeout bounds when changing tools.
