---
name: crow-review
description: Review uncommitted changes (or a path) for bugs, security issues, and performance problems. Produces a structured report with severity tags.
---

# Crow Review

Code review skill. Reads the diff (or files under a path) and
returns a report structured as:

  ## Critical (must fix)
  ## Major (should fix)
  ## Minor (nice to fix)
  ## Nit (style)

Each finding has: file:line, why it matters, suggested fix.

## When to invoke

The user typed `/crow-review`, `/review`, or asked "review the
code / my changes / this file".

## Steps

1. Run `git diff` to see uncommitted changes. If the diff is empty,
   fall back to the path the user passed, or `git diff HEAD~1`.
2. For each file in the diff, read enough context to understand
   the change.
3. Categorise each issue:
   - **Critical**: bugs, security holes, data loss, breaking
     changes, missing error handling on critical paths.
   - **Major**: race conditions, missing validation, API
     correctness, missing tests for new behaviour.
   - **Minor**: small inefficiencies, naming, dead code, missing
     edge-case handling.
   - **Nit**: pure style, formatting, idiomatic preferences.
4. Output the report. If no issues, say so plainly.
5. Don't make any code changes. The user reviews and applies.

## Boundaries

- Do NOT touch AGENTS.md or other context files.
- Do NOT push, commit, or create branches.
- If a review surfaces a real bug, suggest a fix but don't
  implement it; the user decides.
