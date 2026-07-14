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

- Spec §8 config example needs a one-line update (rename `[provider]` block is unchanged; only paths). Will be patched in wave 2 once we know the file shape.
- All tests, docs, fixture filenames use `crow` (not `pale`).
- The `80 Workspace/Crow/` vault folder name stays.
