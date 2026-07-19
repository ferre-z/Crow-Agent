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
import { METHODS, methodParamsSchemas, sessionCreateParamsSchema } from "./methods.ts";
import { EVENTS, sessionStateEventSchema } from "./events.ts";

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

  it("rejects bad model refs", () => {
    expect(() => sessionCreateParamsSchema.parse({ cwd: "/work", model: "noslash" })).toThrow();
  });

  it("has a params schema for every method", () => {
    for (const method of Object.values(METHODS)) {
      expect(methodParamsSchemas[method]).toBeDefined();
    }
  });
});

describe("events", () => {
  it("validates a session_state event", () => {
    const ev = { sessionId: "s1", state: "streaming" };
    expect(sessionStateEventSchema.parse(ev)).toEqual(ev);
  });

  it("accepts every event notification frame", () => {
    const n = makeNotification(EVENTS.TOKEN, { sessionId: "s1", text: "hi" });
    expect(decodeFrame(encodeFrame(n).trimEnd())).toEqual(n);
  });
});
