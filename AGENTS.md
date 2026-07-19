# Repository Guidelines

## Project Structure & Module Organization

Crow is a pnpm monorepo (TypeScript strict, ESM/NodeNext). Shared wire types
live in `packages/protocol` (`@crow/protocol`); the agent runtime on the pi
SDK lives in `packages/core` (`@crow/core`); custom memory in
`packages/memory` (`@crow/memory`); the per-host daemon in `packages/daemon`
(`@crow/daemon`, binary `crowd`); the typed daemon client in
`packages/client` (`@crow/client`). Apps live under `apps/` (`desktop`
Electron hub, `cli` thin client). Bundled skills live in `skills/`, design
history and the protocol spec in `docs/`, utility scripts in `scripts/`.

Unit tests are colocated as `*.test.ts` next to sources. Keep modules narrow;
expose public APIs through each package's `src/index.ts`.

## Build, Test, and Development Commands

- `pnpm install` — install all workspace deps (Node ≥ 20, pnpm 9)
- `pnpm check` — format:check + lint + typecheck + build + test (mirrors CI)
- `pnpm test` / `pnpm typecheck` / `pnpm lint` / `pnpm build` — recursive across packages
- `pnpm --filter @crow/<pkg> <script>` — run in one package
- `pnpm format` — apply Prettier

Run `pnpm check` before committing. Tests use a scripted mock provider and
require neither network access nor an API key.

## Coding Style & Naming Conventions

Prettier (100 col, double quotes, semicolons) and ESLint flat config govern
style; `verbatimModuleSyntax` is on, so use `import type` for type-only
imports (enforced as inline type imports). Idiomatic TS naming: `camelCase`
for functions/vars, `PascalCase` for types/classes, `SCREAMING_SNAKE_CASE`
for constants. Prefer `zod` schemas at every process/network boundary.

## Testing Guidelines

Vitest everywhere. Name tests after observable behavior
(`rejects_session_on_bad_token`). Keep tests deterministic — scripted mock
LLM provider, fake timers for the scheduler, temp dirs for SQLite. No live
API keys in tests.

## Commit & Pull Request Guidelines

Conventional-Commit subjects (`feat(<scope>): …`, `fix(<scope>): …`,
`chore: …`, `docs: …`, `test(<scope>): …`), ≤72 chars, body explains the why.
Scopes: `protocol`, `core`, `memory`, `daemon`, `desktop`, `cli`, `repo`.
One commit per logical unit. Do not push unless the owner asks.

## Security & Configuration

Never commit API keys, tokens, session data, or SQLite state files. Daemon
tokens live in `~/.crow/daemon.json` (mode 0600). Preserve token auth on the
WS API and path confinement in tools when changing daemon/core code.

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
9. **The user reviews on their box; we test what we can locally.** Per-feature commit so the user always has something concrete to look at.

### One-line install is the contract (P8)

10. **`curl .../install.sh | sh` must work on a clean Linux/macOS box with zero manual setup.** No "first install X". The script auto-installs what it needs.
11. **Disk + memory awareness are first-class.** A script that fills `/home` to 100% on a quota-bound box is broken even if it "works" elsewhere. Probe `df` and `/proc/meminfo` before large writes.
12. **Idempotent.** Re-running the installer upgrades in place; re-running tests is deterministic.

### Commit cadence

13. **Commit at every meaningful change**, not at the end of a long session. One commit per logical unit (one feature, one bug fix, one cleanup, one docs pass).
14. **Conventional-Commit subject** on every commit. Keep the subject ≤72 chars; the commit body explains the why.
15. **Tests run before commit.** `pnpm check` must pass. If a test is broken, fix the test, don't bypass.
16. **Push only when the owner asks.** No speculative pushes.
17. **No `// FIXME: remove before merge`.** Stub code in a committed PR is a bug with extra steps.

### Discovery before work

18. **Audit before designing.** For every new feature or refactor on an unfamiliar area, read the affected files end-to-end first. No proposing changes to code you haven't read.
19. **Map the project against a real reference** when stuck. Clone the closest comparable OSS project, study it, take notes, then redesign. Don't reinvent against stale memory.

### Self-honesty

20. **If you broke something, say so directly.** No hedging, no "it's likely unrelated". State the failure mode and the fix.
21. **Don't ship code you couldn't run.** If the env prevents the full gate, ship a smaller feature with a clear note about what wasn't verified, then add a follow-up todo to verify on next round.
22. **Update this file when the user corrects you.** If they say "stop rushing", capture it as a rule above. The owner is the source of truth for how work should happen here.
