# Plan 01 — MCP integration

**Goal:** `@crow/core` sessions can use MCP servers — tools from configured
MCP servers appear as `AgentTool`s alongside the built-in coding tools.

## Why

The founding plan promised "skills, MCP, A2A" — MCP is the only one missing.
pi-agent-core ships **no** MCP client (verified by searching its dist), so
this is entirely Crow-owned: use the official `@modelcontextprotocol/sdk`.

## Current state

- Tool set: `createCodingTools(env)` in `packages/core/src/tools/index.ts` —
  returns `AgentTool[]` (TypeBox params).
- Session construction: `buildSessionHarness` in `packages/core/src/session.ts`
  — assembles env, tools, skills, system prompt.
- pi `AgentTool` shape: `{ name, label, description, parameters: TSchema,
execute(toolCallId, params, signal?, onUpdate?) => Promise<AgentToolResult> }`.
  Tool names are surfaced to the model as-is.

## Design

1. **New dep** (sanctioned change): `@modelcontextprotocol/sdk` in
   `@crow/core`. Use its `Client` + `StdioClientTransport` (stdio first;
   streamable-HTTP transport as a follow-up).
2. **New file** `packages/core/src/mcp.ts`:
   - `McpServerConfig = { name: string; command: string; args?: string[];
env?: Record<string,string> }` (stdio shape).
   - `class McpManager { constructor(configs: McpServerConfig[]);
connect(): Promise<void>; getTools(): AgentTool[];
close(): Promise<void> }`
   - Per server: spawn client, `listTools()`, convert each MCP tool to an
     `AgentTool` named `mcp_<server>_<tool>` (avoid collisions with built-ins).
   - Schema conversion: MCP tools carry JSON Schema input schemas; pi wants
     TypeBox. TypeBox 1.x is structurally JSON-Schema — pass the schema
     through as `parameters` (cast), and rely on pi's validation. If a
     schema uses constructs TypeBox rejects, wrap it in
     `Type.Any({ description })` and document the degradation.
   - `execute` → MCP `callTool`, map the MCP result content blocks
     (text/image) to `AgentToolResult.content`.
   - Timeouts + cancellation: pass the pi `signal` into the MCP call; kill
     server processes on `close()`.
3. **Wire into sessions**: `CreateSessionOptions` gains `mcpServers?:
McpServerConfig[]`; `CrowSessionManager.create` instantiates a
   `McpManager`, merges `getTools()` into the harness tool set, and closes it
   on `CrowSession.close()`. Failures to start one server must NOT kill
   session creation — log a diagnostic and continue without that server's
   tools.
4. **Config plumbing**: `session.create` wire params gain
   `mcpServers?: [{ name, command, args?, env? }]` (zod schema in
   `packages/protocol/src/methods.ts`). Daemon passes through. Desktop/CLI
   don't need UI in v1 (params are enough).

## Tests

- Unit: schema conversion (a sample JSON Schema → usable `AgentTool`),
  name-prefixing, close() kills the child process.
- Integration: a tiny fake MCP server (a Node script implementing the MCP
  stdio handshake with 1–2 tools — echo + add) as a test fixture in
  `packages/core/src/testing/`; session with `mcpServers` sees the tools and
  the faux provider can call them end-to-end (script a `fauxToolCall`).
- Daemon: `session.create` with a bad MCP server still succeeds (diagnostic
  only).

## Acceptance

- A real MCP server (e.g. the fixture) exposes its tools to a Crow session
  and the model can call them; `pnpm check` green; `docs/protocol.md`
  updated with `mcpServers`.
