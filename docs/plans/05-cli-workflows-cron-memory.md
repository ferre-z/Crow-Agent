# Plan 05 — CLI: workflows, cron, memory commands

**Goal:** the `crow` CLI exposes the P6/P7 surface that today only exists
on the wire.

## Current state

- CLI (`apps/cli/src/bin.ts`): hand-rolled dispatch, `hosts`, `info`,
  `sessions`, `send`, `prompt`, `cancel`, `attach`; `render.ts` event
  renderer; e2e tests spawn the real bin against a faux daemon.
- Wire methods already exist: `workflow.list/run`, `cron.add/list/remove`,
  `memory.query/write/episodes/facts`.

## Design

New commands (same global flags: `--host`/`--url`+`--token`/`--json`):

- `crow workflows` — list (name, description, step count).
- `crow workflow run <name> [--inputs '{"k":"v"}']` — returns runId and
  streams `event.workflow` steps until done (reuse the `send --wait`
  streaming loop pattern; map `event.workflow` in `render.ts`).
- `crow cron list` — table: id (short), name, workflow, recurrence, next run.
- `crow cron add <name> --workflow <wf> --recurrence "@every 30m"
[--inputs '{...}']`.
- `crow cron remove <jobId>`.
- `crow memory query "<q>" [--k 10] [--kinds fact|episode]`.
- `crow memory add "<text>" [--tags a,b]`.
- `crow memory episodes` / `crow memory facts`.

Extend `render.ts`: `event.workflow` (step lines with kind icons) and
`event.cron_fired`. Update USAGE + `--help`.

## Tests

- `render.ts` unit tests for the two new event kinds.
- e2e: against a real daemon — `crow workflows`; `crow workflow run
self-check` streams to done; `crow cron add/list/remove` round-trip;
  `crow memory add/query` round-trip.

## Acceptance

- All commands work against a live daemon; exit codes match the existing
  conventions (0 ok / 1 runtime / 2 usage); `pnpm check` green.
