# Reviewer dispatch template

Copy this prompt, fill in placeholders, pass to `delegate_task` (2 reviewers per task).

```
You are reviewing a single task's diff. You are NOT editing code.
You are reporting findings only.

## Inputs

- Task brief: <path to docs/briefs/wave-N/task-N-M.md>
- Diff file: <path printed by scripts/review-package.sh BASE HEAD>
- Project root: /home/ubuntu/code/crow/.worktrees/wave-1-foundation/
- v0 spec (source of truth): /home/ubuntu/ob-vault/30 Projects/Agent & ecosystem/08-Personal-Agent-v0-Spec.md

## Review type

PICK ONE: SPEC_COMPLIANCE | CODE_QUALITY

### If SPEC_COMPLIANCE
- For every requirement in the task brief, cite the spec section and confirm ✅/❌.
- If ❌, quote the missing requirement and the actual code line.
- Check interfaces: for every interface in the brief, confirm the signature
  matches exactly (names, types, visibility, serde tags).
- Out-of-scope check: list anything added that the spec §3.2 excludes.
- Output ending: "Verdict: ✅ SPEC PASS" or "Verdict: ❌ SPEC FAIL (N findings)"

### If CODE_QUALITY
- Paste the actual cargo output the implementer returned. If they didn't
  include it, verdict ❌ immediately.
- YAGNI: list anything added that wasn't asked for.
- Test coverage: for every edge case the brief calls out, confirm a test.
  Missing = ⚠️ Minor (or 🔴 Critical if the spec requires it).
- Doc comments: every public item has `///` doc. Missing = ⚠️ Minor.
- `cargo fmt` + `cargo clippy -D warnings` + `cargo test` all pass.
- Output ending: "Verdict: ✅ QUALITY PASS" or "Verdict: ❌ QUALITY FAIL (N findings)"

## Hard rules for you

- Do NOT re-run the implementer's tests in a way that mutates the worktree.
  You may read files and run `cargo check` if you need to confirm a
  signature, but `cargo test` results must come from the implementer.
- Do NOT pre-judge: never say "treat X as Minor" or "the plan chose to...".
  If you find an issue, report it. The orchestrator decides severity.
- Quote the actual file and line, not a paraphrase.

## Return format

Verdict: ✅ or ❌
Findings: numbered list with file:line, severity, description.
Concerns (not blocking): numbered list.
```

## Convenience script

```bash
# Run from inside the worktree. Prints the diff to a file and echoes its path.
# Usage: scripts/review-package.sh BASE_REF HEAD_REF
# Example: scripts/review-package.sh main HEAD
```
