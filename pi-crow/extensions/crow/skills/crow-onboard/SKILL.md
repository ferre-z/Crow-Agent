---
name: crow-onboard
description: First-run onboarding. Detect the project stack, write an AGENTS.md, and configure providers. Idempotent — safe to re-run.
---

# Crow Onboard

Onboarding skill for new Crow users. Invoked via `/crow-onboard`
or `/onboard`. Safe to re-run; only writes files that don't
exist or that the user confirms updating.

## Steps

1. Detect the project stack by reading:
   - `package.json` (Node/JS/TS)
   - `Cargo.toml` (Rust)
   - `pyproject.toml` / `requirements.txt` (Python)
   - `go.mod` (Go)
   - `pom.xml` / `build.gradle` (Java)
   - `*.csproj` / `*.sln` (C# / .NET)
   - `Gemfile` (Ruby)
   - `composer.json` (PHP)

   Stop at the first match; Crow assumes one primary stack per
   project.

2. Read existing files (don't overwrite silently):
   - `AGENTS.md`, `CLAUDE.md`, `.cursorrules`, `copilot-instructions.md`,
     `.github/copilot-instructions.md`

3. If `AGENTS.md` doesn't exist, draft one based on the detected
   stack with these sections:
   - **Stack**: detected languages and frameworks.
   - **Build / test commands**: extracted from the manifest.
   - **Conventions**: anything obviously enforced (formatter,
     linter, commit-message style).
   - **Don'ts**: anything that would obviously break the build
     (e.g. "don't run `npm test` — it hits the live DB").
   - **Repo layout**: 5-10 lines of the directory tree.

4. Ask the user to review the draft before writing.

5. Write `AGENTS.md` only after explicit approval.

6. Verify provider config:
   - Check `~/.pi/agent/settings.json` for `defaultProvider`.
   - If unset, suggest `nvidia` (Crow's default) or let the user
     pick.

## Boundaries

- Do NOT modify any source files during onboarding.
- Do NOT install dependencies or run package managers.
- AGENTS.md is the only file Crow writes.
- If the project already has AGENTS.md (or CLAUDE.md), don't
  overwrite — just summarise what's there and ask.
