# Crow Desktop — Tauri 2 Claymorphism Shell

## Context

Crow today is a CLI-only Rust agent. The README lists a "Tauri 2 desktop shell" as
in-progress but **no GUI exists** (no `src-tauri`, no frontend, no GUI deps in any
`Cargo.toml`/branch). This plan builds that shell.

Two hard facts from exploration drive the design:

1. **The data model is rich and complete.** `AgentEvent` already models the full live
   stream (`TextDelta`, `ReasoningDelta`, `ToolStarted`, `ToolOutput`, `ToolFinished`,
   `ModelFinished`, `RunFinished`, `RunFailed`, `RunCancelled`), `Message`/`Part`,
   `SessionEntry` (JSONL), the tool set (`read`/`write`/`edit`/`bash`), and the approval
   flow (`AskRequest`/`AskResponse` over channels in `policy.rs`). The GUI has a
   well-typed surface to render.
2. **`crow serve` is a stub that cannot stream.** `handle_submit` awaits `agent.submit`
   inline and returns only an ack; it uses `CollectingSink`, spawns nothing, and pushes
   **no event notifications**. The stdin loop blocks during a run so `interrupt` can't
   arrive mid-run. There is no real provider wiring (only an empty mock under a hidden
   `__model` flag), and the ack's `run_id` is generated separately from the Agent's real
   run id, so correlation is already broken.

Chosen approach (confirmed with user): **repair the app-server to stream, then ship a
Tauri 2 desktop GUI that runs `crow serve` as a bundled sidecar and speaks the JSON-RPC
protocol over its stdio.** Frontend is **React + Vite + TypeScript + Framer Motion**.
Desktop-only (macOS/Linux, matching the CLI). Outcome: a tactile, animated claymorphism
GUI where you watch the agent think, call tools, and approve/deny its actions live.

This plan is **stack + implementation steps only — no code.**

---

## Part 1 — Design language (the visual brief)

**Subject grounding (per design method):** the product is an *autonomous coding agent
named Crow*. Audience: developers. The screen's single job: **watch an agent reason, call
tools, and approve its actions in real time.** The justified aesthetic risk: the blue
accent is the **iridescent sheen on a crow's black feathers** — charcoal-black clay
surfaces catch a cool blue-indigo light. That is why the palette is near-black clay with
a single blue→indigo gradient accent, not a generic "dark theme + blue."

### Palette (charcoal clay + iridescent blue)
| Token | Hex | Use |
|---|---|---|
| `--obsidian` | `#0E0F13` | app background (deepest) |
| `--slate` | `#171A21` | raised clay surface (cards, rail) |
| `--ash` | `#22262F` | higher clay / inset wells, input troughs |
| `--fog` | `#9AA3B2` | secondary/muted text |
| `--mist` | `#E7EBF2` | primary text on clay |
| `--sheen` | `#4C82FB` | primary blue accent (active, focus, links) |
| `--iris` | `#6D5EF3` | gradient partner (blue→indigo flow) |
| `--halo` | `#38BDF8` | cyan gradient tip / success glow |

Semantic mapping: running = `--sheen`→`--iris` flowing gradient; success = `--halo`;
error = a warm desaturated coral `#F2785C` (only place warmth appears); reasoning =
`--fog`.

### Claymorphism recipe (the core surface treatment)
- **Dual shadow** on every raised element: a soft dark drop shadow (down-right) plus a
  faint light rim highlight (up-left) so surfaces read as extruded, puffy clay.
  On dark UI the "light" is a low-opacity cool-white inner highlight, not pure white.
- **Inset variant** for wells (input trough, tool-output pane): shadows inverted (inner).
- **Generous radius**: 20–28px on cards, 14–18px on controls, full-round on the status orb.
- **Subtle top gradient** on each clay surface (`--slate`→ slightly lighter) so light
  falls from the top. Keeps surfaces from looking flat-gray.

### Typography (deliberate, not defaults)
- **Display:** `Clash Display` (or `General Sans`) — geometric, confident, used *sparingly*
  for the app title, section eyebrows, big status labels.
- **Body/UI:** `Inter` — dense, legible for chat and controls.
- **Mono:** `JetBrains Mono` — essential; tool output, code diffs, `bash` commands, IDs.
- Type scale: 12 / 14 / 16 / 20 / 28 / 40, weights 400/500/600, tight tracking on display.

