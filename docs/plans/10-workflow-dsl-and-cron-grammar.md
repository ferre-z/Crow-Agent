# Plan 10 — Workflow DSL (YAML/TS) + full cron grammar

**Goal:** workflows can be written in YAML (or TS) instead of hand-written
JSON, and the scheduler understands real cron expressions.

## Current state

- Workflows: `isWorkflow` structural validation in
  `packages/core/src/workflows.ts`; daemon loads built-ins + `*.json` from
  `<dataDir>/workflows/` (`loadBuiltinWorkflows` in `server.ts`).
- Scheduler (`packages/core/src/scheduler.ts`): recurrence grammar is
  `@every N{s|m|h}`, `@hourly`, `@daily HH:MM` only. `parseRecurrence` +
  `nextRunAt` are pure and tested.
- Repo already depends on `yaml` transitively (pi-agent-core depends on it)
  — adding it as a direct dep of `@crow/core` is sanctioned if needed.

## Design

1. **YAML loader**: `<dataDir>/workflows/*.yaml|*.yml` parsed with the
   `yaml` package into the same `Workflow` shape (reuse `isWorkflow`;
   report file/line on validation errors). Keep `.json` support.
2. **Cron**: implement standard 5-field cron (`M H DoM Mon DoW` with
   `*`, `*/n`, lists, ranges) as `parseRecurrence` extensions —
   pure function `nextCronRun(expr, from)`; keep the existing `@every` etc.
   as shortcuts. ~200 lines, no dependency, or adopt `cron-parser` if the
   owner prefers a dep (call it out in the commit).
3. **TS workflows** (optional): `*.workflow.ts` exporting a default
   `Workflow` — loaded via dynamic import in the daemon (Node type-stripping
   handles `.ts` directly). Gate behind an explicit opt-in flag
   (`--ts-workflows`) since it's code execution.

## Tests

- YAML: fixture files → parsed workflows run end-to-end like the JSON path;
  malformed YAML gives a clear error naming the file.
- Cron: a table of expressions → expected next runs (minute steps, ranges,
  lists, `*/5`, month/dow boundaries, leap day).
- Scheduler accepts both grammars and stores the original spec string.

## Acceptance

- A YAML workflow runs from cron on a full-cron schedule; `pnpm check`
  green; README + protocol doc updated.
