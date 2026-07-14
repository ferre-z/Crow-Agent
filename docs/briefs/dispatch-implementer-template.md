# Implementer dispatch template — Wave 1, Round A, Task 1.1

Copy this prompt, fill in the placeholders, and pass to `delegate_task`.

```
You are implementing task 1.1 of the Crow v0 agent (a Rust coding agent).
You will work in a git worktree. You will write code, run tests, commit,
and return a diff. You will NOT touch files outside the task's scope.

## Task brief (read first, this is your contract)

Read /home/ubuntu/code/crow/docs/briefs/wave-1/task-1-1.md in full.
It contains exact file paths, interfaces, acceptance criteria, and forbidden items.

## Source of truth

The v0 spec is at:
/home/ubuntu/ob-vault/30 Projects/Agent & ecosystem/08-Personal-Agent-v0-Spec.md
You may consult §6 (project structure) and §15 (CLI), but ONLY if the brief is
ambiguous. If the brief and spec conflict, the brief wins for this task and
you must flag the conflict in your return.

## Working directory

The worktree is at /home/ubuntu/code/crow/.worktrees/wave-1-foundation/
You are on branch wave-1-foundation. The main branch is sacred — never
commit to it. All your commits go to wave-1-foundation.

## Environment

The Hermes agent-env script has been pre-sourced in your shell:
- ANTHROPIC_BASE_URL=https://api.minimax.io/anthropic
- ANTHROPIC_API_KEY=<live sk-cp-*>
- ANTHROPIC_MODEL=MiniMax-M3

## What to do (TDD)

1. Read the brief end to end. Ask clarifying questions by returning a
   NEEDS_CONTEXT block if anything is truly ambiguous. Do NOT guess.
2. Run `cargo new --bin crow` style scaffolding INSIDE the worktree
   (the worktree is empty — main is at the bootstrap commit only).
3. For each file in the brief's "Create" list, write the failing test
   first, run it to confirm RED, then write the implementation, then
   confirm GREEN.
4. After all files are in, run the FULL quality gate:
     cargo fmt --all --check
     cargo clippy --all-targets --all-features -- -D warnings
     cargo test --all-targets --all-features
5. Capture the actual stdout/stderr of every cargo command. Paste it
   verbatim in your return. Submissions without real cargo output are
   auto-rejected.
6. Commit on wave-1-foundation with a Conventional Commits message:
   "chore(workspace): Cargo crate + CI scaffolding (task 1.1)"
7. Return the report (see format below).

## Return format

Status: DONE | DONE_WITH_CONCERNS | NEEDS_CONTEXT | BLOCKED
Commits: <list of commit SHAs, short>
Files created: <list>
Files modified: <list>
Quality gate:
  - cargo fmt: <paste output, last 5 lines>
  - cargo clippy: <paste output, last 10 lines>
  - cargo test: <paste output, last 20 lines, including "test result: ok" line>
Concerns: <list, or "none">
Suggested follow-ups for the next task: <list, or "none">

## Hard rules

- Do not edit any file outside the brief's Create/Modify list.
- Do not add a dependency not in the brief. (The brief may say "add to
  Cargo.toml: tokio = { version = ..., features = [...] }" — only those.)
- Do not write a `#[ignore]` on a test to make it pass.
- Do not silence a clippy warning with #[allow(...)] without a comment
  explaining why.
- Do not run `cargo clean` or delete files outside the worktree.
- If you hit a blocker, return BLOCKED with a one-paragraph description
  of what you tried and what failed. Do not retry the same approach more
  than twice.
```