### Layout
```
┌──────────────────────────────────────────────────────────────────┐
│  ◐ Crow      [status orb: Idle▸Sampling▸Tool]     ⧉ tokens 1.2k/8k │  top bar
├────────────┬───────────────────────────────────────────┬─────────┤
│  SESSIONS  │           CONVERSATION STREAM             │ INSPECTOR│
│  (clay     │  ▸ user bubble (clay, inset)              │ (tool    │
│   rail,    │  ▸ assistant text (streams token-by-token)│  detail, │
│   collaps- │  ▸ reasoning (collapsible, --fog)         │  args,   │
│   ible)    │  ▸ TOOL CARD  ⟳ flowing gradient border   │  schema, │
│            │      when running; output pane inset       │  usage)  │
│  + new     │  ▸ run finished / failed banner           │          │
├────────────┴───────────────────────────────────────────┴─────────┤
│  composer (clay input, flowing border on focus)      ▸ Send  ⏹ Stop│
└──────────────────────────────────────────────────────────────────┘
        ┌── APPROVAL CARD (transparent overlay, backdrop-blur) ──┐
        │  ✋ crow wants to run:  bash `rm -rf build/`            │
        │        [ Deny ]           [ Allow ]                    │
        └────────────────────────────────────────────────────────┘
```

### Signature element
**The agent heartbeat orb** in the top bar: a claymorphic sphere that *morphs and pulses*
with `AgentState` (`Idle` calm slow breath → `Sampling` faster shimmer → `ExecutingTool`
a flowing conic gradient sweep → `Failed` a single coral flash). One memorable, living
object; everything else stays quiet. It is literally the crow's "eye."

### Motion (Framer Motion — orchestrated, not scattered)
- **Load:** sidebar rail + top bar stagger-in (spring), 250ms.
- **Streaming text:** each `TextDelta` appended as a token with a 120ms fade/slide-up.
- **Tool card:** animated **conic-gradient flowing border** (`--sheen`→`--iris`→`--halo`)
  while a call is pending (`ToolStarted`→`ToolFinished`); collapse/expand via layout anim.
- **Approval card:** spring scale-in from the originating tool card, `backdrop-blur`
  transparent overlay dimming the stream.
- **Status orb:** spring morph between states.
- **Reduced motion:** `prefers-reduced-motion` disables the flow/pulse, keeps opacity fades.

### Transparent overlays / flowing borders / responsive widgets (explicit asks)
- **Transparent overlays:** approval modal, session-switch scrim, tool inspector drawer —
  all `rgba` + `backdrop-filter: blur()` over the clay.
- **Flowing borders:** conic-gradient border that rotates (CSS `@property --angle` + keyframe,
  or animated mask) on: the active tool card, the composer on focus, the heartbeat orb.
- **Responsive widgets:** token-usage meter, status orb, tool cards, and the sessions rail
  reflow at breakpoints; rail auto-collapses to icons under ~900px; inspector becomes a
  bottom sheet under ~1100px. Container-query-driven where possible.

---

## Part 2 — Stack

**Frontend**
- React 18 + TypeScript, **Vite** (dev server on `:5173`, matches `devUrl`).
- **Framer Motion** — all animation (spring, layout, orchestration).
- **Tailwind CSS** — utility layer; claymorphism encoded as design tokens + a few
  `@layer components` clay classes (`.clay`, `.clay-inset`, `.flow-border`).
- **Zustand** — client state (session list, active run, event log, approval queue).
- **@tauri-apps/api** — `invoke` + `Channel` for the event stream; `event` for lifecycle.
- Fonts self-hosted (Fontsource) so the bundle is offline-capable.

**Desktop shell**
- **Tauri 2** (`tauri`, `tauri-build`, `@tauri-apps/cli`).
- **tauri-plugin-shell** — spawn/manage the `crow serve` **sidecar** (`bundle.externalBin`).
- Tauri Rust side owns the sidecar child, does line-framed JSON-RPC, and forwards server
  event notifications to the webview via a **Tauri Channel** (per the v2 Channel-streaming
  pattern). Frontend never parses raw stdio.

