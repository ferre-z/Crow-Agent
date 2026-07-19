import { EVENTS, type SessionInfo } from "@crow/protocol";
import { describe, expect, it } from "vitest";

import type { DaemonEventFrame, HostInfoResult } from "../../shared/api.ts";
import {
  basename,
  initialState,
  reducer,
  selectActiveSession,
  selectCurrentApproval,
  selectSessions,
  sessionDisplayName,
  type Action,
  type AppState,
  type SessionEntry,
} from "./state.ts";

const HOST: HostInfoResult = {
  hostname: "devbox",
  platform: "linux",
  arch: "arm64",
  node: "v22.0.0",
  daemonVersion: "0.1.0",
  protocolVersion: "0.1.0",
  sessions: 0,
};

function sessionInfo(id: string, cwd: string, state: "idle" | "busy" = "idle"): SessionInfo {
  return {
    id,
    cwd,
    model: "faux/faux-1",
    state,
    createdAt: "2026-07-19T00:00:00Z",
    approvalMode: "auto",
  };
}

function event(method: string, params: unknown): Action {
  const frame: DaemonEventFrame = { method, params };
  return { type: "daemon.event", frame };
}

function connectedState(sessions: SessionInfo[] = []): AppState {
  return reducer(initialState(), {
    type: "connect.succeeded",
    hostName: "local",
    info: HOST,
    sessions,
  });
}

function entry(state: AppState, sessionId: string): SessionEntry {
  const found = state.sessions[sessionId];
  if (!found) throw new Error(`no session entry for ${sessionId}`);
  return found;
}

describe("connection lifecycle", () => {
  it("starts disconnected with empty state", () => {
    const state = initialState();
    expect(state.connection).toBe("disconnected");
    expect(state.hosts).toEqual([]);
    expect(selectSessions(state)).toEqual([]);
    expect(selectCurrentApproval(state)).toBeUndefined();
  });

  it("stores the host list", () => {
    const hosts = [{ name: "local", url: "ws://127.0.0.1:7749", token: "t" }];
    const state = reducer(initialState(), { type: "hosts.set", hosts });
    expect(state.hosts).toEqual(hosts);
  });

  it("connect.started shows progress and clears the previous error", () => {
    const failed = reducer(initialState(), { type: "connect.failed", message: "boom" });
    const state = reducer(failed, { type: "connect.started" });
    expect(state.connecting).toBe(true);
    expect(state.connectError).toBeUndefined();
  });

  it("connect.succeeded seeds sessions and maps coarse busy to streaming", () => {
    const state = connectedState([sessionInfo("s1", "/home/u/a"), sessionInfo("s2", "/b", "busy")]);
    expect(state.connection).toBe("connected");
    expect(state.hostInfo?.hostname).toBe("devbox");
    expect(entry(state, "s1").live).toBe("idle");
    expect(entry(state, "s2").live).toBe("streaming");
    expect(selectSessions(state).map((s) => s.info.id)).toEqual(["s1", "s2"]);
  });

  it("connect.failed keeps the app on the connect screen with an inline error", () => {
    const state = reducer(initialState(), { type: "connect.failed", message: "auth failed" });
    expect(state.connection).toBe("disconnected");
    expect(state.connecting).toBe(false);
    expect(state.connectError).toBe("auth failed");
  });

  it("disconnect.requested resets session state but keeps saved hosts", () => {
    const hosts = [{ name: "local", url: "ws://x", token: "t" }];
    let state = reducer(initialState(), { type: "hosts.set", hosts });
    state = reducer(state, {
      type: "connect.succeeded",
      hostName: "local",
      info: HOST,
      sessions: [sessionInfo("s1", "/a")],
    });
    state = reducer(state, { type: "disconnect.requested" });
    expect(state.connection).toBe("disconnected");
    expect(selectSessions(state)).toEqual([]);
    expect(state.hostInfo).toBeUndefined();
    expect(state.hosts).toEqual(hosts);
  });

  it("an unexpected socket drop returns to the connect screen with a notice", () => {
    let state = connectedState([sessionInfo("s1", "/a")]);
    state = reducer(state, { type: "daemon.connection", state: "disconnected" });
    expect(state.connection).toBe("disconnected");
    expect(state.connectError).toBe("lost connection to local");
    expect(selectSessions(state)).toEqual([]);
  });

  it("ignores duplicate disconnected pushes and connected pushes", () => {
    const disconnected = reducer(initialState(), {
      type: "daemon.connection",
      state: "disconnected",
    });
    expect(disconnected).toEqual(initialState());

    const connected = connectedState();
    expect(reducer(connected, { type: "daemon.connection", state: "connected" })).toBe(connected);
  });
});

