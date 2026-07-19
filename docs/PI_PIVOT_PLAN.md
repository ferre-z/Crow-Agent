# Pivot Plan: Crow on top of pi-agent-core

Status: pivot proposal. This doc supersedes the from-scratch
`TUI_PARITY_PLAN.md` once accepted.

## TL;DR

Crow's from-scratch Rust kernel + TUI is a worse Claude Code
clone than the existing `pi-coding-agent` (TypeScript, MIT,
maintained by earendil-works). Rather than keep rebuilding
features pi already ships, **Crow becomes a fork + customisation
of pi-coding-agent**. We keep the Rust binary as a thin wrapper
that delegates to the TypeScript runtime via the documented RPC
mode. All work in `src/tui/`, `src/provider/`, `src/agent.rs`,
`src/cli.rs`, and the entire `Cargo.toml` is replaced by a small
Rust shim plus a `pi-crow/` TypeScript project.

## Why pivot

1. **Features ship for free.** pi already has session picker,
   compact, fork/clone/branching, MCP via extensions, themes
   (dark/light/custom), Skills, prompt templates, GitHub Copilot /
   Anthropic / OpenAI / **NVIDIA NIM** / Bedrock / etc., an
   extension API, model cycling, copy/export/share, all slash
   commands we built, and more.
2. **Reliability.** pi's tool-call streaming, JSON parsing,
   model fallback, and provider quirks are battle-tested against
   the entire NVIDIA / Anthropic / OpenAI catalog. Our hand-rolled
   genai 0.6.5 wrapper required three separate fixes just to get
   streaming working on Nemotron 3 Ultra (URL trailing slash,
   double-encoded tool args, empty-stream retry).
3. **Maintenance.** pi's maintainers follow upstream provider
   changes. We were on the hook for every model id rename.
4. **MCP, OAuth, Anthropic subs** all need real implementation;
   pi ships them.
5. **Crow's "skill" is differentiation.** pi is the substrate; the
   user's value-add is the skill set (custom tools, custom prompts,
   custom theme, custom extension) — not the agent loop itself.

## Three integration options

| Option | Pros | Cons |
|---|---|---|
| **A. Fork pi-coding-agent, rebrand as Crow** | Full feature parity day 1, less code to write | Two languages (TS+our Rust shim); pnpm + cargo build pipeline |
| **B. Use pi-agent-core + write our own TUI on top** | TS only; can reuse pi-tui or write a Rust TUI that calls Node via RPC | We re-write the chat rendering, picker, etc. — the work we just spent on |
| **C. Run pi-coding-agent as a subprocess, drive via RPC** | Zero TS code in Crow; pure Rust wrapper | Rust and TS run as two processes; RPC overhead per event |

**Recommendation: Option A.** It's the fastest path to a 1:1
Claude Code clone, and pi is MIT-licensed so forking + rebranding
is clean. The "two languages" concern is real but small: pi is
already npm-distributed; we add an npm postinstall hook and ship
the TS bundle.

## What we keep from the current Crow repo

- `docs/TUI_PARITY_PLAN.md` — useful as the spec for the
  *differentiating* features we'll build on top of pi
- `config/pricing.toml` — pi uses a different config key but
  pricing tables transfer
- The current NVIDIA API key in `~/.zshrc`-style env (just
  `NVIDIA_API_KEY`)
- `apps/desktop/` (Tauri 2) — pivots to wrap the new TS CLI
  via the JSON-RPC mode pi already exposes (`--mode rpc`)
- All the test fixtures and integration scenarios

## What gets archived

The Rust kernel and TUI worked but the user wants 1:1 Claude Code
parity faster than we're shipping it. The Rust code moves to
`archive/crow-rust-v0/` so we can keep the git history. After the
pivot:

- `Cargo.toml` shrinks to a 100-LOC binary that just spawns `pi`
  with the right args + a thin env-var passthrough.
- `src/cli.rs` becomes an arg translator.
- `src/tui/` is gone (pi has its own).
- `src/agent.rs`, `src/provider/`, `src/policy.rs`, `src/session.rs`,
  `src/tool/`, `src/message.rs`, `src/event.rs`, `src/ids.rs`,
  `src/config.rs` — gone, replaced by pi's TypeScript packages.

## Repository layout (post-pivot)

