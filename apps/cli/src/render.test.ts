import { EVENTS } from "@crow/protocol";
import { describe, expect, it } from "vitest";

import { renderEvent } from "./render.js";

describe("renderEvent", () => {
  it("renders tokens", () => {
    expect(renderEvent(EVENTS.TOKEN, { sessionId: "s1", text: "hi" })).toBe("hi");
  });

  it("renders thinking dimly", () => {
    const out = renderEvent(EVENTS.THINKING, { sessionId: "s1", text: "hmm" });
    expect(out).toContain("thinking");
  });

  it("renders tool call", () => {
    const out = renderEvent(EVENTS.TOOL_CALL, {
      sessionId: "s1",
      callId: "c1",
      tool: "read",
      args: { path: "file.txt" },
    });
    expect(out).toContain("→ read");
    expect(out).toContain('"file.txt"');
  });

  it("renders tool result error", () => {
    const out = renderEvent(EVENTS.TOOL_RESULT, {
      sessionId: "s1",
      callId: "c1",
      tool: "bash",
      output: "nope",
      isError: true,
    });
    expect(out).toContain("← bash [error]");
    expect(out).toContain("nope");
  });

  it("renders session state changes", () => {
    const out = renderEvent(EVENTS.SESSION_STATE, { sessionId: "s1", state: "idle" });
    expect(out).toContain("[state idle]");
  });

  it("renders errors", () => {
    const out = renderEvent(EVENTS.SESSION_STATE, {
      sessionId: "s1",
      state: "error",
      error: "boom",
    });
    expect(out).toContain("session error: boom");
  });

  it("returns null for unknown events", () => {
    expect(renderEvent("event.unknown", {})).toBeNull();
  });
});
