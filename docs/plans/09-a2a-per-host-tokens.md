# Plan 09 — A2A: per-host tokens and richer agent cards

**Goal:** daemon-to-daemon delegation works without a shared token, and
agent cards carry enough to route tasks safely.

## Current state

- Delegation (`delegateA2a` in `packages/daemon/src/server.ts`) calls the
  remote with the **local daemon's own token** — works only when both
  daemons share one (the test literally uses `"shared-token"`).
- Agent card (`GET /.well-known/agent.json` in `a2a-server.ts`): name,
  version, capabilities, model list, endpoint. No auth requirements, no
  rate/cost info, no signing.

## Design

1. **Per-host tokens**: `agent.spawn` params gain `hostToken?: string`
   (zod, optional) — forwarded to the remote's bearer check instead of the
   local token. Team presets' per-step `host` gains an optional
   `hostToken`. Desktop team/spawn forms get an optional "remote token"
   field. Document that v1 passes tokens explicitly; a fleet credential
   store is a follow-up.
2. **Card enrichment**: add `auth: { scheme: "bearer" }`, `limits:
{ maxConcurrentTasks: N }`, and the daemon's public hostname; a
   `GET /a2a/tasks` list endpoint (admin debugging).
3. **Backpressure**: A2A server caps concurrent running tasks (default 4);
   beyond that → `429 { error: "busy" }`. Client treats 429 as a retryable
   error (bounded retries in `CrowA2aClient.delegate`).

## Tests

- Delegation with distinct tokens succeeds when `hostToken` is supplied and
  fails (401 surfaced as `event.agent` error) without it.
- 429 path: fill the remote with long-running faux tasks, assert the cap.
- Card schema test.

## Acceptance

- Two daemons with different tokens delegate successfully via `hostToken`;
  `pnpm check` green; `docs/protocol.md` A2A section updated.
