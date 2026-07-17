# Crow Desktop — 3-Agent Execution Plan

> Full design spec: `docs/desktop/DESIGN-PLAN.md` (repo copy) — also at
> `~/.claude/plans/make-a-detailed-plan-spicy-yao.md` on the Claude host.
> Repo root: `/home/ubuntu/code/crow`. Read the DESIGN-PLAN before starting.

## The frozen contract (shared source of truth — do not renegotiate)

All three agents integrate at ONE seam: the `crow serve` line-delimited JSON-RPC
protocol and the `AgentEvent` schema. It is fully specified in DESIGN-PLAN Parts 3–5.
Freeze it exactly:

- **Methods (underscore names):** `initialize`, `session_start`, `session_list`,
  `session_load`, `submit`, `interrupt`, `policy_set`, `shutdown` + NEW `ask_resolve`.
- **Server→client notifications:** `ready`, NEW `event`
  `{"jsonrpc":"2.0","method":"event","params":{session_id,run_id,seq,event:<AgentEvent>}}`,
  NEW `ask` `{"jsonrpc":"2.0","method":"ask","params":{ask_id,call:{name,args}}}`.
- **`AgentEvent`** is `type`-tagged: `RunStarted|ModelStarted|TextDelta|ReasoningDelta|
  ToolStarted|ToolOutput|ToolFinished|ModelFinished|RunFinished|RunCancelled|RunFailed`
  (exact fields in DESIGN-PLAN). `ToolOutput.chunk` = byte array; timestamps = Unix-ms
  numbers; `ToolFinished.result` = `{Success:{output,truncated}}|{Error:{code,message,truncated}}`.
- **`session_load` replay** is a DIFFERENT `kind`-tagged shape (`session_started`,
  `user_message`, `assistant_message`, `tool_started`, `tool_finished`, `run_finished`,
  `run_interrupted`, `run_failed`) — the TS model must handle both.

Because the contract is frozen, **all three sections start in parallel.** Frontend logic
is written against the documented contract, not against a running server.

---

## Section 1 — CLAUDE (heavy / hard / correctness-critical)

The two Rust integration layers + owning the contract. Highest risk; nothing streams
until this lands.

**1A. Backend rewrite — `src/app_server.rs`** (reuse existing primitives; no new event types)
- Non-blocking `submit`: `tokio::spawn` the `agent.submit` future, return ack immediately,
  keep the stdin loop reading so `interrupt`/`ask_resolve` arrive mid-run.
- Swap `CollectingSink` → existing **`ChannelSink`** (`src/event.rs`); reader task drains
  it and writes each `AgentEvent` as an `event` notification with the envelope above.
- Fix id correlation: (a) build the provider from config/env inside the server
  (`app_server::run` currently ignores `Config`); (b) construct the Agent so it **adopts
  the `session_start` ULID** (not a fresh one) and surface the Agent's real `run_id` in the
  ack + every envelope. `interrupt` keys on the correlated ids.
- Approval flow: hold the `AskResolver` receiver (`src/policy.rs`); emit `ask`
  notifications; add `ask_resolve` `{ask_id,decision:"allow"|"deny"}` replying on the
  `oneshot::Sender` with `AskResponse::Allow|Deny`.
- Real provider via `genai` (`src/provider/genai.rs`) gated on `NVIDIA_API_KEY`/`CROW_API_KEY`;
  keep `__model` mock for tests. Keep the underscore protocol; do NOT adopt wave-4.
- Tests in `tests/app_server.rs`: submit → ordered events; interrupt → `RunCancelled`;
  ask deny → `ToolFinished` error `policy_denied`.

**1B. Tauri Rust integration — `apps/desktop/src-tauri/src/lib.rs`** (the integration brain)
- Spawn `crow serve` sidecar (tauri-plugin-shell), hold child + stdin in `Mutex` state.
- Line framer: parse each stdout line; route `result`/`error`→id-map oneshot,
  `event`→webview Channel, `ask`→approval Channel, `ready`→connected flag.
- Commands (`generate_handler!`): `session_start`, `session_list`, `session_load`,
  `submit`(takes `Channel<AgentEvent>`), `interrupt`, `ask_resolve`, `set_project_root`.
- `capabilities/default.json`: `core:default` + shell exec scoped to the `crow` sidecar.
- `main.rs`: thin passthrough → `lib::run()`.

**1C. Publish the authoritative TS contract** — write `apps/desktop/src/ipc/contract.md`
(or stub `events.ts` types) so Sections 2/3 build against a single frozen definition.

**Owns:** `src/app_server.rs`, `tests/app_server.rs`, `apps/desktop/src-tauri/**`,
`apps/desktop/src/ipc/contract.md`. **Depends on:** nothing (starts immediately).

