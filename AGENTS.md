# Repository Guidelines

## Project Structure & Module Organization

Crow is a single Rust 2021 crate with a library and the `crow` binary. Core behavior lives in `src/agent.rs`; messages and events are in `src/message.rs` and `src/event.rs`; provider adapters are under `src/provider/`; and project-confined tools are under `src/tool/`. Session persistence, cancellation, context compilation, and typed IDs have dedicated modules. The system prompt is in `prompts/system_prompt.md`.

Unit tests are colocated in `#[cfg(test)]` modules. Cross-module integration coverage lives in `tests/`, with scripted JSONL inputs in `tests/fixtures/`. Design history, implementation briefs, and decisions are documented under `docs/`. Utility scripts belong in `scripts/`.

## Build, Test, and Development Commands

A Makefile wraps the standard cargo invocations so a single `make <target>` runs them:

- `make help` — list every target
- `make test` — runs the deterministic test suite (no API key needed)
- `make build` — release build into `target/release/crow`
- `make lint` — clippy with `-D warnings`
- `make fmt` / `make fmt-check` — apply / verify rustfmt
- `make install` — **debug** build + copy `~/.cargo/bin/crow` + `cargo clean`. Small disk footprint; the optimized equivalent is `make install-release`
- `make install-release` — `cargo install --path . --locked` (release build, ~600 MiB peak)
- `make smoke` — release build + `crow --version && crow doctor`
- `make ci` — `fmt-check + lint + build + test` (mirrors `.github/workflows/ci.yml`)

Underlying cargo equivalents (still work directly):

- `cargo build` compiles a debug build using the pinned Rust 1.88 toolchain.
- `cargo run -- <args>` runs the local CLI; use `cargo run --release` for an optimized build.
- `cargo test --all-targets --all-features` runs the complete deterministic test suite.
- `cargo fmt --all -- --check` verifies formatting without changing files.
- `cargo clippy --all-targets --all-features -- -D warnings` applies the repository's lint policy and treats warnings as failures.

Run `make ci` (or all three cargo checks) before opening a pull request. Tests use a scripted mock provider and require neither network access nor `NVIDIA_API_KEY`.

## One-line scripts

- `bash scripts/install.sh` — clone, build, install to `~/.cargo/bin`.
  **Auto-bootstraps missing dependencies**: Rust via rustup if `cargo`
  is missing; basic build tools (`git`, `make`, `curl`) via the
  system package manager (apt / dnf / pacman / zypper / apk / brew)
  when missing. Linux + macOS only. Pass `--no-bootstrap` to skip
  auto-install.
- `bash scripts/test.sh` — wrapper around `make test`, usable from any cwd.

## Coding Style & Naming Conventions

Use `rustfmt` formatting (four-space indentation) and idiomatic Rust naming: `snake_case` for modules, functions, and tests; `CamelCase` for types and traits; and `SCREAMING_SNAKE_CASE` for constants. Keep modules narrow and expose shared APIs through `src/lib.rs` or the relevant `mod.rs`. Prefer typed library errors with `thiserror`.

## Testing Guidelines

Use `#[test]` for synchronous logic and `#[tokio::test]` for async behavior. Name tests after observable behavior, for example `read_rejects_path_outside_root`. Add regression tests alongside the affected module; reserve `tests/` for public, cross-module workflows. Extend fixtures when provider event sequences are important, keeping them deterministic.

## Commit & Pull Request Guidelines

History generally follows concise Conventional Commit-style subjects such as `feat(agent): ...`, `test(provider): ...`, and `chore(agent): ...`; merge commits use `merge: task ...`. Keep commits focused and use an accurate module scope. Pull requests should explain the behavior change, link the relevant issue or task brief, note tests run, and call out configuration or security implications. Include terminal output or screenshots only when CLI/TUI behavior changes materially.

## Security & Configuration

Never commit API keys, session data, or generated `.review-packages/`. Live provider use expects `NVIDIA_API_KEY`; keep secrets in the environment. Preserve project-root path confinement and cancellation/timeout bounds when changing tools.

