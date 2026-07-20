# Crow

A multihost agent suite built on the [pi](https://github.com/earendil-works/pi)
agent framework (`@earendil-works/pi-ai` + `@earendil-works/pi-agent-core`,
consumed as plain npm dependencies).

Crow is three things:

- **`crowd`** — a daemon that runs on every host/server you control. It embeds
  pi agent sessions (tools, skills, MCP) and exposes a WebSocket JSON-RPC
  control API with token auth. It also owns scheduling (cron + workflows),
  custom memory, sub-agents/teams, and an A2A endpoint for daemon-to-daemon
  delegation.
- **Crow Desktop** — an Electron app that is the hub: it connects to every
  `crowd` you run and gives you chat, a fleet view, approvals, workflow/cron
  management, and memory browsing in one place.
- **`crow`** — a thin CLI that talks to any daemon (local or remote).

## Status

Committed so far:

- **P0** — monorepo scaffold, CI, docs
- **P1** — `crowd` daemon + `@crow/core` runtime (sessions, confined tools, skills)
- **P2** — `@crow/client` + Electron desktop hub + tool-call approvals
- **P3** — multihost fleet in the desktop app + `crow` CLI

See `docs/` for the architecture and protocol spec. Next: P4 sub-agents/teams.

## Repository layout

```
packages/
  protocol/   @crow/protocol — wire types (zod), JSON-RPC + A2A schemas
  core/       @crow/core     — agent runtime on the pi SDK: sessions, tools,
                               skills, MCP, sub-agents, teams
  memory/     @crow/memory   — SQLite memory: episodic log, facts, FTS5
  daemon/     @crow/daemon   — `crowd` per-host daemon
  client/     @crow/client   — typed WS JSON-RPC client (used by desktop + CLI)
apps/
  desktop/    Electron hub app (P2)
  cli/        `crow` thin client (P3)
skills/       bundled Crow skills
docs/         architecture decisions, protocol spec
scripts/      installer (P8)
```

## Local laptop setup

Requires **Node ≥ 22.19** (pi packages require it) and **pnpm 9**.

```bash
# 1. Clone
gh repo clone ferre-z/Crow-Agent && cd Crow-Agent

# 2. Install
pnpm install

# 3. Verify the gate (no API key needed; tests use a scripted fake provider)
pnpm check   # format:check + lint + typecheck + build + test
```

## Run the daemon

The daemon stores its config/token in `~/.crow/daemon.json` (mode `600`).

```bash
# Start a local crowd on the default port (7749)
pnpm --filter @crow/daemon start

# Or explicitly:
node packages/daemon/src/bin.ts --port 7749 --data-dir ~/.crow --token <your-token>
```

If you omit `--token`, one is generated for you. Read it from `~/.crow/daemon.json`
to configure the desktop app / CLI.

## Run the desktop app

```bash
pnpm --filter @crow/daemon start   # terminal 1
pnpm --filter @crow/desktop dev    # terminal 2
```

The Electron binary downloads automatically during `pnpm install` (workspace
`postinstall`). If it's ever missing (e.g. you installed with scripts
disabled), run `node apps/desktop/node_modules/electron/install.js` once.

Then in the desktop app add one or more hosts (e.g. `ws://127.0.0.1:7749`) with
the token from `~/.crow/daemon.json`. The fleet sidebar supports multiple
simultaneous hosts; chat sessions are scoped per host.

## Run the CLI

The CLI stores saved hosts in `~/.crow/hosts.json` (mode `600`).

```bash
# Save a host
crow hosts add local --url ws://127.0.0.1:7749 --token <token>

# One-shot prompt (creates a session, sends, streams until idle)
crow prompt "list the files here" --host local

# Or send to an existing session
crow sessions --host local
crow send <session-id> "what does package.json do?" --host local --wait

# Ad-hoc without saving
crow info --url ws://127.0.0.1:7749 --token <token>
crow prompt "hello" --url ws://127.0.0.1:7749 --token <token>
```

If `crow` is not on your PATH, use `node apps/cli/src/bin.ts <args>` from the
repo root (Node 22's type stripping runs the TS source directly).

## Tests

```bash
pnpm test            # all workspace packages
pnpm --filter @crow/core test
pnpm --filter @crow/daemon test
pnpm --filter @crow/desktop test
pnpm --filter @crow/client test
```

No API key is needed for the test suite; it uses a scripted faux provider from
`@crow/core/testing`.

## Manual smoke test

With a real daemon running:

```bash
# Daemon
curl -i -N \
  -H "Authorization: Bearer $(jq -r .token ~/.crow/daemon.json)" \
  -H "Connection: Upgrade" \
  -H "Upgrade: websocket" \
  -H "Sec-WebSocket-Key: $(openssl rand -base64 16)" \
  -H "Sec-WebSocket-Version: 13" \
  http://127.0.0.1:7749
```

Or use any WebSocket client and send NDJSON frames like:

```json
{"jsonrpc":"2.0","id":1,"method":"host.info","params":{}}\n
```

## Conventions

- TypeScript strict, ESM, NodeNext resolution everywhere.
- pi is a pure dependency — customization via extensions/skills/config,
  never patches to upstream code.
- Tests run against a scripted mock provider; no API key needed.
- Conventional Commits (`feat(daemon): …`, `fix(core): …`).
- Relative imports use `.ts` extensions; Node 22's built-in type stripping runs
  sources directly in development (no build step needed for dev).
