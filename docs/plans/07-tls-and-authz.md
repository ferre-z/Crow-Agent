# Plan 07 — TLS and authz for WS + A2A

**Goal:** encrypted transport and per-host authorization beyond a single
shared static token.

## Current state

- WS auth: bearer token compared in `verifyClient` (`packages/daemon/src/server.ts`);
  plaintext `ws://`.
- A2A HTTP: same bearer check (`a2a-server.ts`); plaintext `http://`.
- Tokens: one per daemon, in `~/.crow/daemon.json` (mode 0600).
- Desktop/CLI store tokens in `hosts.json` (mode 0600).

## Design (incremental)

1. **TLS**: `crowd` gains `--tls-cert PATH --tls-key PATH` → `https` +
   `wss` servers (Node `https`/`tls`, pass options to `WebSocketServer`).
   Client (`@crow/client`) gains `ca`/`rejectUnauthorized` options; desktop
   host entries gain a "self-signed ok" toggle. Also support `wss://` URLs in
   the hosts file (scheme already flows through).
2. **Token scopes** (cheap authz): daemon.json gains optional
   `tokens: [{ token, scopes: ["sessions","agents","cron","memory","admin"] }]`.
   Dispatch checks the connection's scopes per method group (map method →
   required scope in one table). Default single token = all scopes
   (backwards compatible).
3. Doc: recommend Tailscale for v1; TLS terminates at the daemon when
   exposed beyond loopback.

## Tests

- TLS: self-signed cert fixture (generate in-test with `openssl` via
  `child_process`, or check in a small test cert) — daemon over `wss`,
  client connects with `rejectUnauthorized: false`, bad CA fails.
- Scopes: a sessions-only token gets `-32001` on `cron.add`.

## Acceptance

- `wss://` works end-to-end; scoped tokens enforce per-method-group;
  `pnpm check` green; README + protocol doc updated.
