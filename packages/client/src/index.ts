/**
 * @crow/client — typed client library for the Crow daemon wire protocol.
 *
 * Used by the Electron hub (apps/desktop, main process) today and by the
 * `crow` CLI in P3. See docs/protocol.md for the wire spec.
 */
export {
  CrowClient,
  type ConnectionState,
  type ConnectionStateListener,
  type CreateSessionParams,
  type CrowClientOptions,
  type DaemonEventListener,
} from "./client.ts";
export { CrowClientError } from "./errors.ts";
export {
  APPROVAL_REQUEST_EVENT,
  APPROVAL_RESPOND_METHOD,
  approvalDecisionSchema,
  approvalRequestEventSchema,
  approvalRespondParamsSchema,
  type ApprovalDecision,
  type ApprovalRequestEvent,
  type ApprovalRespondParams,
} from "./approval.ts";
export {
  EVENTS,
  METHODS,
  RPC_ERRORS,
  type HostInfoResult,
  type SessionCreateResult,
  type SessionInfo,
  type SessionListResult,
  type SessionState,
  type SessionStateEvent,
  type ThinkingEvent,
  type TokenEvent,
  type ToolCallEvent,
  type ToolResultEvent,
} from "@crow/protocol";
