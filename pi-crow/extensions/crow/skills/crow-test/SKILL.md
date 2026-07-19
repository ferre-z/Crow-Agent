---
name: crow-test
description: Run the project's tests with smart defaults. Detects stack, picks the right test runner, skips network/integration tests by default unless the user asks for them.
---

# Crow Test

Run the project's tests intelligently. The user typed
`/crow-test`, `/test`, or asked "run the tests".

## Steps

1. Detect the test runner from the stack:
   - JS/TS: `npm test` (or `pnpm test` / `yarn test` if a
     `pnpm-lock.yaml` / `yarn.lock` exists).
   - Rust: `cargo test --workspace --no-fail-fast`.
   - Python: `pytest -x` (or `python -m unittest` if no pytest).
   - Go: `go test ./...`.
   - Java: `mvn test` (or `gradle test`).
   - Ruby: `bundle exec rspec` (or `rake test`).

2. Skip integration / e2e tests by default:
   - Pass `--testPathIgnorePatterns` (Jest) or equivalent.
   - For pytest, skip tests with `@pytest.mark.integration` or
     in a `tests/integration/` directory unless the user passed
     `--all`.
   - For Go, run `go test -short ./...`.
   - Surface a one-line summary of what was skipped so the user
     knows.

3. Time out after 5 minutes by default. If a test hangs, kill it
   and surface the partial output.

4. Summarise: pass/fail counts, slowest test (top 3 if many),
   any flaky tests (passed on retry only — re-run failures once).

## Arguments

`/crow-test [path] [--all] [--watch]`

- `path`: only run tests under this path.
- `--all`: include integration / e2e tests.
- `--watch`: re-run on file changes (default: one-shot).

## Boundaries

- Do NOT modify source files.
- Do NOT install dependencies.
- Do NOT push, commit, or create branches.
- If the test command would hit the network, warn before running.
