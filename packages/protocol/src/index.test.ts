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
  agentSpawnParamsSchema,
  METHODS,
  methodParamsSchemas,
  NOTIFICATIONS,
  sessionCreateParamsSchema,
  sessionInfoSchema,
  teamListResultSchema,
  teamRunParamsSchema,
} from "./methods.ts";
import {
  agentEventSchema,
  approvalRequestEventSchema,
  EVENTS,
  eventParamsSchemas,
  sessionStateEventSchema,
  teamEventSchema,
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

  it("validates agent.spawn params (P4)", () => {
    const parsed = agentSpawnParamsSchema.parse({
      prompt: "fix the tests",
      cwd: "/work",
      systemPrompt: "You are careful.",
      tools: ["read", "edit"],
      model: "nvidia/some-model",
    });
    expect(parsed.tools).toEqual(["read", "edit"]);

    // Only prompt + cwd are required; absent tools means the full set.
    const bare = agentSpawnParamsSchema.parse({ prompt: "hi", cwd: "/work" });
    expect(bare.systemPrompt).toBeUndefined();
    expect(bare.tools).toBeUndefined();
    expect(bare.model).toBeUndefined();

    expect(() => agentSpawnParamsSchema.parse({ prompt: "", cwd: "/work" })).toThrow();
    expect(() => agentSpawnParamsSchema.parse({ prompt: "hi" })).toThrow();
    expect(() =>
      agentSpawnParamsSchema.parse({ prompt: "hi", cwd: "/work", model: "noslash" }),
    ).toThrow();
  });

  it("validates team.run params and the team.list result shape (P4)", () => {
    const parsed = teamRunParamsSchema.parse({
      team: "solo-review",
      input: "review this",
      cwd: "/w",
    });
    expect(parsed.model).toBeUndefined();
    expect(() => teamRunParamsSchema.parse({ team: "x", input: "y" })).toThrow();
    expect(() => teamRunParamsSchema.parse({ team: "", input: "y", cwd: "/w" })).toThrow();

    const listed = teamListResultSchema.parse({
      teams: [
        {
          name: "solo-review",
          description: "one reviewer",
          agents: [{ name: "reviewer", role: "Reviews the input" }],
        },
      ],
    });
    expect(listed.teams[0]?.agents[0]?.name).toBe("reviewer");
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

  it("validates agent lifecycle events (P4)", () => {
    expect(agentEventSchema.parse({ agentId: "agent_1", state: "started" })).toEqual({
      agentId: "agent_1",
      state: "started",
    });
    expect(
      agentEventSchema.parse({ agentId: "agent_1", state: "done", output: "result text" }),
    ).toEqual({ agentId: "agent_1", state: "done", output: "result text" });
    expect(agentEventSchema.parse({ agentId: "agent_1", state: "error", error: "boom" })).toEqual({
      agentId: "agent_1",
      state: "error",
      error: "boom",
    });
    expect(() => agentEventSchema.parse({ agentId: "agent_1", state: "running" })).toThrow();
  });

  it("validates team progress events (P4)", () => {
    const step = teamEventSchema.parse({
      runId: "run_1",
      state: "step_done",
      step: 2,
      agent: "implementer",
      output: "patched",
    });
    expect(step.step).toBe(2);
    expect(teamEventSchema.parse({ runId: "run_1", state: "done", output: "verdict" })).toEqual({
      runId: "run_1",
      state: "done",
      output: "verdict",
    });
    expect(
      teamEventSchema.parse({
        runId: "run_1",
        state: "error",
        step: 1,
        agent: "planner",
        error: "x",
      }),
    ).toEqual({ runId: "run_1", state: "error", step: 1, agent: "planner", error: "x" });
    // Steps are 1-based: 0 and unknown states are rejected.
    expect(() => teamEventSchema.parse({ runId: "r", state: "step_done", step: 0 })).toThrow();
    expect(() => teamEventSchema.parse({ runId: "r", state: "stepping" })).toThrow();
  });
});
