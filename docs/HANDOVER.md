# Crow — Agent Handover

You are an AI agent taking over the Crow codebase. Read `AGENTS.md` first
(conventions + the owner's non-negotiable operating rules), then this file,
then the plan for your assigned task in `docs/plans/`.

## What Crow is

A multihost agent suite on the pi framework (`@earendil-works/pi-ai` +
`pi-agent-core`, consumed as plain npm deps — never fork or patch upstream):

- **`crowd`** (`packages/daemon`) — per-host daemon. WS JSON-RPC control API
  (bearer token), pi sessions, approvals, sub-agents/teams, A2A HTTP
  delegation, workflows + cron, memory.
- **Crow Desktop** (`apps/desktop`) — Electron hub; connects to many daemons
  (fleet view, chat, approvals, team runs).
- **`crow`** (`apps/cli`) — thin CLI client.

## Current state (as of last commit on `main`)

Phases P0–P8 are all committed. Root gate `pnpm check` is green:
200 tests (protocol 19, core 34, memory 5, daemon 30, client 15, desktop 62,
cli 18). `electron-vite build` produces working bundles; the app was launched
headless under xvfb and stays up.

Wire spec: `docs/protocol.md` (keep it in sync when you change the wire).
Architecture: `docs/architecture.md`.

## Verification matrix — what is proven vs assumed

| Area                                | State                                                                                       |
| ----------------------------------- | ------------------------------------------------------------------------------------------- |
| WS wire, auth, sessions, approvals  | Proven — live-daemon tests + manual smoke                                                   |
| Sub-agents, teams                   | Proven — tests + events verified                                                            |
| A2A delegation                      | Proven — real two-daemon test locally                                                       |
| Workflows + cron                    | Tested — daemon tests incl. backdated-tick fire                                             |
| Memory (SQLite+FTS5)                | Tested — 5 store tests + daemon wire test                                                   |
| Desktop connect flow                | Fixed after user report; NOT re-verified on a real display — needs the deep-test pass below |
| Electron packaging (`pnpm package`) | Config written, NEVER RUN — see plan 08                                                     |
| `scripts/install.sh`                | Syntax-checked only, NEVER RUN on a clean box — plan 08                                     |
| CLI                                 | 18 tests incl. spawned-bin e2e; not manually used by a human                                |
| A2A across real machines            | Only loopback tested — shared-token assumption (plan 09)                                    |

## Known gaps (each has a plan in `docs/plans/`)

1. **MCP is absent entirely.** The original plan promised MCP support in
   `@crow/core` ("MCP client config"). pi ships no MCP client; nothing in the
   repo mentions MCP. → `docs/plans/01-mcp-integration.md`
2. **Desktop has no workflow/cron UI.** P6 backend is done; the desktop only
   has chat + teams. → `docs/plans/02-desktop-workflows-cron-ui.md`
3. **Desktop has no memory browser.** P7 backend is done; no UI. → `docs/plans/03-desktop-memory-browser.md`
4. **`session.attach` has no replay buffer.** Late-attaching clients miss
   earlier output; `since` is accepted and ignored. → `docs/plans/04-session-replay-buffer.md`
5. **CLI lacks workflow/cron/memory commands.** Only sessions/hosts/prompt.
   → `docs/plans/05-cli-workflows-cron-memory.md`
6. **Path confinement is syntactic.** Symlinks inside the root escape it.
   → `docs/plans/06-confinement-symlink-hardening.md`
7. **WS/A2A auth is a single static token over plaintext.** TLS + per-host
   authz are open. → `docs/plans/07-tls-and-authz.md`
8. **Distribution unverified.** electron-builder config + install.sh never
   executed; no release CI. → `docs/plans/08-installer-and-release-ci.md`
9. **A2A delegation reuses the local token.** Works only when daemons share
   a token; agent cards are minimal. → `docs/plans/09-a2a-per-host-tokens.md`
10. **Workflow defs are JSON-only; cron grammar is a subset** (`@every`,
    `@hourly`, `@daily HH:MM`). Plan promised YAML/TS defs + full cron.
    → `docs/plans/10-workflow-dsl-and-cron-grammar.md`

## How to run everything (dev)

Node ≥ 22.19, pnpm 9.

```bash
pnpm install
pnpm check                                   # full gate — must be green before any commit

pnpm --filter @crow/daemon start             # daemon on ws://127.0.0.1:7749 (token in ~/.crow/daemon.json)
pnpm --filter @crow/desktop dev              # Electron hub
node apps/cli/src/bin.ts prompt "hi" --url ws://127.0.0.1:7749 --token "$(jq -r .token ~/.crow/daemon.json)"
```

Daemon flags worth knowing: `--a2a-port N` (A2A HTTP), `--skill-dir PATH`
(skills on every session), `--public-base-url URL`.

## Deep-test checklist (owner runs this on a real machine)

Before declaring the current state "good", run this end-to-end:

1. Fresh `pnpm install` on a clean profile → Electron binary auto-downloads
   (postinstall fix). `pnpm --filter @crow/desktop dev` launches with no
   `bufferutil` error.
2. Daemon up → desktop: add host, wrong token → red auth error; fix token →
   connects. Kill daemon → sidebar shows disconnected + error; restart →
   reconnect button works. (This was the reported bug; the fix is committed
   but unverified on a display.)
3. Create session (ask approvals) → prompt that triggers a tool → approval
   modal → allow/deny/always behave. Cancel mid-stream → "cancelled" state.
4. Two daemons → both in fleet; same-named sessions stay distinct.
5. Team run (`plan-implement-review`) → step timeline completes.
6. `crow prompt` one-shot streams tokens; `crow sessions`, `crow info`.
7. Memory: run a session → new session in same cwd gets the memory block
   (check daemon's composed system prompt via a logging run, or query
   `memory.episodes` and confirm the episode exists).

## Working rules for the next agent

- Every plan in `docs/plans/` is self-contained: goal, current-state file
  pointers, design, wire changes, implementation steps, tests, acceptance.
- Follow `AGENTS.md`: conventional commits, `pnpm check` green before every
  commit, deterministic tests (faux provider, no API keys), one commit per
  logical unit, push only when the owner asks.
- When a plan changes the wire, update `docs/protocol.md` in the same commit.
- When you finish a plan, update this matrix and mark the plan done.
