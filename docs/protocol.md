# Crow wire protocol (v0 draft)

Transport: WebSocket, newline-delimited JSON-RPC 2.0 messages. One WS
connection per (desktop, daemon) pair. Auth: bearer token presented in the
`Authorization: Bearer <token>` header at WS upgrade time; tokens live in
`~/.crow/daemon.json` (mode 0600) on the daemon host.

## Methods (client → daemon)

| Method            | Params                                                                         | Result                                                                                   |
| ----------------- | ------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------- |
| `session.create`  | `{ cwd, model?, systemPrompt?, skillDirs?, approvalMode?, autoApproveTools? }` | `{ sessionId }`                                                                          |
| `session.send`    | `{ sessionId, text }`                                                          | `{}` (events stream separately)                                                          |
| `session.cancel`  | `{ sessionId }`                                                                | `{}`                                                                                     |
| `session.list`    | `{}`                                                                           | `{ sessions: SessionInfo[] }`                                                            |
| `session.attach`  | `{ sessionId, since? }`                                                        | `{}` (live events; P1 has no replay buffer, `since` is ignored — plan 04)                |
| `agent.spawn`     | `{ prompt, cwd, systemPrompt?, tools?, model?, host? }`                        | `{ agentId }` (P4; returns immediately, completion arrives via `event.agent`; P5 `host`) |
| `team.list`       | `{}`                                                                           | `{ teams: [{ name, description, agents: [{ name, role }] }] }` (P4)                      |
| `team.run`        | `{ team, input, cwd, model? }`                                                 | `{ runId }` (P4; progress arrives via `event.team`)                                      |
| `workflow.run`    | `{ workflow, inputs? }`                                                        | `{ runId }` (P6; progress arrives via `event.workflow`)                                  |
| `workflow.list`   | `{}`                                                                           | `{ workflows: [{ name, description, cwd, allowShell?, steps: [{ kind, name }] }] }` (P6) |
| `cron.add`        | `{ name, workflow, recurrence, inputs? }`                                      | `CronJob` (P6; full job record)                                                          |
| `cron.list`       | `{}`                                                                           | `{ jobs: CronJob[] }` (P6)                                                               |
| `cron.remove`     | `{ jobId }`                                                                    | `{}` (P6)                                                                                |
| `memory.query`    | `{ q, k?, kinds? }`                                                            | `{ results: MemoryHit[] }` (P7)                                                          |
| `memory.write`    | `{ text, tags? }`                                                              | `{ id, text, tags, createdAt }` (P7)                                                     |
| `memory.episodes` | `{}`                                                                           | `{ episodes: Episode[] }` (P7)                                                           |
| `memory.facts`    | `{}`                                                                           | `{ facts: Fact[] }` (P7)                                                                 |
| `host.info`       | `{}`                                                                           | `{ hostname, platform, arch, node, daemonVersion, protocolVersion, sessions, a2a? }`     |

`SessionInfo` is `{ id, cwd, model, state, createdAt, approvalMode }` with
`model` a `provider/modelId` ref (nullable), `state` one of `"idle" | "busy"`,
and `approvalMode` one of `"auto" | "ask"`. `CronJob` is
`{ id, name, workflowName, recurrence, inputs, createdAt, lastRunAt?, nextRunAt, enabled }`.
`MemoryHit` is `{ id, kind: "episode"|"fact", text, score, tags, createdAt, sessionId?, host? }`.
`Episode`/`Fact` follow `packages/memory/src/db.ts`.

A rejected WS upgrade answers HTTP 401 before any frames flow. Unknown
methods return `-32601`, bad params `-32602`, unknown sessions `-32002`,
prompts on a busy session `-32003`; unknown teams/workflows/cron jobs also
return `-32602`. Garbage lines return `-32700` (unparseable) or `-32600`
(well-formed JSON, invalid frame) with id `"unknown"`.

## Tool-call approvals (P2)