**Backend (the `crow` crate — repaired)**
- Reuse the existing `crow` binary as the sidecar; no new dependency from the Tauri crate
  on the `crow` lib (clean separation, matches README's "separate crate talks via serve").
- Live provider: existing `genai` NVIDIA adapter (`src/provider/genai.rs`), gated on
  `NVIDIA_API_KEY`/`CROW_API_KEY`.

**Repo layout (new)**
```
apps/desktop/
  src-tauri/
    src/main.rs        # thin passthrough → lib::run()
    src/lib.rs         # sidecar manager, JSON-RPC client, tauri commands, Channel forward
    capabilities/default.json
    tauri.conf.json    # externalBin: crow sidecar, devUrl :5173
    Cargo.toml, build.rs
  src/                 # React + Vite + TS
  package.json
scripts/               # add: build crow sidecar with target-triple suffix for Tauri
```

---

## Part 3 — Backend fixes (must land first; enables streaming)

All in **`src/app_server.rs`**, reusing existing primitives — **do not build new event types.**

1. **Stream events.** Replace `CollectingSink` with the existing **`ChannelSink`**
   (`src/event.rs`, bounded mpsc). Construct the `Agent` with that sink; in a reader task,
   drain the channel and write each `AgentEvent` to stdout as a JSON-RPC **notification**.
   Wrap in an envelope: `{"jsonrpc":"2.0","method":"event","params":{"session_id","run_id","seq","event":<AgentEvent>}}`.
   `AgentEvent` already serializes as `{"type":"TextDelta","text":...}` etc. — reuse verbatim.
2. **Non-blocking run.** `tokio::spawn` the `agent.submit` future; return the ack
   **immediately**. Keep the stdin loop reading so `interrupt`/`ask_resolve` arrive mid-run.
3. **Fix id correlation.** Two bugs: (a) `app_server::run` never plumbs config/provider —
   `cli::run` loads `Config` then discards it; the server must build the provider from
   config/env itself. (b) `submit` calls `Agent::new`, which mints a *fresh* `session_id`,
   ignoring the ULID persisted by `session_start`; construct the Agent so it **adopts the
   session_start id** (via `resume_into`/explicit id) and surface the Agent's real
   `run_id`. Use those real ids in the ack and every event envelope — stop generating a
   throwaway ack `run_id`. `interrupt` must key on / accept the correlated ids.
4. **Wire the approval flow.** Hold the `AskResolver` receiver (`src/policy.rs`); when an
   `AskRequest` arrives, emit an `ask` notification `{ask_id, call:{name,args}}`; add an
   **`ask_resolve`** method `{ask_id, decision:"allow"|"deny"}` that replies on the
   request's `oneshot::Sender` with `AskResponse::Allow|Deny`. (These types are not
   `Serialize` — the bridge maps to/from small DTOs.)
5. **Real provider.** Select `genai` when `NVIDIA_API_KEY`/`CROW_API_KEY` is set; keep the
   `__model` mock path for tests. Return a clear typed error when no provider is configured.
6. **Protocol consistency.** Keep the **current** underscore method names + `ready`
   notification + JSON-RPC result envelopes (they exist and are tested). Add only the
   `event`, `ask` notifications and the `ask_resolve` method. Explicitly **do not** adopt
   the unimplemented wave-4 `hello/reply/event/bye` docs — note that divergence in the PR.
7. Add integration tests: submit → assert ordered `event` notifications
   (`RunStarted…RunFinished`); interrupt mid-run → `RunCancelled`; ask → `ask_resolve`(deny)
   → `ToolFinished` error `policy_denied`.

**Critical files:** `src/app_server.rs` (rewrite `handle_submit`, add reader task + new
methods), reuse `src/event.rs` (`ChannelSink`), `src/policy.rs` (`AskResolver`/`AskRequest`),
`src/provider/genai.rs`, `src/cli.rs` (Serve wiring only if flags change).

---

## Part 4 — Tauri shell & JSON-RPC client

**`src-tauri/src/lib.rs`** (all logic here per Tauri v2 mobile-safe convention):
- On startup, spawn the `crow serve` sidecar via **tauri-plugin-shell**; hold child +
  stdin handle in `Mutex` app state.
- **Line framer:** buffer sidecar stdout, split on `\n`, `serde_json`-parse each line.
  Route by shape: `result`/`error` → resolve the matching request (id map);
  `method:"event"` → forward `params` to the webview **Channel**; `method:"ask"` → forward
  to a dedicated approval Channel; `method:"ready"` → mark connected.
- **Commands** (`generate_handler!`): `session_start`, `session_list`, `session_load`,
  `submit`, `interrupt`, `ask_resolve`, `set_project_root`. Each assigns a JSON-RPC id,
  writes one line to sidecar stdin, awaits its reply via a oneshot registered in the id map.
  All params/returns owned + `Serialize`/`Deserialize`.
- **Streaming:** `submit` takes a `Channel<AgentEvent>`; the framer pushes decoded events
  into it (Channel-streaming pattern) so React sees a typed live feed.
- **Capabilities** (`capabilities/default.json`): `core:default` + `shell:allow-execute`
  (or `shell:allow-spawn`) scoped to the `crow` sidecar only.
