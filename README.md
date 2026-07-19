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

Early scaffold. See `docs/` for the architecture and protocol spec. Build
order follows the phase plan: P0 scaffold → P1 core+daemon MVP → P2 desktop
MVP → P3 multihost fleet → P4 sub-agents/teams → P5 A2A → P6 workflows+cron →
P7 memory → P8 distribution.

## Repository layout

```
packages/
  protocol/   @crow/protocol — wire types (zod), JSON-RPC + A2A schemas
  core/       @crow/core     — agent runtime on the pi SDK: sessions, tools,
                               skills, MCP, sub-agents, teams
  memory/     @crow/memory   — SQLite memory: episodic log, facts, FTS5
  daemon/     @crow/daemon   — `crowd` per-host daemon
apps/
  desktop/    Electron hub app (P2)
  cli/        `crow` thin client (P3)
skills/       bundled Crow skills
docs/         architecture decisions, protocol spec
scripts/      installer (P8)
```

## Development

Requires Node ≥ 20 and pnpm 9.

```bash
pnpm install
pnpm check      # format:check + lint + typecheck + build + test
```

Per-package: `pnpm --filter @crow/<pkg> test|typecheck|build`.

## Conventions

- TypeScript strict, ESM, NodeNext resolution everywhere.
- pi is a pure dependency — customization via extensions/skills/config,
  never patches to upstream code.
- Tests run against a scripted mock provider; no API key needed.
- Conventional Commits (`feat(daemon): …`, `fix(core): …`).
