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

/** Params validator per event method. */
export const eventParamsSchemas = {
  [EVENTS.TOKEN]: tokenEventSchema,
  [EVENTS.THINKING]: thinkingEventSchema,
  [EVENTS.TOOL_CALL]: toolCallEventSchema,
  [EVENTS.TOOL_RESULT]: toolResultEventSchema,
  [EVENTS.SESSION_STATE]: sessionStateEventSchema,
  [EVENTS.APPROVAL_REQUEST]: approvalRequestEventSchema,
} as const;
