# Plan 06 — Confinement symlink hardening

**Goal:** the `ConfinedExecutionEnv` can no longer be escaped via symlinks
inside the project root (documented gap in AGENTS.md).

## Current state

- `packages/core/src/env/confined-env.ts`: resolves each input path against
  the root and rejects when the _syntactic_ result escapes. A symlink
  `root/evil -> /etc` makes `root/evil/passwd` pass the check while reading
  outside the root. The gap is called out in a code comment.

## Design

1. On every path-touching op, after the syntactic check, call
   `canonicalPath` (resolves symlinks) when the path exists and re-check
   containment against the canonical form. Missing paths (write targets)
   can't be canonicalized — canonicalize the nearest existing ancestor
   instead and re-join the remainder.
2. Cost: one extra fs call per op. Acceptable for tools; bash stays
   cwd-confined only (document — full shell confinement is out of scope).
3. Keep behavior identical for non-symlink paths (tests must not regress).

## Tests

- In the existing tools/confinement tests (`packages/core/src/tools/tools.test.ts`):
  symlink inside root → outside target: read/write rejected; symlink inside
  root → inside target: allowed; symlink chain; write through a symlinked
  directory to a new file outside: rejected.

## Acceptance

- The four symlink cases behave as above; `pnpm check` green.
