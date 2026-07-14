### Task 1.6 — Public API smoke

**Files:**
- Create: `tests/phase0_smoke.rs`
- Modify: `src/lib.rs` (re-export enough for the test to import)

**Spec references:** v0 spec §18 (acceptance criteria 10 — "Unit and integration suites pass without network access").

**What it does:**
- Imports every public type from `crow::*`
- Constructs one of each: `SessionId`, `RunId`, `MessageId`, `ToolCallId`, `AgentEvent`, `Message`, `SessionEntry`, `Provider` (via `ScriptedProvider`)
- Replays a one-event fixture end-to-end: open session, append started, append user, stream scripted provider, append assistant, finish
- Asserts: every call succeeds, no panics, no unwraps leaked, session file exists, `read_entries` returns the same 5 entries

**Acceptance:**
- `cargo test --test phase0_smoke` exits 0
- Runs without network (no provider URL, no real HTTP)
- The full sequence is byte-stable across runs (test asserts on a SHA256 of the session file content with a fixed `started_at` injected)

**Forbidden:** No `unwrap`/`expect` in the test body. No `tokio::test` without a `current_thread` runtime feature flag.

---

## Review rubrics

Both reviewers use the same template, dispatched in parallel after the implementer returns. Two MiniMax M3 subagents per task.

### Spec reviewer prompt (abbreviated)

> You are reviewing a single task's diff against the v0 spec.
> - Spec source: `~/code/crow/docs/spec/08-Personal-Agent-v0-Spec.md` (copy of the vault note)
> - Diff: <printed by orchestrator>
> - Task brief: this file, section "Task N.M"
>
> Output format:
> 1. **Spec coverage** — for every requirement in the task brief, cite the spec section and confirm ✅/❌. If ❌, quote the missing requirement and the actual code.
> 2. **Interface conformance** — for every interface in the task brief, confirm the signature matches exactly (names, types, visibility).
> 3. **Out-of-scope check** — list anything added that the spec excludes (e.g. permissions, MCP, SQLite). Each must be ❌.
> 4. **Verdict:** ✅ SPEC PASS or ❌ SPEC FAIL (with numbered findings).

### Quality reviewer prompt (abbreviated)

> You are reviewing a single task's diff for code quality and the project's quality gate.
> - Diff: <printed by orchestrator>
> - Quality gate: `cargo fmt --all --check && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-targets --all-features`
>
> Output format:
> 1. **Gate evidence** — paste the actual `cargo` output for fmt, clippy, test. If the implementer didn't include it, ❌ immediately.
> 2. **YAGNI / scope discipline** — list anything added that wasn't asked for. Each = ⚠️ Minor.
> 3. **Test coverage** — for every edge case the brief calls out, confirm a test exists. List missing.
> 4. **Doc comments** — every public item has `///` doc. Missing = ⚠️ Minor.
> 5. **Verdict:** ✅ QUALITY PASS or ❌ QUALITY FAIL (with numbered findings).

## Reject conditions (any one = re-dispatch the implementer)

- Implementer didn't paste the actual `cargo test` output
- Spec reviewer found a missing or wrong requirement
- Quality reviewer found a Critical (compile error, test fail, clippy warning)
- Diff includes files outside the task's "Create / Modify" list
- New dependency added without a decision doc (`docs/decisions/NN-...md`)

## Decision log

- `docs/decisions/01-binary-name.md` — why `crow` not `pale`
