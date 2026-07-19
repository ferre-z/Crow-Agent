import { describe, expect, it, vi } from "vitest";

import { ApprovalGate, DEFAULT_DENY_REASON, type ApprovalRequest } from "./approvals.ts";

describe("ApprovalGate", () => {
  it("allows everything in auto mode without an approver", async () => {
    const gate = new ApprovalGate({ mode: "auto" });
    await expect(gate.check("c1", "bash", { command: "ls" })).resolves.toEqual({ allow: true });
  });

  it("defaults to auto mode", async () => {
    const gate = new ApprovalGate();
    expect(gate.mode).toBe("auto");
    await expect(gate.check("c1", "bash", {})).resolves.toEqual({ allow: true });
  });

  it("asks in ask mode and allows on allow", async () => {
    const seen: ApprovalRequest[] = [];
    const gate = new ApprovalGate({
      mode: "ask",
      ask: (req) => {
        seen.push(req);
        return Promise.resolve("allow");
      },
    });
    await expect(gate.check("c1", "bash", { command: "ls" })).resolves.toEqual({ allow: true });
    expect(seen).toEqual([{ callId: "c1", tool: "bash", args: { command: "ls" } }]);
  });

  it("skips the approver for tools in autoApproveTools", async () => {
    const ask = vi.fn(() => Promise.resolve("deny" as const));
    const gate = new ApprovalGate({ mode: "ask", autoApproveTools: ["read"], ask });
    await expect(gate.check("c1", "read", { path: "x" })).resolves.toEqual({ allow: true });
    expect(ask).not.toHaveBeenCalled();
    // Other tools still ask.
    await gate.check("c2", "bash", {});
    expect(ask).toHaveBeenCalledTimes(1);
  });

  it("stops asking for a tool after an always decision", async () => {
    const tools: string[] = [];
    const gate = new ApprovalGate({
      mode: "ask",
      ask: (req) => {
        tools.push(req.tool);
        return Promise.resolve("always");
      },
    });
    await expect(gate.check("c1", "bash", {})).resolves.toEqual({ allow: true });
    await expect(gate.check("c2", "bash", {})).resolves.toEqual({ allow: true });
    expect(tools).toEqual(["bash"]);
    // A different tool still asks.
    await gate.check("c3", "write", {});
    expect(tools).toEqual(["bash", "write"]);
  });

  it("returns the default reason on a bare deny", async () => {
    const gate = new ApprovalGate({ mode: "ask", ask: () => Promise.resolve("deny") });
    await expect(gate.check("c1", "bash", {})).resolves.toEqual({
      allow: false,
      reason: DEFAULT_DENY_REASON,
    });
  });

  it("passes through a verdict's deny reason", async () => {
    const gate = new ApprovalGate({
      mode: "ask",
      ask: () => Promise.resolve({ decision: "deny", reason: "approval timed out" }),
    });
    await expect(gate.check("c1", "bash", {})).resolves.toEqual({
      allow: false,
      reason: "approval timed out",
    });
  });

  it("denies in ask mode when no approver is configured", async () => {
    const gate = new ApprovalGate({ mode: "ask" });
    const result = await gate.check("c1", "bash", {});
    expect(result.allow).toBe(false);
    expect(result.reason).toBeTruthy();
  });

  it("keeps asking after a one-off allow", async () => {
    let calls = 0;
    const gate = new ApprovalGate({
      mode: "ask",
      ask: () => {
        calls += 1;
        return Promise.resolve("allow");
      },
    });
    await gate.check("c1", "bash", {});
    await gate.check("c2", "bash", {});
    expect(calls).toBe(2);
  });
});
