# Crow app-server IPC contract (authoritative)

> Source of truth for the desktop GUI ↔ `crow serve` wire protocol.
> Authored from the **shipped** implementation in `src/app_server.rs`
> (all 18 integration tests green; clippy clean). If code and this
> doc disagree, the code wins — file an issue. Do NOT implement the
> unshipped wave-4 `hello/reply/event/bye` protocol; this is the
> real contract.

## Transport

Line-delimited JSON over the `crow serve` child process's stdio: one
JSON value per line. Requests → stdin; responses + server-pushed
notifications → stdout (all serialized through a single writer, so
lines never interleave). Logs → stderr. EOF on stdin = clean shutdown
(any in-flight runs are cancelled first).

`protocol_version` is currently `1`.

## Lifecycle

1. Spawn `crow serve`. First stdout line is the ready notification:
   `{"jsonrpc":"2.0","method":"ready","params":{"protocol_version":1}}`
2. (optional) `initialize` → `{"protocol_version":1}`.
3. `session_start` → `{session_id, path}`.
4. `submit` → streamed ack, then `event`/`ask` notifications.
5. `interrupt` / `ask_resolve` may be sent while a run is live.
6. `shutdown` → `{result:null}` then the process exits (cancels
   in-flight runs).

## Request/response methods

Every response is `{"jsonrpc":"2.0","id":<echoed>,"result":<R>}` or
`{"jsonrpc":"2.0","id":<echoed>,"error":{"code":<i32>,"message":<str>}}`.
Unknown method → error `-32000`; unparseable line → error `-32700`
(with `id:null`). `id` is echoed verbatim (any JSON value).

| Method | Params | Result |
|---|---|---|
| `initialize` | (ignored) | `{protocol_version:1}` |
| `session_start` | `{project_root:string}` | `{session_id:string, path:string}` |
| `session_list` | `{project_root:string}` | `{sessions:[{session_id, started_at:<unix-ms number>, schema_version, path}]}` (newest first; `[]` if none) |
| `session_load` | `{session_id:string, path:string, project_root:string}` | `{session_id, events:[<replay object>]}` — see **Replay**. `path` must equal `<project_root>/.crow/sessions/<session_id>.jsonl` (canonicalised) or a typed `path mismatch` error is returned. |
| `interrupt` | `{session_id:string}` | `{cancelled:bool}` |
| `ask_resolve` | `{session_id, ask_id, decision:"allow"\|"deny"}` | `{resolved:bool}` |
| `policy_set` | (ignored) | `{ok:true}` — **documented no-op in v0.** Approval is fixed to `DefaultPolicy` (`read` auto-allows; `write`/`edit`/`bash` prompt via `ask`; unknown tools are denied). The method is accepted for forward compatibility. |
| `shutdown` | (ignored) | `null`, then exit (cancels in-flight runs) |

### `submit` (special: streamed)

Params: `{session_id:string, path:string, project_root:string,
user_message:string}`. `path` must equal
`<project_root>/.crow/sessions/<session_id>.jsonl` (canonicalised) or
a typed `path mismatch` error is returned. The `project_root` is the
canonical working directory the agent uses for tool execution.

Production requires a provider (`NVIDIA_API_KEY` or `CROW_API_KEY`);
otherwise the ack is replaced by an error whose message contains
`no provider configured`.

**Test-only backdoors** (`__script` / `__model` / `__max_turns` /
`__max_tool_calls`) are **only** present in builds compiled with the
`serve-test-hooks` cargo feature (or under `cfg(test)`). The shipped
`crow serve` binary does **not** honour them. If you pass one in a
release build, it is silently ignored and the production provider
path runs instead.

`submit` does **not** return a normal response. On success it streams,
**in order**:
1. The ack, correlated by the request `id`:
   `{"jsonrpc":"2.0","id":<id>,"result":{run_id:string, session_id:string}}`.
   `session_id` equals the one from `session_start`; `run_id` is the
   agent's real run id and matches every subsequent event envelope.
2. Zero or more `event` notifications, then `ask` notifications as
   needed, ending in a **terminal event** (`RunFinished` /
   `RunCancelled` / `RunFailed`).

**Terminal-event guarantee:** every run ends in **exactly one**
terminal `event`. The agent emits one on every exit path
(`max_tool_calls`, `max_turns`, `context_limit`, `empty_stream`,
`stream_error`, etc.) and the server's backstop synthesizes one if
the sink ever dropped the agent's terminal. The GUI can therefore
await a terminal without a timeout.

