---
type: wave-plan
status: detailed
wave: 3
phase: 2 + 3 (per 07-Build-Roadmap.md)
parent: 00-master-plan.md
---

# Wave 3 — Mutation tools + crash recovery (Phases 2+3)

**Goal:** `write`, `edit`, `bash` tools + interrupted-run recovery. Green gate: force-kill the process at every recorded state → restart → valid resumable session.

**Builds on:** Wave 2 (agent loop, tool registry, session, cancellation, read tool, AGENTS.md discovery, headless CLI).

> **Spec reminder:** v0 spec §3.2 explicitly excludes **OS-level sandboxing, MCP, skills, plugins, hooks, subagents, swarms**. We do NOT add `bubblewrap`, `landlock`, or any sandbox. We rely on process-group termination + project-root confinement (for the dedicated file tools only — bash is full user-level by spec design).

## Dispatch strategy (revised post-wave-1)

Mostly sequential. The wave 1 lesson: implementers don't reliably have access to the post-merge API of their dependencies. Branching each task from main AFTER the previous task's merge eliminates that class of bug.

| Round | Tasks | Strategy |
|---|---|---|
| **H** | 3.1 + 3.2 + 3.3 | **Parallel** (different files, no shared state) |
| **I** | 3.4 | Single task (depends on all H tasks) |
| **J** | 3.5 + 3.6 | **Sequential** (security tests first, then CLI + integration) |

## Dependency map

```
3.1 write tool  ─────┐
                     ├── 3.4 crash recovery ── 3.6 sessions + resume
3.2 edit tool   ─────┤
                     │
3.3 bash tool   ─────┴── 3.5 symlink/path escape security tests
```

## Tasks (full briefs in `docs/briefs/wave-3/task-N-M.md`)

| # | Task | File | Round |
|---|---|---|---|
| 3.1 | `write` tool (atomic temp+rename, parent dirs, diff) | `task-3-1.md` | H |
| 3.2 | `edit` tool (exact match, 0/1/many enforcement) | `task-3-2.md` | H |
| 3.3 | `bash` tool (process group, timeout, byte caps) | `task-3-3.md` | H |
| 3.4 | Crash recovery (`RunInterrupted` on read) | `task-3-4.md` | I |
| 3.5 | Symlink + path escape security tests | `task-3-5.md` | J |
| 3.6 | `crow sessions` + `crow --resume` + full §18 integration suite | `task-3-6.md` | J |

## Decision log to update

- `docs/decisions/07-secret-redaction-patterns.md` — what patterns the redaction list matches, where it lives, and the explicit statement that it's not a security boundary (added in 3.1)
- `docs/decisions/08-trust-model-v0.md` — restate the spec's "trusted-user tool" model and what wave 3 does NOT add (no sandbox, no permission prompts) (added in 3.3)
