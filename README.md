# Crow

A small autonomous coding agent. Crow is built on top of
[pi-coding-agent](https://github.com/earendil-works/pi) (the
upstream agent loop, providers, session machinery, MCP,
extensions, themes) and adds NVIDIA-first defaults, a custom
theme, and a curated skill set on top.

Crow ships Claude-Code-style features — streaming TUI, tool cards
with diffs, session branching, plan mode, compact, MCP, skills,
themes, JSON mode, RPC mode — by riding pi. Crow's value-add is
the conventions, defaults, and skills tuned for NVIDIA Nemotron 3
Ultra.

---

## Quick start

**Install (one line):**

```bash
curl -sSf https://raw.githubusercontent.com/ferre-z/Crow-Agent/main/scripts/install.sh | sh
```

The installer auto-installs Node.js (>= 18), `git`, and `curl` via
your system package manager when missing. Linux and macOS only
(Windows: use WSL). Pass `--no-bootstrap` to opt out.

After install, the `crow` binary is on PATH and ready:

```bash
crow --version                          # confirm install
crow tui                                # interactive Claude-Code-style REPL
crow -p "say hi"                        # one-shot
crow --mode json -p "list files in ."   # streaming JSON for CI
```

Set your API key in the environment:

```bash
export NVIDIA_API_KEY="nvapi-..."        # default provider
# or:  ANTHROPIC_API_KEY, OPENAI_API_KEY, GEMINI_API_KEY, ...
```

Crow boots straight into Nemotron 3 Ultra on the `crow/dark` theme
when `NVIDIA_API_KEY` is set; pick any other model via `/model`
or `--model`.

### All commands at a glance

| Goal | Command |
|---|---|
| Install | `curl -sSf https://raw.githubusercontent.com/ferre-z/Crow-Agent/main/scripts/install.sh \| sh` |
| Install release build | `… \| sh -s -- --release` |
| Run tests | `git clone … && cd pi-crow && npm run check` |
| Launch interactive TUI | `crow tui` |
| Resume a past session | `crow -c` (or `crow tui -c`) |
| Pick from session list | `crow -r` |
| Pick a different model | `crow --model <id>` |
| One-shot prompt | `crow -p "fix the bug"` |
| Streaming JSON | `crow --mode json -p "..."` |
| RPC mode (for tooling) | `crow --mode rpc` |
| List slash commands | `/help` (inside the TUI) |
| Install a Crow extension | `crow install npm:@crow/<pkg>` |

---

## What's different from upstream pi

| Surface | pi (default) | Crow |
|---|---|---|
| Default provider | anthropic | **nvidia** |
| Default model | claude-opus-4-8 | **nvidia/nemotron-3-ultra-550b-a55b** |
| Default theme | dark | **crow/dark** (greener Crow palette) |
| Branded commands | `/help`, `/login`, `/model`, … | adds `/crow-status`, `/crow-review`, `/crow-plan`, `/crow-onboard`, `/crow-test`, `/crow-commit` |
| Footer | built-in | optional **crow-status** extension (tokens + cost + ctx % + model + branch) |
| Pricing table | none | bundled per-model rates for the defaults |

Everything else (Anthropic/OpenAI/Google/Bedrock providers,
plan-mode-as-file, compact, MCP, session branching, RPC mode,
JSON mode, keybindings, theme hot-reload) ships unchanged from
upstream pi.

---

## Architecture

```
crow-agent/
├── pi-crow/                      fork of pi-mono, rebranded to @crow/*
│   ├── packages/
│   │   ├── ai/                   unified provider layer (NVIDIA, Anthropic, …)
│   │   ├── agent/                agent-core
│   │   ├── tui/                  pi-tui (differential TUI rendering)
│   │   ├── orchestrator/
│   │   └── coding-agent/         'crow' binary (rebranded from 'pi')
│   └── extensions/crow/
│       ├── extensions/crow-status-line.ts
│       ├── skills/
│       │   ├── crow-review/
│       │   ├── crow-plan/
│       │   ├── crow-onboard/
│       │   ├── crow-test/
│       │   └── crow-commit/
│       ├── themes/crow-dark.json
│       └── package.json          declares skills + themes + extensions
├── apps/desktop/                 Tauri 2 shell (Rust)
│   ├── crow-desktop-bridge.js    Node bridge: JSON-RPC ↔ pi --mode rpc
│   ├── src/                      React + TypeScript frontend
│   └── src-tauri/                Tauri Rust shell
├── config/
│   └── pricing.toml              per-model token rates (legacy, kept for parity)
├── docs/
│   ├── PI_PIVOT_PLAN.md          the pivot plan this repo followed
│   ├── TUI_PARITY_PLAN.md        the original from-scratch TUI plan
│   └── (postmortems, briefs, decisions)
└── scripts/install.sh            the one-line installer
```

The Rust kernel that previously implemented the agent loop lives
in git history but is no longer the source of truth. See
`archive/crow-rust-v0/` (preserved in git) for the old code.

---

## Skills (ship with Crow)

| Skill | Trigger | What it does |
|---|---|---|
| `/crow-review` | "review my changes / this file" | Structured diff review with severity tags (Critical / Major / Minor / Nit) |
| `/crow-plan` | "plan X first" | Read-only plan mode; writes `.crow/plans/<id>.md` and waits for approval |
| `/crow-onboard` | first run / "set up this project" | Detects stack, drafts `AGENTS.md`, configures providers |
| `/crow-test` | "run the tests" | Smart test runner per stack, skips integration by default |
| `/crow-commit` | "commit this" | Conventional-Commit generator from staged diff; never pushes |

Each is a single `SKILL.md` under `pi-crow/extensions/crow/skills/`
following the Agent Skills standard. They ship inside the
`@crow/coding-agent` npm package and install via
`crow install npm:@crow/coding-agent`.

---

## Themes

- `crow/dark` — Crow's default. Greener accent palette, slightly
  warmer text. Drop-in if you want the upstream `dark` instead:
  `/settings → theme → dark`.

Themes hot-reload: edit a theme file in
`pi-crow/extensions/crow/themes/` and Crow applies the change
immediately.

---

## Desktop

`apps/desktop/` is a Tauri 2 shell that drives Crow via
`apps/desktop/crow-desktop-bridge.js` — a ~150-LOC Node bridge
that translates the desktop's existing JSON-RPC contract into
pi's `--mode rpc` wire format. Run `npm run tauri dev` in
`apps/desktop/` to launch.

---

## Pricing

`config/pricing.toml` ships per-model USD/1K rates and context
sizes for the defaults. The `crow-status` extension reads this
table to render the footer line.

| Model | Input $/1K | Output $/1K | Context |
|---|---|---|---|
| `nvidia/nemotron-3-ultra-550b-a55b` | 0.0005 | 0.0015 | 262 144 |
| `nvidia/llama-3.1-nemotron-ultra-253b-v1` | 0.0006 | 0.0006 | 131 072 |
| `meta/llama-3.1-70b-instruct` | 0.00059 | 0.00079 | 131 072 |

---

## Status

**v0 — pivoted to pi-coding-agent.**

The original Crow repo (Rust kernel + TUI + session storage) is
archived in git at the previous `main` tip. See
`docs/PI_PIVOT_PLAN.md` for the rationale and the migration
checklist.

What's coming:
- Crow extensions package on npm (`@crow/coding-agent`)
- `/memory` and `/init` skills
- Lighter crow-desktop-bridge that uses pi's native RPC protocol
  instead of the desktop's bespoke one
- A web UI (Tauri doesn't fit every workflow)