```
crow/
├── Cargo.toml          # ~50 LOC, just spawns pi
├── src/
│   └── main.rs         # arg translator + env-var passthrough
├── pi-crow/            # fork of pi-coding-agent, customised
│   ├── package.json
│   ├── packages/
│   │   ├── ai/         # forked from pi-ai, NVIDIA-first
│   │   ├── agent-core/ # forked from pi-agent-core
│   │   ├── tui/        # forked from pi-tui
│   │   └── coding-agent/
│   ├── extensions/
│   │   └── crow/       # our custom extensions (skills, themes,
│   │                   #   tools, custom commands)
│   ├── themes/
│   │   └── crow-default.toml
│   ├── skills/
│   │   ├── review/
│   │   ├── plan/
│   │   └── crow-onboard/
│   └── AGENTS.md
├── apps/
│   ├── desktop/        # Tauri 2 shell, drives `pi --mode rpc`
│   └── web/            # (future) browser shell
└── docs/
    ├── PI_PIVOT_PLAN.md   # this file
    └── (others)
```

## What ships in the first Crow release on top of pi

After the pivot, Crow's value-add over stock pi is:

### Skills (Agent Skills standard, drop-in)

| Skill | Description |
|---|---|
| `crow/review` | Code review with structured severity tags |
| `crow/plan` | Plan-mode that writes plan to `.crow/plans/<id>.md` and asks the user to approve |
| `crow/onboard` | First-run wizard: detect stack, generate `AGENTS.md`, configure providers |
| `crow/test` | Run the project's tests with smart inference (skip network tests, watch mode) |
| `crow/commit` | Conventional-commit message generation from staged diff |

### Extensions (TypeScript)

