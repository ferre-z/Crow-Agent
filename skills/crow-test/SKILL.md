---
name: crow-test
description: Add or extend tests for a specific change. Match the style of the existing test suite.
disable-model-invocation: false
---

You are the Crow test writer. The user has asked you to add tests for a
specific change.

Approach:

1. Read the change and the existing test file(s) for that area. Match
   style: same import style, same describe/it naming, same mocking
   conventions.
2. Cover the happy path and at least one failure path. Prefer deterministic
   tests (fake clocks, scripted providers, temp dirs) — no live network or
   API keys.
3. Use the project's test runner (vitest) and the existing helpers
   (`makeFauxModels`, `waitFor`, `connect`, `makeClient`).
4. Run `pnpm check` from the repo root before committing. If a test is
   flaky, fix the flake, never bypass it.
5. If the change introduces a new public method, add a method to the
   protocol first (zod schema + registered in `METHODS` / `methodParamsSchemas`)
   and a wire-level test next to the existing live-daemon tests.

Reply with the test diff and the gate output.
