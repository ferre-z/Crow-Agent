# Plan 03 — Desktop memory browser

**Goal:** browse and search a host's memory (episodes + facts) in the
desktop; write new facts from the UI.

## Current state

- Wire (done): `memory.query { q, k?, kinds? } → { results: MemoryHit[] }`,
  `memory.write { text, tags? } → { id, ... }`, `memory.episodes`,
  `memory.facts`. `MemoryStore` in `packages/memory/src/db.ts`.
- Daemon auto-writes an episode when a session settles (state idle/error)
  and injects a context block into new sessions' system prompts.

## Design

1. **Bridge + main**: add `memoryQuery`, `memoryWrite`, `memoryEpisodes`,
   `memoryFacts` (hostName-scoped, passthrough via `manager.call`).
2. **UI**: a "memory" tab/panel per host (sidebar button toggles a
   `MemoryView.tsx` in the main pane — reuse the `activeView` model with
   `{ kind: "memory", hostName }`):
   - Search box → `memory.query` live results (kind badge, score, tags,
     text, createdAt).
   - Two sections: recent episodes (session id, host, text) and facts.
   - "Add fact" form (text + tags csv) → `memory.write`, refresh list.
3. **Reducer**: minimal — a `memory` slice with per-host query results and
   lists; or keep it as local component state with direct bridge calls
   (simpler; only promote to the reducer if cross-component sharing is
   needed). Prefer component-local for v1, but keep the queries typed.

## Tests

- Reducer tests only if state lands there; otherwise a small pure helper
  (`formatHit`, tag parsing) unit test. UI verified manually per the
  deep-test checklist.

## Acceptance

- Search finds a fact written via the UI; episodes from completed sessions
  appear with their session ids; `pnpm check` green.
