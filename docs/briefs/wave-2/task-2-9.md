### Task 2.9 — Nemotron API research (Nemotron Ultra subagent)

`docs/decisions/02-nemotron-genai-api.md`:
- Verify the NVIDIA endpoint URL (hosted vs self-hosted NIM)
- Confirm the exact model identifier for Nemotron 3 Ultra
- Document the tool-call streaming format (does the API return tool calls in deltas or as a single block?)
- Document the reasoning field (does `genai` 0.6.5 surface it for Nemotron?)
- Document the rate-limit response shape
- Document any `genai` quirks specific to this provider

**Acceptance:** the doc cites 2+ official sources (NVIDIA docs, model card, or `genai` source) per claim.

## Review gate (same as wave 1)

Two MiniMax M3 reviewers per task: spec compliance + code quality.
Reject if the implementer doesn't paste `cargo test` output.

## Decision log to update

- `docs/decisions/02-nemotron-genai-api.md` (from 2.9)
- `docs/decisions/03-context-size-estimation.md` (added in 2.6 if needed) — how we estimate context size for the `context_limit` error before real compaction ships