`session.create` accepts an optional `approvalMode` (`"auto"` default,
preserving pre-P2 behavior) and `autoApproveTools` (tool names that never ask,
default `[]`). In `"ask"` mode every tool call — except tools in
`autoApproveTools` or previously approved `"always"` this session — pauses
before execution; the daemon sends `event.approval_request` to connections
attached to that session and waits for a matching `approval.respond`:

- `"allow"` runs this one call; `"always"` runs it and auto-approves the tool
  for the rest of the session; `"deny"` blocks the call and the agent receives
  an error tool result with the reason.
- With no attached clients the call is denied with reason
  `"no client attached to approve"`; after 120 s without an answer it is
  denied with reason `"approval timed out"`.
- `approvalId` is unique per request (`appr_<uuid>`); responds for unknown or
  expired ids, or from connections not attached to the session, are ignored.
- `approval.respond` is a notification: no `id`, no response frame.

## Sub-agents and teams (P4)

`agent.spawn` starts an independent agent run with its own tool set — `tools`
whitelists names from the default coding set (`read`/`write`/`edit`/`bash`),
absent means the full set — and returns `{ agentId }` immediately. `team.run`
runs a named preset (`team.list` enumerates them) as a sequence of sub-agents
whose outputs thread through, and returns `{ runId }` immediately. An unknown
team name is an `INVALID_PARAMS` (-32602) error on the RPC itself.

Both report progress as `event.agent` / `event.team` notifications broadcast to
**every** connected client (not session-scoped, no attach needed). `event.team`
`step` is 1-based; the final `done` carries the last agent's output. Run
failures surface as the event with state `"error"` — never as an RPC error,
because the RPC already returned.

## Events (daemon → client, as JSON-RPC notifications)

| Method                   | Params                                                                                           |
| ------------------------ | ------------------------------------------------------------------------------------------------ |
| `event.token`            | `{ sessionId, text }`                                                                            |
| `event.thinking`         | `{ sessionId, text }`                                                                            |
| `event.tool_call`        | `{ sessionId, callId, tool, args }`                                                              |
| `event.tool_result`      | `{ sessionId, callId, tool, output, isError }`                                                   |
| `event.approval_request` | `{ sessionId, approvalId, callId, tool, args }` — client replies with `approval.respond`         |
| `event.session_state`    | `{ sessionId, state, error? }` — state is `"idle" \| "streaming" \| "error"`                     |
| `event.agent`            | `{ agentId, state, output?, error? }` — state is `"started" \| "done" \| "error"` (P4)           |
| `event.team`             | `{ runId, state, step?, agent?, output?, error? }` (P4)                                          |
| `event.workflow`         | `{ runId, state, step?, name?, kind?, output?, error? }` — `kind` is `prompt`/`shell`/`a2a` (P6) |
| `event.cron_fired`       | `{ jobId, jobName, workflowRunId }` (P6; precedes the run's `event.workflow` frames)             |

## Notifications (client → daemon, no response)

| Method             | Params                                                    |
| ------------------ | --------------------------------------------------------- |
| `approval.respond` | `{ approvalId, decision: "allow" \| "deny" \| "always" }` |

## A2A (daemon → daemon, P5)

A daemon with `a2a` enabled (`--a2a-port`, or the `a2a` constructor option)
serves HTTP on a separate port with the same bearer token as the WS API:

- `GET /.well-known/agent.json` — agent card: name, version, capabilities, available models, tasks endpoint.
- `POST /a2a/tasks` — delegate an `agent.spawn`-shaped task; returns `201 { taskId, state }`.
- `GET /a2a/tasks/:id` — poll a task: `{ taskId, state: "running"|"done"|"error", output?, error? }`.
- `GET /healthz` — liveness.

Delegation is **polling only** (no push callbacks). `agent.spawn { host }`
and team presets with a per-step `host` route through this surface; the
delegating daemon forwards its **own** token (shared-token assumption — see
plan 09). `host.info` reports the A2A base URL as `a2a` when enabled.

All schemas are defined once in `packages/protocol` with zod and inferred
types; this document is the human-readable mirror.
