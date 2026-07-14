### Task 2.2 — `genai` 0.6.5 adapter (real provider, behind `Provider` trait)

Adds to `src/provider/genai.rs`:
- `pub struct GenaiProvider { client: genai::Client, model: String, base_url: String }`
- `impl Provider for GenaiProvider` using the stream API
- Maps `genai` chat events → `AgentEvent` via the accumulator from 2.1
- Reads `NVIDIA_API_KEY` from env at runtime only
- Builds `tools_schema` from the tool registry's `schemars::Schema`s

**Spec:** §9, §8 config.
**Acceptance:** 6+ tests, all opt-in. Without `NVIDIA_API_KEY`, the live smoke test is `#[ignore]`. With it, a budget-capped smoke test (1 turn, 1 tool call) is included.
**Critical constraint:** raw `genai` events MUST NOT leak to the rest of the crate. The accumulator is the only path.
