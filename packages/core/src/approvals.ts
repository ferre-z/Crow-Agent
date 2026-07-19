/**
 * Per-session tool-call approval gate (P2). Transport-agnostic: the daemon
 * injects an `ask` callback that fans `event.approval_request` out to attached
 * WS clients and resolves with their decision; tests inject a scripted one.
 *
 * The gate maps the harness `tool_call` hook onto a small decision protocol:
 * - "auto" mode: every call is allowed immediately (pre-P2 behavior).
 * - "ask" mode: calls pause until `ask` resolves, unless the tool is in
 *   `autoApproveTools` or was approved "always" earlier in the session.
 */

export type ApprovalMode = "auto" | "ask";

export type ApprovalDecision = "allow" | "deny" | "always";

/** One in-flight tool call awaiting an approval decision. */
export interface ApprovalRequest {
  callId: string;
  tool: string;
  args: unknown;
}

/**
 * Richer answer form for callers that own a deny reason (e.g. the daemon's
 * "approval timed out"); a bare {@link ApprovalDecision} is also accepted.
 */
export interface ApprovalVerdict {
  decision: ApprovalDecision;
  reason?: string;
}

export type ApprovalAsk = (request: ApprovalRequest) => Promise<ApprovalDecision | ApprovalVerdict>;

export interface ApprovalCheckResult {
  allow: boolean;
  /** Present when `allow` is false; becomes the error tool result's text. */
  reason?: string;
}

export interface ApprovalGateOptions {
  mode?: ApprovalMode;
  autoApproveTools?: string[];
  ask?: ApprovalAsk;
}

/** Reason used when a client answers "deny" without supplying its own. */
export const DEFAULT_DENY_REASON = "denied by user";

export class ApprovalGate {
  readonly mode: ApprovalMode;
  private readonly autoApproveTools: Set<string>;
  private readonly alwaysApproved = new Set<string>();
  private readonly ask?: ApprovalAsk;

  constructor(options: ApprovalGateOptions = {}) {
    this.mode = options.mode ?? "auto";
    this.autoApproveTools = new Set(options.autoApproveTools ?? []);
    this.ask = options.ask;
  }

  async check(callId: string, tool: string, args: unknown): Promise<ApprovalCheckResult> {
    if (this.mode === "auto") {
      return { allow: true };
    }
    if (this.autoApproveTools.has(tool) || this.alwaysApproved.has(tool)) {
      return { allow: true };
    }
    if (!this.ask) {
      return { allow: false, reason: "approval required but no approver is configured" };
    }
    const raw = await this.ask({ callId, tool, args });
    const verdict: ApprovalVerdict = typeof raw === "string" ? { decision: raw } : raw;
    switch (verdict.decision) {
      case "allow":
        return { allow: true };
      case "always":
        this.alwaysApproved.add(tool);
        return { allow: true };
      case "deny":
        return { allow: false, reason: verdict.reason ?? DEFAULT_DENY_REASON };
    }
  }
}