- **`tauri.conf.json`:** `bundle.externalBin` → `binaries/crow`; `build.devUrl` :5173;
  `beforeDevCommand` `npm run dev`; window 1280×800, `decorations` per design, transparent
  optional; strict CSP.
- **`main.rs`** thin passthrough → `lib::run()`.

**Sidecar packaging:** add a script to `cargo build --release` the `crow` binary and copy
it to `apps/desktop/src-tauri/binaries/crow-<target-triple>` (the suffix Tauri sidecars
require). Document in the desktop README.

---

## Part 5 — Frontend architecture

- **Typed event model** (`src/ipc/events.ts`): TS discriminated unions mirroring **two
  distinct shapes** — the *live* stream (`AgentEvent`, `type`-tagged) and *JSONL replay*
  from `session_load` (`kind`-tagged `{kind:"tool_started",...}` objects, timestamps
  omitted). Both feed the same reducer. Use the *exact* field names from the data-model
  report (e.g. `ToolOutput.chunk` is a byte array → decode UTF-8; timestamps are Unix-ms
  **numbers**; `Role::ToolResult` serializes as `"toolresult"`; `Part::ToolCall` uses `id`
  not `call_id`).
- **Store (Zustand):** `sessions[]`, `activeSessionId`, `runId`, ordered `entries[]`
  (assembled from events: concatenate `TextDelta`; group `ToolStarted`→`ToolOutput`→
  `ToolFinished` by `call_id`; `ReasoningDelta` collapsed), `agentState`, `usage`,
  `approvalQueue[]`.
- **Event reducer:** subscribe to the `submit` Channel; fold events into `entries`.
  `ToolStarted` opens a tool card in "running" (flow border on); `ToolFinished` closes it
  with success/error; `RunFinished`/`RunFailed`/`RunCancelled` end the run + settle the orb.
- **Components:** `TopBar`+`HeartbeatOrb`, `SessionRail`, `ConversationStream`
  (`UserBubble`, `AssistantText`, `ReasoningBlock`, `ToolCard`, `RunBanner`),
  `Inspector`, `Composer`, `ApprovalOverlay`. Clay classes shared; Framer Motion per Part 1.
- **Approval UX:** `ask` notifications populate `approvalQueue`; `ApprovalOverlay` renders
  the top item; Allow/Deny calls `ask_resolve`.

---

## Milestones

1. **M0 — Backend streaming.** Part 3. Gate: integration test shows ordered `event`
   notifications for a real (or mock) submit, interrupt works mid-run, ask round-trips.
2. **M1 — Shell handshake.** Scaffold `apps/desktop`, spawn sidecar, `ready` handshake,
   `session_start`/`session_list` render in a bare UI. Gate: sessions list from a real dir.
3. **M2 — Live conversation.** `submit` + Channel streaming; assistant text + tool cards
   render live; interrupt/Stop works. Gate: end-to-end run visible with `NVIDIA_API_KEY`.
4. **M3 — Claymorphism system.** Tokens, clay classes, fonts, orb, flow borders, overlays,
   Framer Motion orchestration, responsive breakpoints, reduced-motion.
5. **M4 — Approval + inspector + session load/resume.** Approval overlay wired to
   `ask_resolve`; inspector; `session_load` replay of JSONL history.
6. **M5 — Polish & package.** Sidecar bundling, icons, CSP, self-critique pass on the
   design, screenshots.

---

## Verification

- **Backend:** `make test` + new `app_server` integration tests (submit stream ordering,
  interrupt, ask deny → `policy_denied`). Manually: `crow serve`, pipe framed JSON-RPC
  lines, confirm `ready` then `event` notifications then `RunFinished`.
- **Shell/IPC:** `npm run tauri dev`; DevTools to confirm the Channel receives typed
  events; verify sidecar starts and `ready` handshake completes (guards white-screen).
- **End-to-end:** with `NVIDIA_API_KEY` set, start a session, send a task that triggers a
  `bash`/`write` tool → approval overlay appears → Allow → tool card streams output →
  `RunFinished`. Deny path → tool card shows `policy_denied`.
- **Design floor:** resize to mobile widths (rail collapse, inspector→sheet); keyboard
  focus visible on composer/buttons; `prefers-reduced-motion` disables flow/pulse;
  screenshot the four orb states.

## Open decision (call out in the PR)
Current server uses underscore JSON-RPC methods + `ready`; unimplemented wave-4 docs
(`docs/briefs/wave-4/`) describe a different `hello/reply/event/bye` protocol. This plan
extends the **current** protocol. If the team prefers the wave-4 contract, M0 grows to a
full protocol rewrite — flag before starting M0.