describe("session tracking", () => {
  it("sessions.set adds new sessions and preserves live state of tracked ones", () => {
    let state = connectedState([sessionInfo("s1", "/a")]);
    state = reducer(
      state,
      event(EVENTS.SESSION_STATE, { sessionId: "s1", state: "error", error: "boom" }),
    );
    state = reducer(state, {
      type: "sessions.set",
      sessions: [sessionInfo("s1", "/a-renamed"), sessionInfo("s2", "/b", "busy")],
    });
    // Existing entry: info refreshed, live error state preserved.
    expect(entry(state, "s1").info.cwd).toBe("/a-renamed");
    expect(entry(state, "s1").live).toBe("error");
    // New entry: coarse state mapped.
    expect(entry(state, "s2").live).toBe("streaming");
    expect(selectSessions(state).map((s) => s.info.id)).toEqual(["s1", "s2"]);
  });

  it("session.created adds the session and makes it active", () => {
    let state = connectedState();
    state = reducer(state, { type: "session.created", info: sessionInfo("s9", "/proj") });
    expect(state.activeSessionId).toBe("s9");
    expect(entry(state, "s9").info.cwd).toBe("/proj");
  });

  it("session.selected switches only to known sessions", () => {
    let state = connectedState([sessionInfo("s1", "/a")]);
    state = reducer(state, { type: "session.selected", sessionId: "nope" });
    expect(state.activeSessionId).toBeUndefined();
    state = reducer(state, { type: "session.selected", sessionId: "s1" });
    expect(state.activeSessionId).toBe("s1");
    expect(selectActiveSession(state)?.info.id).toBe("s1");
  });
});

describe("transcript: text streaming", () => {
  it("prompt.sent appends a user item and marks the session streaming", () => {
    let state = connectedState([sessionInfo("s1", "/a")]);
    state = reducer(state, { type: "prompt.sent", sessionId: "s1", text: "hello" });
    const transcript = entry(state, "s1").transcript;
    expect(transcript).toEqual([{ kind: "user", id: "i1", text: "hello" }]);
    expect(entry(state, "s1").live).toBe("streaming");
  });

  it("accumulates token events into a single assistant item", () => {
    let state = connectedState([sessionInfo("s1", "/a")]);
    state = reducer(state, { type: "prompt.sent", sessionId: "s1", text: "hi" });
    state = reducer(state, event(EVENTS.TOKEN, { sessionId: "s1", text: "Hello" }));
    state = reducer(state, event(EVENTS.TOKEN, { sessionId: "s1", text: ", " }));
    state = reducer(state, event(EVENTS.TOKEN, { sessionId: "s1", text: "world" }));
    const transcript = entry(state, "s1").transcript;
    expect(transcript).toHaveLength(2);
    expect(transcript[1]).toEqual({ kind: "assistant", id: "i2", text: "Hello, world" });
  });

  it("starts a new assistant item after a user message or tool card", () => {
    let state = connectedState([sessionInfo("s1", "/a")]);
    state = reducer(state, event(EVENTS.TOKEN, { sessionId: "s1", text: "first" }));
    state = reducer(
      state,
      event(EVENTS.TOOL_CALL, { sessionId: "s1", callId: "c1", tool: "read", args: {} }),
    );
    state = reducer(state, event(EVENTS.TOKEN, { sessionId: "s1", text: "second" }));
    state = reducer(state, { type: "prompt.sent", sessionId: "s1", text: "again" });
    state = reducer(state, event(EVENTS.TOKEN, { sessionId: "s1", text: "third" }));
    const kinds = entry(state, "s1").transcript.map(
      (i) => `${i.kind}:${"text" in i ? i.text : ""}`,
    );
    expect(kinds).toEqual([
      "assistant:first",
      "tool:",
      "assistant:second",
      "user:again",
      "assistant:third",
    ]);
  });

  it("accumulates thinking separately from assistant text", () => {
    let state = connectedState([sessionInfo("s1", "/a")]);
    state = reducer(state, event(EVENTS.THINKING, { sessionId: "s1", text: "hmm" }));
    state = reducer(state, event(EVENTS.THINKING, { sessionId: "s1", text: "…" }));
    state = reducer(state, event(EVENTS.TOKEN, { sessionId: "s1", text: "answer" }));
    const transcript = entry(state, "s1").transcript;
    expect(transcript[0]).toEqual({ kind: "thinking", id: "i1", text: "hmm…" });
    expect(transcript[1]).toEqual({ kind: "assistant", id: "i2", text: "answer" });
  });

  it("tracks transcripts per session independently", () => {
    let state = connectedState([sessionInfo("s1", "/a"), sessionInfo("s2", "/b")]);
    state = reducer(state, event(EVENTS.TOKEN, { sessionId: "s1", text: "one" }));
    state = reducer(state, event(EVENTS.TOKEN, { sessionId: "s2", text: "two" }));
    expect(entry(state, "s1").transcript.map((i) => ("text" in i ? i.text : ""))).toEqual(["one"]);
    expect(entry(state, "s2").transcript.map((i) => ("text" in i ? i.text : ""))).toEqual(["two"]);
  });
});

