### Task 2.1 — Provider-neutral stream processor

Adds to `src/provider/stream.rs`:
- `pub struct StreamAccumulator { ... }` that buffers fragmented tool-call JSON, merges text/reasoning deltas, yields `AgentEvent`s in source order
- Handles UTF-8 boundaries at chunk edges
- Triggers `Completed { message, usage, stop_reason }` exactly once per stream
- Detects malformed JSON and emits `Failed { code: "stream_invalid", ... }`

**Spec:** §9 (provider events `Started/TextDelta/ReasoningDelta/ToolCallDelta/Completed/Failed`).
**Acceptance:** 8+ tests including fragmented JSON merge, UTF-8 split across chunks, double `Completed`, malformed mid-stream.
