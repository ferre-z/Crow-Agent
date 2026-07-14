### Task 7.1 — Plan mode

**Files:**
- Modify: `src/agent.rs` (the agent loop checks a `mode` flag and restricts tools)
- Modify: `crates/crow-desktop/src/frontend/components/composer.ts` (mode picker)
- Modify: `crates/crow-desktop/src/frontend/components/chat.ts` (render "Apply plan" button when in plan mode)

**Why this exists:** plan mode lets the user ask the model "what would you do?" without committing to any changes. The model runs with only `read`/`glob`/`grep` (no mutations). When it finishes, the response is presented as a "proposed plan" with an "Apply" button that re-runs the loop in build mode with the plan as context.

**Spec references:** spec §3.2 (no permission prompts in v0 — plan mode is a v0.1 extension; the kernel stays autonomous by default). The desktop adds a `mode` toggle.

**Behavior:**
- Composer has a mode picker: `Plan` / `Build`. Default `Build`.
- In `Plan` mode, the agent loop has access to a restricted tool registry (only `read`, `glob`, `grep`). All other tools return `ToolError::Denied("plan mode")`.
- The model's response in `Plan` mode is presented with an "Apply plan" button below the assistant message.
- Clicking "Apply plan" dispatches a new `Submit` with the plan text as a `User` message, the mode flipped to `Build`, and the model runs the plan.

**Implementation:**
- New `AgentConfig::mode: AgentMode { Plan, Build }`.
- New `ToolRegistry::restricted(read_tools) -> Self` that returns a registry containing only the read tools.
- The agent loop picks the registry based on the mode at session start.
- The desktop's IPC passes the mode to `SessionStart`.

**Procedure:**
1. Add `AgentMode` enum to `agent.rs`.
2. Add `restricted()` constructor to `ToolRegistry`.
3. Modify the `Agent` constructor to accept the mode + the right registry.
4. The desktop's composer has a `Plan`/`Build` toggle.
5. The chat detects plan-mode responses (no `ToolStarted` events for mutation tools) and renders the "Apply plan" button.
6. Tests: a unit test for `restricted()` and a test that the agent loop respects the mode.

**Acceptance:**
- Manual test: switch to Plan mode → submit "add a login button" → model responds with a plan (no actual edits).
- Click "Apply plan" → agent runs in Build mode → makes the actual edits.
- `cargo build --workspace` is clean.

**Forbidden:**
- No hard-coding the read-tool list in the agent loop (use `restricted()`).
- No "stealth" mode switching (the user must always see the mode).

**Dependency:** none new.