---

## Working with this Repository — Operating Rules

These rules come from the project owner (Ferre) and are non-negotiable for any AI agent touching this codebase. They override any "ship a quick patch" instinct. Update this section freely as the owner adds new rules.

### Tempo and quality

1. **Quality over speed.** This is not a race. There is no deadline except "done right".
2. **Unlimited time and tokens.** Do not truncate work to save round-trips. Do not skip tests because "the user is waiting". The user would rather wait than receive broken code.
3. **Work until perfection, not until "good enough".** "Good enough" is the bug you ship on Friday. If a step is half-done, finish it before moving on. No `// TODO` carried forward.
4. **No rushing.** Slow down on every change. Read your own diffs before committing. If something feels off, stop and verify.
5. **Benchmark against real results, not vibes.** Don't mark a step complete because "I think it works" — run it, see it pass, then close the task.
6. **Don't self-review cyclically.** Stop re-litigating completed work. Audit once, write findings, move to the next step. Revisit only when new evidence warrants.

### Task discipline

7. **One task = one granular todo.** A todo should be testable in isolation. "Design install.sh replacement" is fine; "make Crow better" is not.
8. **No "as a smoke test" / "I'll verify later" commits.** Run the gates locally (lint, test, build, whatever applies) before committing. If you can't run it locally because the environment is hostile, surface that explicitly and stop — don't commit unfinished work.
9. **The user reviews on their box; we test what we can locally.** Per-feature commit + push so the user always has something concrete to look at.

### One-line install is the contract

10. **`curl .../install.sh | sh` must work on a clean Linux/macOS box with zero manual setup.** No "first install rustup", no "first apt install X". The script auto-installs what it needs.
11. **Prebuilt binary first; source build only as fallback.** End users should never compile Rust from source via the installer — that's a developer-time path. Release binaries come from CI artefacts (`.github/workflows/release.yml`).
12. **Disk + memory awareness are first-class.** A script that fills `/home` to 100% on a quota-bound box is broken even if it "works" elsewhere. Probe `df` and `/proc/meminfo` before large writes.
13. **Idempotent.** Re-running the installer upgrades in place; re-running the doctor is cheap; re-running tests is deterministic.

### Per-feature commit and push cadence

14. **Commit at every meaningful change**, not at the end of a long session. One commit per logical unit (one feature, one bug fix, one cleanup, one docs pass).
15. **Push after every commit** via `git push origin main`. Do not accumulate local commits — the user wants a per-feature green build visible on `main`.
16. **Conventional-Commit subject** on every commit: `feat(<scope>): ...`, `fix(<scope>): ...`, `chore: ...`, `docs: ...`, `test(<scope>): ...`. Keep the subject ≤72 chars; the commit body explains the why.
17. **Tests run before commit.** `make fmt-check && make lint && make test` must pass. If a test is broken, fix the test, don't bypass.
18. **No `// FIXME: remove before merge`.** Stub code in a committed PR is a bug with extra steps.

### Discovery before work

19. **Audit before designing.** For every new feature or refactor on an unfamiliar area, read the affected files end-to-end first. No proposing changes to code you haven't read.
20. **Map the project against a real reference** when stuck. Clone the closest comparable OSS project, study its installer/build/release, take notes, then redesign. Don't reinvent against stale memory.
21. **User environment varies.** Ask `df -h /`, `free -h`, `quota -v` before designing install code. Don't assume the user's machine matches the agent's sandbox.

### Self-honesty

22. **If you broke something, say so directly.** No hedging, no "it's likely unrelated". State the failure mode and the fix.
23. **Don't ship code you couldn't run.** If the env prevents the full gate, ship a smaller feature with a clear note about what wasn't verified, then add a follow-up todo to verify on next round.
24. **Update this file when the user corrects you.** If they say "stop rushing", capture it as a rule above. The owner is the source of truth for how work should happen here.
