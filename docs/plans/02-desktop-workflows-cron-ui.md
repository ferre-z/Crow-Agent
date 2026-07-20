# Plan 02 — Desktop workflows & cron manager UI

**Goal:** the desktop can list/run workflows and add/list/remove cron jobs
per host (the P6 backend is complete; there is no UI).

## Current state

- Wire (done): `workflow.list`, `workflow.run`, `cron.add`, `cron.list`,
  `cron.remove`; events `event.workflow` (step_started/step_done/done/error),
  `event.cron_fired`. Schemas in `packages/protocol/src/{methods,events}.ts`.
- Daemon behavior: `workflow.run` returns `{ runId }` immediately and streams
  `event.workflow`; cron fires emit `event.cron_fired { jobId, jobName,
workflowRunId }` before the run's events.
- Desktop architecture: reducer `state.ts` (keyed by `${host}:${sessionId}`),
  `MainScreen.tsx` sidebar + main pane, `activeView` model (session vs team
  run) added in P4 — reuse it for workflow runs.
- Bridge: methods must be added like P4's team ones (`window.crow.workflowList(host)` etc.), via a `manager.call` passthrough in `connection-manager.ts`.

## Design

1. **Bridge + main**: add `workflowList/run`, `cronAdd/List/Remove` to
   `CrowBridge` (shared/api.ts, preload, main/index.ts passthrough).
2. **Reducer** (`state.ts`):
   - `workflows: Record<hostName, WorkflowInfo[]>`, `cronJobs: Record<hostName, CronJobWire[]>`.
   - `workflowRuns: Record<runId, { hostName, runId, workflow, state,
steps: WorkflowStepState[], output?, error? }>` fed by `event.workflow`
     (route before the sessionId guard like agent/team events).
   - `cronFired` events: link runId → job (store `jobId` on the run).
   - Actions: `workflows.set`, `cronJobs.set`, `workflow.started`, plus the
     daemon events. ~15 reducer tests.
3. **UI** (`MainScreen.tsx`):
   - Sidebar new collapsible section per host: "workflows" (list with Run
     button) and "cron" (list with recurrence, next run, delete; add form:
     name, workflow select, recurrence input, inputs JSON).
   - Main pane: `WorkflowRunView.tsx` — step timeline like TeamRunView
     (kind icons: prompt/shell/a2a), output, error states. Selected via
     `activeView` (`{ kind: "workflow", runId }`).
4. **Run a workflow**: modal with workflow select, inputs (JSON textarea),
   then watch the run live.

## Tests

- Reducer: workflows/cron list set, run lifecycle, step sequencing,
  error paths, cron_fired linkage.
- Keep the 79 existing desktop tests green; target ~95 after.

## Acceptance

- Against a live daemon with the built-in `self-check` workflow: run it
  from the UI, watch steps, see the output; add a cron job and see it listed
  with a computed next run; delete it. `pnpm check` green.
