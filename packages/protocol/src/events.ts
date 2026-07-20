import { z } from "zod";

/**
 * Crow daemon events (daemon → client, sent as JSON-RPC notifications).
 * Method names use the `event.*` prefix per docs/protocol.md.
 */
export const EVENTS = {
  TOKEN: "event.token",
  THINKING: "event.thinking",
  TOOL_CALL: "event.tool_call",
  TOOL_RESULT: "event.tool_result",
  SESSION_STATE: "event.session_state",
  APPROVAL_REQUEST: "event.approval_request",
  AGENT: "event.agent",
  TEAM: "event.team",
} as const;

export const sessionStateSchema = z.enum(["idle", "streaming", "error"]);
export type SessionState = z.infer<typeof sessionStateSchema>;

export const tokenEventSchema = z.object({
  sessionId: z.string(),
  text: z.string(),
});
export type TokenEvent = z.infer<typeof tokenEventSchema>;

export const thinkingEventSchema = z.object({
  sessionId: z.string(),
  text: z.string(),
});
export type ThinkingEvent = z.infer<typeof thinkingEventSchema>;

export const toolCallEventSchema = z.object({
  sessionId: z.string(),
  callId: z.string(),
  tool: z.string(),
  args: z.unknown(),
});
export type ToolCallEvent = z.infer<typeof toolCallEventSchema>;

export const toolResultEventSchema = z.object({
  sessionId: z.string(),
  callId: z.string(),
  tool: z.string(),
  output: z.string(),
  isError: z.boolean(),
});
export type ToolResultEvent = z.infer<typeof toolResultEventSchema>;

export const sessionStateEventSchema = z.object({
  sessionId: z.string(),
  state: sessionStateSchema,
  error: z.string().optional(),
});
export type SessionStateEvent = z.infer<typeof sessionStateEventSchema>;

/**
 * Sent for each tool call that needs approval (session in "ask" mode). The
 * daemon holds the tool call until a client answers with an `approval.respond`
 * notification carrying this `approvalId`, or the approval times out.
 */
export const approvalRequestEventSchema = z.object({
  sessionId: z.string(),
  approvalId: z.string(),
  callId: z.string(),
  tool: z.string(),
  args: z.unknown(),
});
export type ApprovalRequestEvent = z.infer<typeof approvalRequestEventSchema>;

/**
 * Sub-agent lifecycle (P4). Broadcast to every connected client — not
 * session-scoped. "done" carries `output`; "error" carries `error`.
 */
export const agentEventSchema = z.object({
  agentId: z.string(),
  state: z.enum(["started", "done", "error"]),
  output: z.string().optional(),
  error: z.string().optional(),
});
export type AgentEvent = z.infer<typeof agentEventSchema>;

/**
 * Team run progress (P4). Broadcast to every connected client. `step` is
 * 1-based; the final "done" carries the last agent's `output`. A step failure
 * surfaces as "error" with `step`/`agent`/`error` set.
 */
export const teamEventSchema = z.object({
  runId: z.string(),
  state: z.enum(["step_started", "step_done", "done", "error"]),
  step: z.number().int().min(1).optional(),
  agent: z.string().optional(),
  output: z.string().optional(),
  error: z.string().optional(),
});
export type TeamEvent = z.infer<typeof teamEventSchema>;

/** Params validator per event method. */
export const eventParamsSchemas = {
  [EVENTS.TOKEN]: tokenEventSchema,
  [EVENTS.THINKING]: thinkingEventSchema,
  [EVENTS.TOOL_CALL]: toolCallEventSchema,
  [EVENTS.TOOL_RESULT]: toolResultEventSchema,
  [EVENTS.SESSION_STATE]: sessionStateEventSchema,
  [EVENTS.APPROVAL_REQUEST]: approvalRequestEventSchema,
  [EVENTS.AGENT]: agentEventSchema,
  [EVENTS.TEAM]: teamEventSchema,
} as const;