describe("transcript: tool cards", () => {
  it("merges tool_result into the matching tool_call card by callId", () => {
    let state = connectedState([sessionInfo("s1", "/a")]);
    state = reducer(
      state,
      event(EVENTS.TOOL_CALL, { sessionId: "s1", callId: "c1", tool: "read", args: { path: "x" } }),
    );
    let card = entry(state, "s1").transcript[0];
    expect(card).toMatchObject({ kind: "tool", callId: "c1", done: false });

    state = reducer(
      state,
      event(EVENTS.TOOL_RESULT, {
        sessionId: "s1",
        callId: "c1",
        tool: "read",
        output: "file body",
        isError: false,
      }),
    );
    card = entry(state, "s1").transcript[0];
    expect(card).toMatchObject({
      kind: "tool",
      callId: "c1",
      done: true,
      output: "file body",
      isError: false,
    });
    expect(entry(state, "s1").transcript).toHaveLength(1);
  });

  it("keeps concurrent tool calls separate by callId", () => {
    let state = connectedState([sessionInfo("s1", "/a")]);
    for (const callId of ["c1", "c2"]) {
      state = reducer(
        state,
        event(EVENTS.TOOL_CALL, { sessionId: "s1", callId, tool: "bash", args: {} }),
      );
    }
    state = reducer(
      state,
      event(EVENTS.TOOL_RESULT, {
        sessionId: "s1",
        callId: "c2",
        tool: "bash",
        output: "two",
        isError: false,
      }),
    );
    const transcript = entry(state, "s1").transcript;
    expect(transcript[0]).toMatchObject({ callId: "c1", done: false });
    expect(transcript[1]).toMatchObject({ callId: "c2", done: true, output: "two" });
  });

  it("marks errored tool results", () => {
    let state = connectedState([sessionInfo("s1", "/a")]);
    state = reducer(
      state,
      event(EVENTS.TOOL_CALL, { sessionId: "s1", callId: "c1", tool: "bash", args: {} }),
    );
    state = reducer(
      state,
      event(EVENTS.TOOL_RESULT, {
        sessionId: "s1",
        callId: "c1",
        tool: "bash",
        output: "denied",
        isError: true,
      }),
    );
    expect(entry(state, "s1").transcript[0]).toMatchObject({
      done: true,
      isError: true,
      output: "denied",
    });
  });

  it("surfaces a tool_result with no matching call as a completed orphan card", () => {
    let state = connectedState([sessionInfo("s1", "/a")]);
    state = reducer(
      state,
      event(EVENTS.TOOL_RESULT, {
        sessionId: "s1",
        callId: "ghost",
        tool: "read",
        output: "x",
        isError: false,
      }),
    );
    expect(entry(state, "s1").transcript[0]).toMatchObject({
      kind: "tool",
      callId: "ghost",
      done: true,
    });
  });
});