---

## Section 2 — OPENCODE / MINIMAX PRO (heavy coding, well-specified)

The entire React app — logic + design-system quality + animation. Bounded by the frozen
contract and DESIGN-PLAN Parts 1 & 5.

- **IPC + state:** `src/ipc/events.ts` (TS discriminated unions for BOTH live `type`-tagged
  events and `kind`-tagged replay), `src/ipc/client.ts` (wrappers over `invoke` + `Channel`),
  Zustand store `src/store.ts` + event reducer (concatenate `TextDelta`; group
  `ToolStarted`→`ToolOutput`→`ToolFinished` by `call_id`; collapse `ReasoningDelta`;
  settle orb on `RunFinished|RunFailed|RunCancelled`; `ask`→`approvalQueue`).
- **Stateful/animated components:** `ConversationStream`, `AssistantText` (token stream),
  `ReasoningBlock` (collapsible), `ToolCard` (running↔finished, flowing gradient border,
  layout-animated collapse), `RunBanner`, `Composer` (focus flow-border, Send/Stop),
  `ApprovalOverlay` (backdrop-blur, spring scale-in, Allow/Deny→`ask_resolve`),
  `Inspector`, `SessionRail`, `TopBar` + **`HeartbeatOrb`** (morphs across AgentState).
- **Design system quality + Framer Motion:** implement the clay recipe, orb states, flow
  borders, and orchestration to the DESIGN-PLAN spec; respect `prefers-reduced-motion`;
  responsive breakpoints (rail collapse <900px, inspector→bottom-sheet <1100px).

**Owns:** `apps/desktop/src/**` EXCEPT scaffolding/config files owned by Section 3, and
EXCEPT `src/ipc/contract.md` (Claude authors it). **Depends on:** the frozen contract
(available now) for logic; Section 3's scaffolding before `npm run dev` works — but can
write all `.ts`/`.tsx` immediately.

---

## Section 3 — OPENCODE / DEEPSEEK V4 FLASH (fast, mechanical, low-risk)

Scaffolding, config, tokens, build glue, docs. No design judgment or async logic.

- **Scaffold `apps/desktop`:** Vite + React + TS (`package.json`, `tsconfig.json`,
  `vite.config.ts`, `index.html`, entry `main.tsx`/`App.tsx` stub), Tailwind
  (`tailwind.config.ts`, `postcss.config.js`, `src/index.css`).
- **Tauri config:** `src-tauri/tauri.conf.json` (`externalBin: binaries/crow`,
  `devUrl: http://localhost:5173`, `beforeDevCommand: npm run dev`, window 1280×800,
  strict CSP), `src-tauri/Cargo.toml`, `src-tauri/build.rs`. (Leave `lib.rs`/`main.rs`
  to Claude — Section 1B — or provide empty stubs Claude will overwrite.)
- **Design tokens (mechanical, from DESIGN-PLAN Part 1):** CSS variables for the 8-color
  palette, the `.clay` / `.clay-inset` / `.flow-border` utility scaffolding (dual-shadow
  recipe, `@property --angle` + rotate keyframes), and the type scale in Tailwind theme.
  Get the values in place; Section 2 tunes the feel.
- **Fonts:** install Fontsource for Clash Display / Inter / JetBrains Mono; wire imports.
- **Build glue:** `scripts/build-sidecar.sh` — `cargo build --release` then copy the
  `crow` binary to `apps/desktop/src-tauri/binaries/crow-$(rustc -vV target triple)`.
- **Docs:** `apps/desktop/README.md` (dev/run/package steps), placeholder app icons.

**Owns:** `apps/desktop/package.json`, `tsconfig.json`, `vite.config.ts`, `index.html`,
`tailwind.config.ts`, `postcss.config.js`, `src/index.css`, `src-tauri/tauri.conf.json`,
`src-tauri/Cargo.toml`, `src-tauri/build.rs`, `scripts/build-sidecar.sh`,
`apps/desktop/README.md`, icons. **Depends on:** nothing (starts immediately).

---

## Integration order (after parallel build)
1. Section 1A lands + green tests → real server streams.
2. Section 3 scaffolding + Section 1B → `npm run tauri dev` boots, `ready` handshake.
3. Section 2 wired to the live Channel → end-to-end run (needs `NVIDIA_API_KEY`).
4. Verify per DESIGN-PLAN "Verification": stream ordering, interrupt, approval allow/deny,
   responsive + reduced-motion, four orb states.

## File-ownership rule (avoid collisions)
Each path is owned by exactly one section (see "Owns" above). If you need a file another
section owns, leave a TODO comment and note it — do not edit across boundaries.