| Extension | Description |
|---|---|
| `crow/theme` | Custom dark + light themes with Crow brand palette |
| `crow/mcp-bridge` | Pass-through MCP support via stdio (re-export of pi-mcp) |
| `crow/status-line` | Custom footer showing Crow session metadata |
| `crow/keybindings` | Claude Code keymap (matches pi's defaults but renamed) |

### Tools

| Tool | Description |
|---|---|
| `grep` (built-in in pi) | Already there; no work needed |
| `glob` (F.51.01 in the old plan) | Already there as `find`; rename to `glob` if needed |
| `web_fetch` (F.51.03) | Add as a skill that wraps curl |

### Theming

- `crow/dark` and `crow/light` themes with the existing Crow palette
  (greens, dark greys, cyan accents — already established)
- `--theme crow/dark` becomes the default when the user runs
  `crow tui`

### Differentiation

- NVIDIA-first defaults: `crow tui` opens with Nemotron 3 Ultra as
  the default model (we ship the right config)
- Pricing-aware cost display: pi already shows cost; we add the
  per-model pricing.toml table that we built
- Custom branding: `crow` binary name, `crow/dark` theme,
  `crow-*` skill namespacing

## Migration steps

The migration is sequenced to land in a way that lets us verify
each step before moving on.

### Step 0 — research (DONE, this doc)

Compare pi-coding-agent against our parity plan; identify the gap
between pi's defaults and Crow's required behaviour. (This doc.)

### Step 1 — install pi locally and verify on Nemotron 3 Ultra

```bash
npm install -g --ignore-scripts @earendil-works/pi-coding-agent
export NVIDIA_API_KEY='nvapi-...'
pi -p "say hi"
pi -p "list files in src/"   # exercises bash tool
pi --mode rpc < /dev/null    # verify RPC mode works
```

Acceptance: every command above prints expected output; no
`empty_stream` errors.

### Step 2 — fork pi-coding-agent into `pi-crow/`

```bash
git clone https://github.com/earendil-works/pi.git pi-crow
cd pi-crow
# Rename package.json names from @earendil-works/* to @crow/*
# Rename binary `pi` → `crow` in coding-agent/package.json
# Replace LICENSE attributions
```

Acceptance: `pnpm run build` produces a `crow` binary that boots
into the same TUI as stock pi.

### Step 3 — Crow defaults: NVIDIA-first config

```json5
// pi-crow/packages/coding-agent/src/default-settings.json
{
  "provider": "nvidia",
  "model": "nvidia/nemotron-3-ultra-550b-a55b",
  "theme": "crow/dark"
}
```

Acceptance: `crow` opens with Nemotron 3 Ultra + crow/dark theme.

### Step 4 — Move existing config to extensions

- `config/pricing.toml` → `pi-crow/extensions/crow/pricing.toml`
  loaded by a new extension that hooks the model-finished event
  and updates a status line.
- `.crow/` rules → pi-crow/policy.toml

Acceptance: per-token cost still accumulates correctly; rules
still gate tool calls.

### Step 5 — Ship the differentiating skills

Build the 5 skills listed above. Each is ~50 LOC of markdown.
Combined: ~1 day of work.

### Step 6 — Ship the custom theme + status line

`crow/dark` theme (light variant optional). Status line that shows:
session name, current model, cost, context %, tool timer.

Acceptance: status line appears in the footer; matches the layout
we already have in Rust.

### Step 7 — Update Tauri desktop

`apps/desktop/` already wraps `crow serve` via JSON-RPC. Replace
that with `crow --mode rpc` (pi's RPC mode). pi already has the
JSON-RPC schema; the Tauri side just needs to map the events
onto the existing UI store.

Acceptance: desktop app launches the same UI as before; the
underlying agent is pi.

### Step 8 — Rust binary becomes a thin shim

```rust
// src/main.rs (~50 LOC)
fn main() {
    let pi = which("pi").expect("pi-coding-agent not installed");
    Command::new(pi).args(std::env::args().skip(1)).status()
        .expect("pi failed");
}
```

The Crow Rust binary becomes a thin launcher that delegates to
the TS pi-crow. This preserves the `crow` command name + binary
distribution + Cargo install path.

### Step 9 — Update install scripts + docs

- `scripts/install.sh` becomes `npm install -g @crow/coding-agent`
- README rewritten to lead with "Crow = pi + custom skills + NVIDIA
  defaults + custom theme"
- `docs/TUI_PARITY_PLAN.md` archived as historical context (the
  plan is fulfilled by stock pi + custom extensions)

## What we lose

- The Rust kernel. pi is TS; Crow becomes a TS-first project
  with a tiny Rust shim. Anyone maintaining the Rust kernel
  will need to learn TS.
- `cargo test --all-targets --all-features` gates become
  `pnpm run check` + `pnpm test`. We keep `cargo build` to verify
  the Rust shim.
- The deterministic scripted provider (`ScriptedProvider`). pi has
  a mock provider of its own but the API is different; we'll need
  to port the scripted test scenarios.

## What we gain

- Streaming JSON output (`--mode json`) for free.
- Built-in session branching with `/tree` (vs our flat list).
- Anthropic / OpenAI / Bedrock / Google / OpenRouter / 25+ other
  providers for free.
- Plan mode (built by writing plans to files; we can add a
  custom one via extension).
- Built-in `/compact` with custom-instruction support.
- MCP via community extensions.
- Real OAuth / subscription auth (vs API-key-only).
- A community of pi users who can contribute skills and themes
  back to Crow.

## Risks and mitigations

| Risk | Mitigation |
|---|---|
| pi's internals change under us | Pin to a specific pi commit; bump on a schedule |
| TS ecosystem toolchain (pnpm, esbuild) is unfamiliar to the current Crow contributors | The install script hides it; contributors edit TypeScript via the existing extension API which is well-documented |
| We lose direct control over the agent loop | pi exposes extension hooks for every event we care about (tool_call, message, etc.); if we ever need to fork the loop itself, the loop is ~300 LOC of TS |
| npm install is heavier than `cargo build` | npm caches per-package; install is one-time + updates are delta |
| Behavioural drift between stock pi and Crow | The 5 skills + extensions live under `extensions/crow/` and are git-tracked; pi upgrades that touch them trigger CI |
| License | pi is MIT; we relicense Crow MIT and attribute upstream in `LICENSE-THIRD-PARTY.md` |

## Acceptance criteria for the pivot

1. `crow tui` opens with Nemotron 3 Ultra as the default model.
2. `crow -p "list files in src/"` runs bash and prints the
   directory listing (vs the old path that needed three fix
   commits to reach this state).
3. `crow sessions` and `/resume` work as they do in stock pi,
   but show Crow's session-name sidecar.
4. The status bar shows `model · cost · context% · tool timer`
   via the Crow extension, not via Rust.
5. The five Crow skills (`crow/review`, `crow/plan`, `crow/onboard`,
   `crow/test`, `crow/commit`) are installable via `crow install`.
6. `cargo build --release` still produces a `crow` binary that
   delegates to the TS bundle.
7. `pnpm run check` passes; lint clean.
8. README, AGENTS.md, install script all updated.

## Open questions for the user

- (a) Should the Rust shim stay (Option A) or do we go pure-TS
  with a different command name (`crow` vs the TS binary)?
- (b) Do we want Crow to also be installable as a single npm
  package (`npm i -g @crow/coding-agent`) without the Rust shim?
- (c) Should `crow/desktop` (Tauri) ship in the same repo or as
  a separate workspace?

## What I'm NOT planning

Per the user's directive: don't cut corners, plan everything I
can imagine right now.

- **Mobile / iOS / Android**: out of scope.
- **Voice / audio input**: out of scope.
- **Custom inference backend**: pi already supports llama.cpp
  router + most major providers; we don't need to roll our own.
- **Replacing pi's UI library**: pi-tui does differential
  rendering; our needs fit inside its widget model.

Everything else listed above is in scope for the pivot.
