import { describe, expect, it } from "vitest";
import {
  decodeFrame,
  encodeFrame,
  makeError,
  makeNotification,
  makeRequest,
  makeResult,
  RPC_ERRORS,
} from "./jsonrpc.ts";
import {
  approvalRespondParamsSchema,
  METHODS,
  methodParamsSchemas,
  NOTIFICATIONS,
  sessionCreateParamsSchema,
  sessionInfoSchema,
} from "./methods.ts";
import {
  approvalRequestEventSchema,
  EVENTS,
  eventParamsSchemas,
  sessionStateEventSchema,
} from "./events.ts";

describe("jsonrpc framing", () => {
  it("round-trips a request through encode/decode", () => {
    const req = makeRequest("1", METHODS.SESSION_CREATE, { cwd: "/tmp" });
    const frame = decodeFrame(encodeFrame(req).trimEnd());
    expect(frame).toEqual(req);
  });

  it("round-trips a result and an error", () => {
    expect(decodeFrame(encodeFrame(makeResult(1, { ok: true })).trimEnd())).toEqual(
      makeResult(1, { ok: true }),
    );
    const err = makeError(1, RPC_ERRORS.SESSION_NOT_FOUND, "no such session");
    expect(decodeFrame(encodeFrame(err).trimEnd())).toEqual(err);
  });

  it("rejects malformed frames", () => {
    expect(() => decodeFrame("not json")).toThrow();
    expect(() => decodeFrame('{"jsonrpc":"2.0"}')).toThrow();
  });
});

describe("method params", () => {
  it("validates session.create params", () => {
    const parsed = sessionCreateParamsSchema.parse({ cwd: "/work", model: "nvidia/some-model" });
    expect(parsed.cwd).toBe("/work");
    expect(parsed.model).toBe("nvidia/some-model");
  });

  it("accepts optional approval fields on session.create", () => {
    const parsed = sessionCreateParamsSchema.parse({
      cwd: "/work",
      approvalMode: "ask",
      autoApproveTools: ["read"],
    });
    expect(parsed.approvalMode).toBe("ask");
    expect(parsed.autoApproveTools).toEqual(["read"]);

    // Both fields are optional; omitting them preserves pre-P2 behavior.
    const bare = sessionCreateParamsSchema.parse({ cwd: "/work" });
    expect(bare.approvalMode).toBeUndefined();
    expect(bare.autoApproveTools).toBeUndefined();
  });

  it("rejects an unknown approvalMode on session.create", () => {
    expect(() => sessionCreateParamsSchema.parse({ cwd: "/work", approvalMode: "yolo" })).toThrow();
    expect(() =>
      sessionCreateParamsSchema.parse({ cwd: "/work", autoApproveTools: "read" }),
    ).toThrow();
  });

  it("requires approvalMode on session.list SessionInfo", () => {
    const info = {
      id: "s1",
      cwd: "/work",
      model: null,
      state: "idle",
      createdAt: "2026-01-01T00:00:00.000Z",
      approvalMode: "auto",
    };
    expect(sessionInfoSchema.parse(info)).toEqual(info);
    const { approvalMode: _omitted, ...withoutMode } = info;
    expect(() => sessionInfoSchema.parse(withoutMode)).toThrow();
  });

  it("rejects bad model refs", () => {
    expect(() => sessionCreateParamsSchema.parse({ cwd: "/work", model: "noslash" })).toThrow();
  });

  it("has a params schema for every method", () => {
    for (const method of Object.values(METHODS)) {
      expect(methodParamsSchemas[method]).toBeDefined();
    }
  });
});

describe("notifications", () => {
  it("validates approval.respond params", () => {
    for (const decision of ["allow", "deny", "always"] as const) {
      const parsed = approvalRespondParamsSchema.parse({ approvalId: "appr_1", decision });
      expect(parsed.decision).toBe(decision);
    }
    expect(() =>
      approvalRespondParamsSchema.parse({ approvalId: "appr_1", decision: "maybe" }),
    ).toThrow();
    expect(() => approvalRespondParamsSchema.parse({ decision: "allow" })).toThrow();
  });

  it("round-trips an approval.respond notification frame", () => {
    const n = makeNotification(NOTIFICATIONS.APPROVAL_RESPOND, {
      approvalId: "appr_1",
      decision: "always",
    });
    expect(decodeFrame(encodeFrame(n).trimEnd())).toEqual(n);
    expect("id" in n).toBe(false);
  });
});

describe("events", () => {
  it("validates a session_state event", () => {
    const ev = { sessionId: "s1", state: "streaming" };
    expect(sessionStateEventSchema.parse(ev)).toEqual(ev);
  });

  it("validates an approval_request event", () => {
    const ev = {
      sessionId: "s1",
      approvalId: "appr_1",
      callId: "call_1",
      tool: "bash",
      args: { command: "ls" },
    };
    expect(approvalRequestEventSchema.parse(ev)).toEqual(ev);
  });

  it("has a params schema for every event", () => {
    for (const event of Object.values(EVENTS)) {
      expect(eventParamsSchemas[event]).toBeDefined();
    }
  });

  it("accepts every event notification frame", () => {
    const n = makeNotification(EVENTS.TOKEN, { sessionId: "s1", text: "hi" });
    expect(decodeFrame(encodeFrame(n).trimEnd())).toEqual(n);
  });
});
