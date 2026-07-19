---
name: crow-plan
description: Enter Crow plan mode. Read the task, write a plan to `.crow/plans/<id>.md`, present it for approval, and DO NOT execute the task until the user approves.
---

# Crow Plan Mode

Plan-mode skill. The user typed `/crow-plan`, `/plan`, or asked
"plan X first". The goal is a written, reviewable plan — no
mutations, no tool calls beyond `read`.

## When to invoke

The user explicitly asked for a plan, or is at the start of a
multi-step task.

## Steps

1. Confirm what they want planned. If unclear, ask one
   clarifying question (only one).
2. Generate a short ULID-style id (or reuse a date slug).
3. Write a plan file to `.crow/plans/<id>.md` with these sections:
   - **Goal**: one-paragraph summary.
   - **Non-goals**: what we're explicitly NOT doing.
   - **Steps**: numbered, each step small enough to review.
   - **Risks**: any unknowns, dependencies on external systems,
     things that might need user input.
   - **Open questions**: anything that needs the user to decide
     before we proceed.
4. Show the plan to the user inline in chat.
5. Wait for explicit approval ("go", "approved", "do it") before
   taking any action beyond reading.

## Boundaries

- During planning, ONLY use `read`. Do NOT use `write`, `edit`,
  `bash`, or any mutating tool.
- The plan file itself is a `write` — that one mutation is OK.
- Do NOT execute any step from the plan until the user approves.
- Do NOT modify AGENTS.md, package.json, Cargo.toml, or any
  dependency manifest during planning.
