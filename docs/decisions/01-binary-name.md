---
type: decision
status: accepted
date: 2026-07-14
---

# Decision 01 — Binary name is `crow`, not `pale`

## Context

v0 spec §8 specifies the working product name as `Pale` and the config directory as `~/.config/pale/`. The vault folder is named `Crow`. AGENTS.md uses "Crow" throughout. Ferre confirmed on 2026-07-14: keep `crow`.

## Decision

Binary name: `crow`. Library crate name: `crow`. Config directory: `~/.config/crow/`. Sessions directory: `~/.local/share/crow/sessions/`. CLI usage strings: `crow [PATH]`, `crow --resume ID`, `crow sessions`, `crow doctor`, `crow exec` (added in wave 2).

## Consequences

- **Spec §8 conflict:** the spec mandates `pale` as the binary name and `~/.config/pale/` for config. We diverge. Rationale: the project folder is named `Crow` and AGENTS.md uses "Crow" throughout — Ferre explicitly chose the rename on 2026-07-14 to keep naming consistent across vault, repo, binary, and config. If/when the spec is updated, this decision can be reverted.
- The `01-binary-name.md` decision log is the source of truth; spec §8 should be patched in a later revision to match.
