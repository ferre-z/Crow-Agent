# Crow wire protocol (v0 draft)

Transport: WebSocket, newline-delimited JSON-RPC 2.0 messages. One WS
connection per (desktop, daemon) pair. Auth: bearer token presented in the
`Authorization: Bearer <token>` header at WS upgrade time; tokens live in
`~/.crow/daemon.json` (mode 0600) on the daemon host.

## Methods (client → daemon)

| Method                      | Params                                       | Result                                                                         |
| --------------------------- | -------------------------------------------- | ------------------------------------------------------------------------------ |
| `session.create`            | `{ cwd, model?, systemPrompt?, skillDirs? }` | `{ sessionId }`                                                                |
| `session.send`              | `{ sessionId, text }`                        | `{}` (events stream separately)                                                |
| `session.cancel`            | `{ sessionId }`                              | `{}`                                                                           |
| `session.list`              | `{}`                                         | `{ sessions: SessionInfo[] }`                                                  |
| `session.attach`            | `{ sessionId, since? }`                      | `{}` (live events; P1 has no replay buffer, `since` is ignored)                |
| `agent.spawn`               | `{ sessionId, prompt, tools?, host? }`       | `{ agentId }` (P4; `host` in P5)                                               |
| `team.run`                  | `{ sessionId, team, input }`                 | `{ runId }` (P4)                                                               |
| `workflow.run`              | `{ workflow, inputs }`                       | `{ runId }` (P6)                                                               |
| `workflow.list`             | `{}`                                         | `{ workflows }` (P6)                                                           |
| `cron.add`                  | `{ schedule, task }`                         | `{ jobId }` (P6)                                                               |
| `cron.list` / `cron.remove` | `{}` / `{ jobId }`                           | `{ jobs }` / `{}` (P6)                                                         |
| `memory.query`              | `{ q, k? }`                                  | `{ results }` (P7)                                                             |
| `memory.write`              | `{ fact, tags? }`                            | `{}` (P7)                                                                      |
| `host.info`                 | `{}`                                         | `{ hostname, platform, arch, node, daemonVersion, protocolVersion, sessions }` |

`SessionInfo` is `{ id, cwd, model, state, createdAt }` with `model` a
`provider/modelId` ref (nullable) and `state` one of `"idle" | "busy"`. A
rejected WS upgrade answers HTTP 401 before any frames flow. Unknown methods
return `-32601`, bad params `-32602`, unknown sessions `-32002`, prompts on a
busy session `-32003`; garbage lines return `-32700` (unparseable) or `-32600`
(well-formed JSON, invalid frame) with id `"unknown"`.

## Events (daemon → client, as JSON-RPC notifications)

| Method                   | Params                                                                       |
| ------------------------ | ---------------------------------------------------------------------------- |
| `event.token`            | `{ sessionId, text }`                                                        |
| `event.thinking`         | `{ sessionId, text }`                                                        |
| `event.tool_call`        | `{ sessionId, callId, tool, args }`                                          |
| `event.tool_result`      | `{ sessionId, callId, tool, output, isError }`                               |
| `event.approval_request` | `{ sessionId, callId, tool, args }` — client replies with `approval.respond` |
| `event.session_state`    | `{ sessionId, state, error? }` — state is `"idle" \| "streaming" \| "error"` |
| `event.job`              | `{ jobId, kind, detail }` (P6)                                               |

## A2A (daemon → daemon, P5)

- `GET /.well-known/agent.json` — agent card: name, capabilities, models, A2A endpoint.
- `POST /a2a/tasks` — delegate a prompt; returns `{ taskId }`; task events polled at `/a2a/tasks/:id` or pushed via registered callback.

All schemas are defined once in `packages/protocol` with zod and inferred
types; this document is the human-readable mirror.
