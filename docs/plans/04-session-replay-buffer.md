# Plan 04 — Session replay buffer

**Goal:** `session.attach` replays what it missed — a client connecting
mid-run (or reconnecting after a drop) sees the session's prior events.

## Current state

- `session.attach { sessionId, since? }` — `since` is **accepted and ignored**
  (comment in `packages/daemon/src/server.ts`, P1 decision).
- Sessions emit `event.token/thinking/tool_call/tool_result/session_state`
  only to currently-attached connections.
- Transcripts DO persist: pi's `JsonlSessionRepo` under `<dataDir>/sessions/`
  (see `packages/core/src/session.ts`).

## Design

Two options; pick **A** (simpler, matches the wire's append-only events).

- **A. In-memory ring buffer per session** (daemon-side): keep the last N
  events (e.g. 500) per session in `ensureSessionSubscription`; on
  `session.attach`, replay buffered events to the attaching connection
  before going live (with an `event.replay_start`/`event.replay_end` marker
  pair, or a `replayed: true` flag on replayed frames — extend protocol).
  `since` (ISO timestamp) filters the buffer; add timestamps to buffered
  entries.
- B. Rebuild from the JSONL transcript (durable across daemon restarts) —
  bigger job; the pi session entries are messages, not wire events, so you'd
  re-derive events. Defer; note as follow-up.

Also: the daemon currently has no `session.remove` — buffered events for
removed sessions must be dropped when a session is removed/closed (add
cleanup to `manager.remove` path if it exists, else on daemon stop).

## Tests

- Daemon: create session, stream some events with no client attached;
  attach a second client → receives the buffered events in order, then live
  ones continue; `since` filters; buffer cap evicts oldest.
- Protocol: marker event schema test.

## Acceptance

- Attach-after-the-fact shows the full prior transcript in the desktop;
  `pnpm check` green; `docs/protocol.md` updated (attach semantics +
  marker events).
