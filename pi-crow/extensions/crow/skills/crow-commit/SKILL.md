---
name: crow-commit
description: Generate a Conventional Commit message from the staged diff and create the commit. Never push.
---

# Crow Commit

Conventional-Commit generator. The user typed `/crow-commit`,
`/commit`, or asked "commit this".

## Steps

1. Run `git diff --staged` to see the staged changes. If the
   staging area is empty, run `git diff` and stage everything the
   user wants committed (ask first if the diff is huge).
2. Detect the change shape:
   - New file → `feat` (or `chore` if it's tooling)
   - Bug fix → `fix`
   - Refactor with no behaviour change → `refactor`
   - Tests only → `test`
   - Docs only → `docs`
   - Build / CI / deps only → `build` or `ci`
   - Performance fix → `perf`
3. Check for breaking changes (`!:` footer in conventional
   commits). Surface them.
4. Pick the scope from the affected paths:
   - `git diff --name-only --staged` → derive a sensible scope.
   - If multiple modules, use the most affected one or omit.
5. Compose:
   ```
   <type>(<scope>): <subject>

   <body — 1-3 short paragraphs explaining what and why>

   <footer — BREAKING CHANGE, closes #N, etc.>
   ```
   - Subject ≤72 chars, imperative mood, no trailing period.
   - Body wraps at 72 chars.
6. Show the message in chat and ask for approval. Common
   adjustments: shorter subject, different scope, additional
   context.
7. Run `git commit -m "<subject>" -m "<body>"` (multi-line).
8. Show the resulting commit hash.

## Boundaries

- NEVER push. The user pushes manually after reviewing.
- NEVER amend or rewrite existing commits unless the user
  explicitly says "fix the last commit" or "amend".
- NEVER skip hooks (`--no-verify`).
- Do NOT stage files that look sensitive (`.env`, secrets,
  large binaries) without explicit confirmation.
