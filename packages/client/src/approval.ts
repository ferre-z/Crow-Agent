import { EVENTS, NOTIFICATIONS } from "@crow/protocol";

/**
 * Tool-approval wire names, re-exported from @crow/protocol so older client
 * imports (`@crow/client` → approval shim) keep working now that the protocol
 * package carries the canonical schemas.
 */
export {
  approvalDecisionSchema,
  approvalRequestEventSchema,
  approvalRespondParamsSchema,
  type ApprovalDecision,
  type ApprovalRequestEvent,
  type ApprovalRespondParams,
} from "@crow/protocol";

export const APPROVAL_REQUEST_EVENT = EVENTS.APPROVAL_REQUEST;
export const APPROVAL_RESPOND_METHOD = NOTIFICATIONS.APPROVAL_RESPOND;