describe("session_state events", () => {
  it("streaming then idle drives the live indicator and clears errors", () => {
    let state = connectedState([sessionInfo("s1", "/a")]);
    state = reducer(state, event(EVENTS.SESSION_STATE, { sessionId: "s1", state: "streaming" }));
    expect(entry(state, "s1").live).toBe("streaming");
    state = reducer(
      state,
      event(EVENTS.SESSION_STATE, { sessionId: "s1", state: "error", error: "boom" }),
    );
    expect(entry(state, "s1").live).toBe("error");
    expect(entry(state, "s1").error).toBe("boom");
    state = reducer(state, event(EVENTS.SESSION_STATE, { sessionId: "s1", state: "idle" }));
    expect(entry(state, "s1").live).toBe("idle");
    expect(entry(state, "s1").error).toBeUndefined();
  });

  it("maps the abort error after cancel to a cancelled state, not an error", () => {
    let state = connectedState([sessionInfo("s1", "/a")]);
    state = reducer(
      state,
      event(EVENTS.SESSION_STATE, {
        sessionId: "s1",
        state: "error",
        error: "Request was aborted",
      }),
    );
    expect(entry(state, "s1").live).toBe("cancelled");
    expect(entry(state, "s1").error).toBeUndefined();
  });

  it("creates a stub entry for events from an untracked session", () => {
    let state = connectedState();
    state = reducer(state, event(EVENTS.TOKEN, { sessionId: "ghost", text: "hi" }));
    expect(entry(state, "ghost").transcript).toHaveLength(1);
    expect(sessionDisplayName(entry(state, "ghost"))).toBe("session ghost".slice(0, 15));
  });

  it("ignores malformed events and unknown methods", () => {
    const state = connectedState([sessionInfo("s1", "/a")]);
    const cases: Action[] = [
      event(EVENTS.TOKEN, { text: "no session id" }),
      event(EVENTS.TOKEN, { sessionId: "s1" }),
      event(EVENTS.SESSION_STATE, { sessionId: "s1", state: "bogus" }),
      event("event.bogus", { sessionId: "s1" }),
      event(EVENTS.TOOL_CALL, { sessionId: "s1", callId: "c1" }),
    ];
    for (const action of cases) {
      expect(reducer(state, action)).toBe(state);
    }
  });
});

describe("approvals", () => {
  const request = {
    sessionId: "s1",
    approvalId: "a1",
    callId: "c1",
    tool: "bash",
    args: { command: "rm -rf /tmp/x" },
  };

  it("queues approval requests and mirrors them into the transcript", () => {
    let state = connectedState([sessionInfo("s1", "/a")]);
    state = reducer(state, event("event.approval_request", request));
    expect(selectCurrentApproval(state)).toMatchObject({ approvalId: "a1", tool: "bash" });
    expect(entry(state, "s1").transcript[0]).toMatchObject({ kind: "approval", approvalId: "a1" });
  });

  it("ignores a duplicate approval id", () => {
    let state = connectedState([sessionInfo("s1", "/a")]);
    state = reducer(state, event("event.approval_request", request));
    const again = reducer(state, event("event.approval_request", request));
    expect(again).toBe(state);
  });

  it("responded approvals leave the queue head-first and record the decision", () => {
    let state = connectedState([sessionInfo("s1", "/a")]);
    state = reducer(state, event("event.approval_request", request));
    state = reducer(
      state,
      event("event.approval_request", { ...request, approvalId: "a2", callId: "c2" }),
    );
    expect(state.pendingApprovals.map((a) => a.approvalId)).toEqual(["a1", "a2"]);

    state = reducer(state, { type: "approval.responded", approvalId: "a1", decision: "allow" });
    expect(state.pendingApprovals.map((a) => a.approvalId)).toEqual(["a2"]);
    expect(selectCurrentApproval(state)?.approvalId).toBe("a2");
    expect(entry(state, "s1").transcript[0]).toMatchObject({
      kind: "approval",
      approvalId: "a1",
      decision: "allow",
    });
    expect(entry(state, "s1").transcript[1]).toMatchObject({ kind: "approval", approvalId: "a2" });
    expect(
      entry(state, "s1").transcript[1] &&
        (entry(state, "s1").transcript[1] as { decision?: string }).decision,
    ).toBeUndefined();
  });

  it("responding to an unknown approval id is a harmless no-op on transcripts", () => {
    let state = connectedState([sessionInfo("s1", "/a")]);
    state = reducer(state, { type: "approval.responded", approvalId: "ghost", decision: "deny" });
    expect(state.pendingApprovals).toEqual([]);
    expect(entry(state, "s1").transcript).toEqual([]);
  });
});

describe("selectors", () => {
  it("basename handles trailing slashes, nesting, and windows separators", () => {
    expect(basename("/home/u/proj")).toBe("proj");
    expect(basename("/home/u/proj/")).toBe("proj");
    expect(basename("/")).toBe("");
    expect(basename("~")).toBe("~");
    expect(basename("C:\\Users\\u\\proj")).toBe("proj");
  });

  it("sessionDisplayName falls back to the session id without a cwd", () => {
    const state = connectedState([sessionInfo("s1", "/home/u/proj")]);
    expect(sessionDisplayName(entry(state, "s1"))).toBe("proj");
  });

  it("selectSessions returns sessions in insertion order", () => {
    let state = connectedState([sessionInfo("s1", "/a")]);
    state = reducer(state, event(EVENTS.TOKEN, { sessionId: "s0", text: "x" }));
    expect(selectSessions(state).map((s) => s.info.id)).toEqual(["s1", "s0"]);
  });
});
