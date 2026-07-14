---
type: wave-plan
status: detailed
wave: 2
phase: 1 (per 07-Build-Roadmap.md)
parent: 00-master-plan.md
---

# Wave 2 — Read-only agent loop (Phase 1)

**Goal:** Read-only agent loop + repository instructions + headless `crow exec`. Green gate: harness can answer a repository question through multiple `read` tool cycles via the scripted provider.

**Worktree:** `.worktrees/wave-2-readonly/` on branch `wave-2-readonly`. Wave 1 branch must be merged first.
**Builds on:** Wave 1 (types, JSONL, cancellation, scripted provider).

## Dependency map

```
2.1 stream processor ────────┐
                             ├── 2.6 agent state machine
2.4 tool registry ───────────┘
        │
        ├── 2.3 read tool
        ├── 2.2 genai adapter
        ├── 2.5 AGENTS.md discovery
                │
                └── 2.7 headless `crow exec`
                        │
                        └── 2.8 integration tests
2.9 nemotron research ────────── (independent, can run anytime)
```

Wave 2 dispatches in 4 rounds:
- **Round D (parallel):** 2.1 + 2.4 + 2.9
- **Round E (parallel):** 2.2 + 2.3 + 2.5
- **Round F (1 task):** 2.6 (state machine, depends on most things)
- **Round G (parallel):** 2.7 + 2.8

## Tasks (interface sketches; full briefs in the dispatch packets)

### Task 2.1 — Provider-neutral stream processor

Adds to `src/provider/stream.rs`:
- `pub struct StreamAccumulator { ... }` that buffers fragmented tool-call JSON, merges text/reasoning deltas, yields `AgentEvent`s in source order
- Handles UTF-8 boundaries at chunk edges
- Triggers `Completed { message, usage, stop_reason }` exactly once per stream
- Detects malformed JSON and emits `Failed { code: "stream_invalid", ... }`

**Spec:** §9 (provider events `Started/TextDelta/ReasoningDelta/ToolCallDelta/Completed/Failed`).
**Acceptance:** 8+ tests including fragmented JSON merge, UTF-8 split across chunks, double `Completed`, malformed mid-stream.

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

### Task 2.3 — `read` tool

`src/tool/read.rs`:
- Args: `{ path: string, offset?: u32, limit?: u32 }`
- Validates: path is inside project root (rejects `..`, absolute outside root, symlinks pointing outside)
- Rejects directories and binary files (sniff first 8KB for NUL)
- Returns `{ content: string, line_count, truncated, byte_size }`
- Capped at 1 MB returned bytes; reports truncation

**Spec:** §13 read tool contract, §4 path containment.
**Acceptance:** 12+ tests including path escape via `..`, symlink swap after canonicalize, binary file, empty file, line offset past EOF, limit smaller than file.

### Task 2.4 — Tool registry + schema validation + output truncation

`src/tool/mod.rs`:
```rust
pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn schema(&self) -> schemars::Schema;
    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: ToolContext,
        events: ToolEventSink,
        cancel: CancelScope,
    ) -> Result<ToolResult, ToolError>;
}

pub struct ToolRegistry { /* HashMap<String, Arc<dyn Tool>> */ }
impl ToolRegistry {
    pub fn new() -> Self;
    pub fn register<T: Tool + 'static>(&mut self, t: T);
    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>>;
    pub fn names(&self) -> Vec<&'static str>;
    pub fn schemas_json(&self) -> serde_json::Value;  // for provider request
}

pub async fn execute_tool_call(
    reg: &ToolRegistry,
    call: &ToolCall,
    ctx: ToolContext,
    events: ToolEventSink,
    cancel: CancelScope,
) -> ToolOutcome;
```

`ToolContext`: `{ project_root: PathBuf, max_output_bytes: usize, command_timeout: Duration }`
`ToolEventSink`: `mpsc::Sender<AgentEvent>` (bounded, capacity 256, drops oldest deltas with a counter)

**Spec:** §13.
**Acceptance:** 8+ tests — schema validation rejects bad args, unknown tool returns error result not panic, output truncation at byte boundary, event sink backpressure.

### Task 2.5 — AGENTS.md discovery + context compiler

`src/context.rs`:
- `pub struct CompiledContext { system_prompt: String, instructions: Vec<(PathBuf, String, [u8; 32])>, total_hash: [u8; 32] }`
- `pub fn compile(project_root: &Path, cwd: &Path) -> Result<CompiledContext, ContextError>`
- Walks `project_root → cwd`, reads each `AGENTS.md`, records path + content + SHA-256
- Reads `system_prompt.md` (versioned in repo) for the system prompt
- `walk` uses `ignore::Walk` to respect `.gitignore`

**Spec:** §12.
**Acceptance:** 10+ tests — nested 3-deep resolution, AGENTS.md in unrelated subdir ignored, hash changes when content changes, missing root file is not an error (just empty list), permissions denied on a parent → returns the accessible prefix + warning entry.

### Task 2.6 — Agent state machine

`src/agent.rs`:
```rust
pub struct AgentConfig { pub max_turns: u32, pub max_tool_calls: u32, pub model: String, pub project_root: PathBuf, /* ... */ }
pub struct Agent {
    config: AgentConfig,
    provider: Arc<dyn Provider>,
    tools: Arc<ToolRegistry>,
    session: SessionWriter,
    cancel: CancelScope,
}

impl Agent {
    pub fn new(...) -> Self;
    pub async fn submit(&mut self, user_msg: Message) -> Result<AgentEvent, AgentError>;
    pub async fn cancel(&self);
    pub fn state(&self) -> AgentState;
}
```

Implements the loop from spec §11 verbatim:
- append and emit user message
- for turn in 1..=max_turns: compile context, stream provider, persist assistant, collect tool calls, execute sequentially, persist results, append to history
- max_turns / max_tool_calls enforcement
- provider 429/5xx/disconnect → typed error, no auto-retry
- cancellation preserves completed history

**Spec:** §11, §16.
**Acceptance:** 12+ tests including: text-only response exits after 1 turn, single tool call then final response, 3 sequential tool calls, tool error → model recovery, max_turns enforcement, max_tool_calls enforcement, cancellation mid-stream preserves history, session append failure aborts run, RunInterrupted entry written on cancel.

### Task 2.7 — Headless `crow exec`

Extends `src/cli.rs` + `src/main.rs`:
- Subcommand: `crow exec "task description"` — runs the agent loop without the TUI, prints events to stdout
- `crow sessions` — lists sessions, prints `(id, started_at, message_count, last_status)`
- `crow --resume ID` — reopens a session, replays history to stdout

**Spec:** §15. `exec` is added in this wave; spec doesn't require it but Phase 1 roadmap says "headless `exec` command".
**Acceptance:** 5+ tests + a CLI smoke test using `assert_cmd`.

### Task 2.8 — Integration test suite

`tests/agent_loop.rs`:
- scripted text-only response
- read → tool result → final response
- write/edit → bash test → final response (write/edit/bash landed in wave 3, this test will be marked `#[ignore]` until then with a comment)
- multiple sequential tool calls
- tool failure followed by model recovery
- cancellation during provider stream
- timeout and child-process cleanup
- crash-shaped incomplete JSONL followed by resume
- max-turn and max-tool-call enforcement
- terminal resize and panic restoration smoke tests (smoke tests get a real TUI in wave 4)

**Spec:** §17. The bash/write/edit integration tests are placeholders.
**Acceptance:** all tests that aren't `#[ignore]` pass without network. At least 7 tests must be non-ignored in this wave.

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
