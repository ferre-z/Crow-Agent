---
type: master-plan
status: draft
updated: 2026-07-14
owner: ferre
orchestrator: hermes
---

# Crow v0 — Master Wave Plan

> **Source of truth:** [`30 Projects/Agent & ecosystem/08-Personal-Agent-v0-Spec.md`](../../ob-vault/30%20Projects/Agent%20%26%20ecosystem/08-Personal-Agent-v0-Spec.md) + `07-Build-Roadmap.md` (Phases 0–4).
> **Project folder:** `80 Workspace/Crow/`
> **Implementation home:** `~/code/crow/` (this repo)
> **Binary name:** `crow` (binary rename from spec's `pale` — decided by Ferre 2026-07-14)

## Why waves, not phases

`07-Build-Roadmap.md` defines 8 phases (0–7) with acceptance gates. Phases are **sequential** — Phase 1 cannot start until Phase 0's gate is green. We respect that.

But within a phase, the work often has **independent slices** (e.g. Phase 0 has ID types, event envelope, JSONL server, mock provider — four parallel-friendly slices). We group those slices into **waves** for subagent dispatch.

A **wave** = one dispatch batch to a pool of subagents. Waves are ordered; tasks within a wave are dispatched in parallel.

## Routing rules (locked 2026-07-14)

| Task class | Model | Why |
|---|---|---|
| Mechanical Rust coding (1–2 files, complete spec) | MiniMax M3 | Default, 100M tok/day |
| Architecture, design, multi-file coordination | MiniMax M3 | Strong reasoning, daily budget |
| Small features, research, doc scraping | Nemotron Ultra | "Almost unlimited" |
| Debugging, refactoring complex | GLM-5.2 | Strongest, slow |

Review passes always MiniMax M3 (the orchestrator's own model, cheap and parallel).

## Quality gate (from `80 Workspace/Crow/AGENTS.md`)

```bash
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

Live Nemotron tests are opt-in and skipped without `NVIDIA_API_KEY`.

## The 3 waves for the first hour

### Wave 1 — Foundation (Phase 0 of roadmap, parallelized)

Goal: **deterministic skeleton** that can replay a scripted provider stream end-to-end. Green gate: scripted multi-turn replay produces byte-stable normalized events.

Tasks (all independent, dispatched in parallel — different files, no shared state):

| # | Task | Model | Output file(s) | Est. work |
|---|---|---|---|---|
| 1.1 | Cargo workspace + CI config (fmt, clippy, test) | MiniMax M3 | `Cargo.toml`, `rust-toolchain.toml`, `.github/workflows/ci.yml`, `clippy.toml` | S |
| 1.2 | ID + event-envelope types (`ids.rs`, `event.rs`, `message.rs`) | MiniMax M3 | `src/ids.rs`, `src/event.rs`, `src/message.rs` | M |
| 1.3 | JSONL session writer skeleton (append-only, atomic, versioned) | MiniMax M3 | `src/session.rs` | M |
| 1.4 | Scripted mock provider + fixture loader | MiniMax M3 | `src/provider/mock.rs`, `tests/fixtures/scripted_stream.jsonl` | M |
| 1.5 | Hierarchical cancellation primitive + tests | MiniMax M3 | `src/cancel.rs` | S |
| 1.6 | Public Cargo API smoke test (init, types visible) | MiniMax M3 | `tests/phase0_smoke.rs` | S |

**Dependencies:** 1.2 must be merged first (1.3, 1.4, 1.5 import from it). 1.1 must be merged first (everything else compiles). So: dispatch 1.1 → wait → dispatch {1.2, 1.5} in parallel → wait → dispatch {1.3, 1.4, 1.6} in parallel.

**Review gate:** 2 reviewers (MiniMax M3) per task: one spec-compliance, one code-quality. Spec reviewer checks against the v0 spec §6, §9, §10. Quality reviewer checks clippy/format/test pass.

### Wave 2 — Read-only agent loop (Phase 1 of roadmap, partially parallel)

Goal: **read-only agent loop + repo instructions + headless `exec`**. Green gate: harness can answer a question through multiple read tool cycles via the scripted provider.

Tasks:

| # | Task | Model | Output file(s) | Est. work |
|---|---|---|---|---|
| 2.1 | Provider-neutral stream processor (event accumulator, fragmented JSON merge) | MiniMax M3 | `src/provider/stream.rs` | M |
| 2.2 | Real `genai` 0.6.5 adapter (OpenAI-compatible) — *behind* `Provider` trait | MiniMax M3 | `src/provider/genai.rs` | M |
| 2.3 | `read` tool (path containment, line numbering, truncation) | MiniMax M3 | `src/tool/read.rs` | M |
| 2.4 | Tool registry + JSON Schema validation + output truncation | MiniMax M3 | `src/tool/mod.rs`, `src/tool/path.rs` | M |
| 2.5 | AGENTS.md discovery (walk root→cwd, deterministic order, hash recorded) | MiniMax M3 | `src/context.rs` | M |
| 2.6 | Agent state machine + turn counter + limits (max_turns, max_tool_calls) | MiniMax M3 | `src/agent.rs` | L |
| 2.7 | Headless `crow exec` subcommand (no TUI) | MiniMax M3 | `src/cli.rs` (extend), `src/main.rs` (extend) | S |
| 2.8 | Integration tests (text-only, read→tool-result→final, fragmented JSON, cancellation) | MiniMax M3 | `tests/agent_loop.rs` | M |
| 2.9 | Genai API research (verify endpoint, model id, tool-call streaming, reasoning field) | Nemotron Ultra | `docs/decisions/02-nemotron-genai-api.md` | S |

**Dependencies:** 2.1 needs 1.2 types. 2.3/2.4 need 1.5 cancellation. 2.6 needs {1.2, 1.3, 1.5, 2.1, 2.4}. 2.8 needs everything else.

So: dispatch 2.1+2.9 in parallel (independent) → wait → dispatch {2.2, 2.3, 2.4, 2.5} in parallel → wait → dispatch 2.6 → wait → dispatch 2.7+2.8 in parallel.

**Review gate:** same as wave 1, 2 reviewers per task. Spec reviewer checks against v0 spec §11 (agent loop), §12 (context), §13 (tool contracts), §14 (CLI), §17 (testing).

### Wave 3 — Mutation tools + persistence crash recovery (Phase 2 + 3 of roadmap)

Goal: **`write`, `edit`, `bash` tools + interrupted-run recovery**. Green gate: force-kill at every recorded state → restart → valid resumable session.

Tasks (sketched, will be detailed after wave 1 lands):

| # | Task | Model | Est. |
|---|---|---|---|
| 3.1 | `write` tool (atomic temp+rename, parent dirs, diff summary) | MiniMax M3 | M |
| 3.2 | `edit` tool (exact match, 0/1/many enforcement) | MiniMax M3 | M |
| 3.3 | `bash` tool (process group, timeout, byte caps, streaming events) | MiniMax M3 | L |
| 3.4 | Crash recovery (`RunInterrupted` append, resume from last complete boundary) | MiniMax M3 | L |
| 3.5 | Symlink swap + path escape security tests | MiniMax M3 | M |
| 3.6 | CLI `crow sessions` + `crow --resume` | MiniMax M3 | S |

## Reviewers

Every task gets two reviewers, dispatched in parallel after the implementer returns:

- **Spec reviewer (MiniMax M3):** did the code do what the spec says? Read the v0 spec section, check the diff against it. Output: ✅ or ❌ with line-cited findings.
- **Quality reviewer (MiniMax M3):** does it pass `cargo fmt --check` + `cargo clippy -D warnings` + `cargo test`? YAGNI? Test coverage on the edge cases the spec calls out (e.g. "zero/one/many match" for edit)? Output: ✅ or ❌ with line-cited findings.

## Critical-stance rules

- A reviewer finding a **Critical** or **Important** issue = implementer gets re-dispatched with the finding. No accepting "close enough."
- A reviewer finding a **Minor** issue = logged in the wave's decision log, fixed in a final polish subagent at end of wave.
- A reviewer saying "looks good" without citing the spec section = rejected, re-dispatched.
- A subagent claiming "compiled and tested" without pasting the actual `cargo test` output = rejected, re-dispatched.

## Worktree plan

- `main` is sacred, never touched
- One worktree per wave: `.worktrees/wave-1-foundation/`, `.worktrees/wave-2-readonly/`, `.worktrees/wave-3-mutation/`
- Each task within a wave: subagent does its work in a scratch dir, returns the diff; orchestrator applies it to the wave's worktree, runs the quality gate, then merges task branches into the wave branch

## Status

- [x] Master plan written
- [x] Wave 1 detailed brief → see `01-wave-1-foundation.md`
- [x] Wave 2 detailed brief → see `02-wave-2-readonly.md`
- [x] Wave 3 detailed brief → see `03-wave-3-mutation.md`
- [ ] Wave 1 dispatched → see `01-wave-1-foundation.md` status
- [ ] Wave 2 dispatched
- [ ] Wave 3 dispatched
