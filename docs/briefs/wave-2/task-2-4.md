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
