# Crow architecture (v0)

## Locked decisions

| Decision       | Choice                                              | Rationale                                                                                            |
| -------------- | --------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| Desktop shell  | Electron                                            | Same Node runtime as pi — daemon and desktop share code, no sidecar bridge                           |
| pi consumption | npm deps (`@earendil-works/pi-ai`, `pi-agent-core`) | Clean upgrades, no fork maintenance. Customization via extensions/skills/config only                 |
| Topology       | Desktop as hub                                      | Desktop connects directly to every daemon over WS JSON-RPC; A2A daemon-to-daemon layered on top (P5) |
| Language       | TypeScript everywhere                               | One toolchain; pi is TS-native                                                                       |
| Tests          | Scripted mock provider                              | Deterministic, no API key (mirrors pi's fixtures approach)                                           |

## Component map

```
apps/desktop (Electron, hub)
   │  WS JSON-RPC (token auth)          ┌───────────────┐
   ├──────────────► packages/daemon ◄──►│ pi sessions    │ (@crow/core wraps
   │   (one crowd per host)             │ tools/skills/  │  pi-ai + pi-agent-core)
   │                                    │ MCP            │
   │  A2A over HTTP (P5)                └───────────────┘
   └──────────────► crowd (host B)
```

- `@crow/protocol` is the single source of truth for the wire format. Both
  daemon and desktop import it — no drift.
- `@crow/core` owns everything agent-shaped: session factory, default coding
  tools, skill loader (`~/.crow/skills` + project `.crow/skills`), MCP client
  config, sub-agent runner, team presets.
- `@crow/daemon` owns everything host-shaped: WS server, auth, session
  registry, scheduler (cron + workflows), A2A endpoint, host info, policy
  (approval rules).
- `@crow/memory` owns persistence beyond sessions: episodic log, facts,
  FTS5 search; embeddings behind an interface, off by default.

## Phase plan

P0 scaffold → P1 core+daemon MVP → P2 desktop MVP → P3 multihost fleet + CLI →
P4 sub-agents/teams → P5 A2A → P6 workflows+cron → P7 memory → P8 distribution.

Historical context: the previous Rust kernel (`archive/crow-rust-v0/`) and the
`pi-crow/` vendor fork were removed at the fresh-start commit; both remain in
git history for reference only.