## Server-pushed notifications

### `event` — one per live `AgentEvent`
```json
{"jsonrpc":"2.0","method":"event","params":{
  "session_id":"<ulid>","run_id":"<ulid>","seq":<u64, 0-based per run>,
  "event": <AgentEvent> }}
```

`seq` is monotonic per run across forwarded events. Gaps in `seq`
indicate the event sink was full and a non-terminal event was
dropped; the terminal event is **never** dropped (see the
terminal-event guarantee above). A backstop-synthesized terminal
uses `seq: u64::MAX` as a sentinel.

`AgentEvent` is `type`-tagged (PascalCase). Exhaustive variants:

| `type` | Fields |
|---|---|
| `RunStarted` | `run_id:string, session_id:string, started_at:<unix-ms number>` |
| `ModelStarted` | — |
| `TextDelta` | `text:string` (concatenate for the live assistant bubble) |
| `ReasoningDelta` | `text:string` |
| `ToolStarted` | `call_id:string, name:string, args:object` |
| `ToolOutput` | `call_id:string, stream:"stdout"\|"stderr", chunk:number[]` (raw bytes — decode UTF-8) |
| `ToolFinished` | `call_id:string, result: ToolOutcome` |
| `ModelFinished` | `usage:{input_tokens:number, output_tokens:number}, stop_reason:"EndTurn"\|"ToolUse"\|"MaxTokens"\|"Cancellation"\|"Error"` |
| `RunFinished` | `message:string` |
| `RunCancelled` | — |
| `RunFailed` | `code:string, retryable:bool, message:string` |

`ToolOutcome` is externally tagged:
`{"Success":{"output":string,"truncated":bool}}` or
`{"Error":{"code":string,"message":string,"truncated":bool}}`.

Common `RunFailed`/`ToolFinished` error codes: `policy_denied`,
`policy_ask_closed`, `unknown_tool`, `invalid_args`, `tool_error`,
`empty_stream`, `provider_error`, `max_tokens`, `max_turns`,
`max_tool_calls`, `context_limit`, `cancelled`, `internal` (backstop
synthesis on unexpected paths).

### `ask` — a tool call awaits approval
```json
{"jsonrpc":"2.0","method":"ask","params":{
  "ask_id":"<string>","call":{"call_id":"<ulid>","name":"<tool>","args":<object>} }}
```
Answer with `ask_resolve` using the same `session_id` + `ask_id`. A
denied (or dropped) ask produces a `ToolFinished` event with
`result.Error.code == "policy_denied"`. DefaultPolicy: `read` auto-
allows; `write`/`edit`/`bash` ask; unknown tools are denied.

## Replay shape (`session_load`) — DISTINCT from live events

Replay objects are **`kind`-tagged** (snake_case), NOT `type`-tagged,
and omit timestamps. The GUI reducer must accept both shapes.

| `kind` | Fields |
|---|---|
| `session_started` | — |
| `user_message` | `id:string, content:string` |
| `assistant_message` | `id:string, parts:[Part]` |
| `tool_started` | `call_id:string, name:string, args:object` |
| `tool_finished` | `call_id:string, outcome: ToolOutcome` |
| `run_finished` | `message:string` |
| `run_interrupted` | `active_call:string\|null` |
| `run_failed` | `code:string, retryable:bool, message:string` |

`Part` is `kind`-tagged (PascalCase): `Text{text}`, `Reasoning{text}`,
`ToolCall{id, name, args}` (note `id`, **not** `call_id`),
`ToolResult{call_id, output, is_error, truncated, display}` where
`display` is `null` or `{path?, line_count?, byte_size?}`.

## Notes / gotchas for the client
- All ids are bare ULID strings (no `run:`/`session:` prefix on the wire).
- Timestamps are Unix-**milliseconds numbers**, never strings.
- `Role::ToolResult` serializes as the one word `"toolresult"`.
- `ToolOutput` chunks are live-only; they are **not** in `session_load`
  replay (reconstruct tool output from `tool_finished.outcome`).
- A tool-only assistant turn has no `Text` part; reasoning is one
  `Reasoning` part per delta (not concatenated).
- `policy_set` is accepted but does nothing in v0.
- The `path` you send is validated against `project_root` and
  `session_id` on every `submit` and `session_load`. The agent's
  working directory is the canonicalised `project_root`, not the
  parent of `path` — this stops a hostile client from steering
  `bash`/`write` to an arbitrary directory.
