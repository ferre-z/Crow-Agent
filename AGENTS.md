# Repository Guidelines

This file is for AI coding agents working in this repository. It assumes no
prior knowledge of the project.

## Project Overview

Crow is a multihost agent suite built on the
[pi](https://github.com/earendil-works/pi) agent framework
(`@earendil-works/pi-ai` + `@earendil-works/pi-agent-core`, consumed as plain
npm dependencies — never fork or patch upstream; customize via
extensions/skills/config only). It is a pnpm monorepo, TypeScript strict, ESM
with NodeNext resolution everywhere.

Crow ships three runtime pieces:

- **`crowd`** (`packages/daemon`, `@crow/daemon`) — a daemon that runs on every
  host you control. It embeds pi agent sessions (tools, skills, MCP) and
  exposes a WebSocket JSON-RPC control API with bearer-token auth.
- **Crow Desktop** (`apps/desktop`, `@crow/desktop`) — an Electron hub app
  (electron-vite, React 19, Tailwind 4) that connects to many daemons at once:
  chat, fleet view, tool-call approvals, per-host sessions.
- **`crow`** (`apps/cli`, `@crow/cli`) — a thin CLI that talks to any daemon,
  local or remote.

Development is phase-planned (see `docs/architecture.md`): P0 scaffold → P1
core+daemon → P2 desktop + `@crow/client` + approvals → P3 multihost fleet +
CLI are committed, as is the P4 backend (sub-agents `agent.spawn` and teams
`team.list`/`team.run` in `@crow/core` + `@crow/daemon`). Next up: P5 A2A
daemon-to-daemon, P6 workflows+cron, P7 memory, P8 distribution. The README's
`skills/` and `scripts/` directories are planned (P8) and do not exist yet.

## Project Structure & Module Organization

- `packages/protocol` (`@crow/protocol`) — single source of truth for the wire
  format: zod schemas, JSON-RPC 2.0 framing, `event.*` notifications,
  A2A types for later phases. Spec in `docs/protocol.md`.
- `packages/core` (`@crow/core`) — agent runtime on the pi SDK: session
  factory/manager (`session.ts`), default coding tools confined to a root
  directory (`tools/`, `env/confined-env.ts`), skill loading (`skills.ts`),
  model registry (`models.ts`), approval gate (`approvals.ts`), and the
  scripted mock provider used by all tests (`testing/faux.ts`, re-exported as
  `testing` from the package root).
- `packages/memory` (`@crow/memory`) — custom SQLite memory (episodic log,
  facts, FTS5). Currently a stub; filled in during P7.
- `packages/daemon` (`@crow/daemon`, binary `crowd`) — per-host WS server,
  token auth, session registry, config in `~/.crow/daemon.json`.
- `packages/client` (`@crow/client`) — typed WS JSON-RPC client shared by the
  desktop app and the CLI.
- `apps/desktop` — Electron app; `src/main` (connection manager, hosts
  store), `src/preload`, `src/renderer` (React UI), `src/shared`.
- `apps/cli` — `crow` thin client; saved hosts live in `~/.crow/hosts.json`.
- `docs/` — `architecture.md` (locked decisions, phase plan) and
  `protocol.md` (wire spec). Keep these in sync with behavior changes.

Unit tests are colocated as `*.test.ts` next to sources. Keep modules narrow;
expose public APIs through each package's `src/index.ts`. Workspace packages
export their TypeScript sources directly (`exports` points at `src/index.ts`)
— there is no compiled boundary between workspace packages, and Node 22's
built-in type stripping runs sources directly in development (no build step
needed to run the daemon or CLI from source).

## Build, Test, and Development Commands

- `pnpm install` — install all workspace deps (pnpm 9; Node ≥ 22.19 required
  by the pi packages, despite the looser `>=20` in the root engines field; CI
  uses Node 22)
- `pnpm check` — format:check + lint + typecheck + build + test (mirrors CI
  exactly; run it before every commit)
- `pnpm test` / `pnpm typecheck` / `pnpm lint` / `pnpm build` — recursive
  across packages
- `pnpm --filter @crow/<pkg> <script>` — run in one package
- `pnpm format` — apply Prettier
- Run the daemon: `pnpm --filter @crow/daemon start` (defaults: port 7749,
  host 127.0.0.1, data dir `~/.crow`; a token is generated on first run into
  `~/.crow/daemon.json`)
- Run the desktop app: `pnpm --filter @crow/desktop dev`
- Run the CLI from source: `node apps/cli/src/bin.ts <args>`

There is no deployment process yet — distribution (one-line installer) is P8
and uncommitted. CI (`.github/workflows/ci.yml`) is a single job: install with
`--frozen-lockfile`, then `pnpm check`, on pushes to `main` and all PRs.

## Coding Style & Naming Conventions

Prettier (100 col, double quotes, semicolons, trailing commas) and the ESLint
flat config govern style. `verbatimModuleSyntax` is on, so use `import type`
for type-only imports (ESLint enforces inline type imports via
`consistent-type-imports`). Unused vars are an error unless prefixed with `_`.

TS config highlights (`tsconfig.base.json`): strict, `noUncheckedIndexedAccess`,
`noImplicitOverride`, `allowImportingTsExtensions`. Relative imports between
source files use explicit `.ts` extensions.

Idiomatic TS naming: `camelCase` for functions/vars, `PascalCase` for
types/classes, `SCREAMING_SNAKE_CASE` for constants. Prefer `zod` schemas at
every process/network boundary.

## Testing Guidelines

Vitest everywhere (no config files; each package runs `vitest run`). Tests are
colocated `*.test.ts` files named after observable behavior
(`rejects_session_on_bad_token`). Keep tests deterministic: script LLM
responses with the faux provider from `@crow/core` (`testing.makeFauxModels()`,
`testing.fauxAssistantMessage()`, `testing.fauxToolCall()`, …) — no live API
keys, no network. Daemon/client/CLI tests spin up a real in-process `crowd`
against the faux provider. Use temp dirs for any filesystem/SQLite state.

## Wire Protocol Essentials

Transport is WebSocket with newline-delimited JSON-RPC 2.0 frames. Auth is a
bearer token in the `Authorization` header at WS upgrade time; a rejected
upgrade answers HTTP 401 before any frames flow. Error codes: unknown method
`-32601`, bad params `-32602`, unknown session `-32002`, prompt on busy
session `-32003`, unparseable line `-32700`, invalid frame `-32600`. Tool-call
approvals: in `approvalMode: "ask"` the daemon pauses tool calls and waits for
an `approval.respond` notification from an attached client (120 s timeout,
deny-by-default). Full spec: `docs/protocol.md`.

## Security & Configuration

Never commit API keys, tokens, session data, or SQLite state files. Daemon
tokens live in `~/.crow/daemon.json` (mode 0600, auto-generated on first run,
never logged; a corrupt config is a loud error, not a silent rotation). CLI
hosts live in `~/.crow/hosts.json` (mode 0600). Preserve token auth on the WS
API and path confinement in tools when changing daemon/core code: the
`ConfinedExecutionEnv` in `packages/core/src/env/confined-env.ts` rejects any
path resolving outside the session root (known gap: confinement is syntactic
— symlinks inside the root can still escape; hardening is deferred).

## Commit & Pull Request Guidelines

Conventional-Commit subjects (`feat(<scope>): …`, `fix(<scope>): …`,
`chore: …`, `docs: …`, `test(<scope>): …`), ≤72 chars, body explains the why.
Scopes: `protocol`, `core`, `memory`, `daemon`, `client`, `desktop`, `cli`,
`repo`. One commit per logical unit. Do not push unless the owner asks.

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
