import { EVENTS, type SessionInfo } from "@crow/protocol";
import { describe, expect, it } from "vitest";

import type { DaemonEventFrame, HostInfoResult } from "../../shared/api.ts";
import {
  basename,
  initialState,
  makeSessionKey,
  reducer,
  selectActiveHostName,
  selectActiveSession,
  selectConnectedHosts,
  selectCurrentApproval,
  selectSessions,
  sessionDisplayName,
  type Action,
  type AppState,
  type SessionEntry,
} from "./state.ts";

const HOST_INFO: HostInfoResult = {
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

function event(hostName: string, method: string, params: unknown): Action {
  const frame: DaemonEventFrame = { hostName, method, params };
  return { type: "daemon.event", frame };
}

function connectedState(
  hostName: string,
  sessions: SessionInfo[] = [],
  host: { name: string; url: string; token: string } = {
    name: hostName,
    url: "ws://x",
    token: "t",
  },
): AppState {
  let state = reducer(initialState(), { type: "hosts.set", hosts: [host] });
  state = reducer(state, { type: "connect.succeeded", hostName, info: HOST_INFO });
  if (sessions.length > 0) {
    state = reducer(state, { type: "sessions.set", hostName, sessions });
  }
  return state;
}

function entry(state: AppState, hostName: string, sessionId: string): SessionEntry {
  const key = makeSessionKey(hostName, sessionId);
  const found = state.sessions[key];
  if (!found) throw new Error(`no session entry for ${key}`);
  return found;
}

describe("connection lifecycle", () => {
  it("starts disconnected with empty state", () => {
    const state = initialState();
    expect(state.hosts).toEqual([]);
    expect(state.fleet).toEqual({});
    expect(state.sessions).toEqual({});
    expect(state.sessionOrder).toEqual([]);
    expect(selectSessions(state)).toEqual([]);
    expect(selectCurrentApproval(state)).toBeUndefined();
  });

  it("stores the host list", () => {
    const hosts = [{ name: "local", url: "ws://127.0.0.1:7749", token: "t" }];
    const state = reducer(initialState(), { type: "hosts.set", hosts });
    expect(state.hosts).toEqual(hosts);
  });

  it("fleet.set builds per-host entries", () => {
    const hosts = [
      { name: "local", url: "ws://127.0.0.1:7749", token: "a" },
      { name: "pi", url: "ws://192.168.1.20:7749", token: "b" },
    ];
    let state = reducer(initialState(), { type: "hosts.set", hosts });
    state = reducer(state, {
      type: "fleet.set",
      views: [
        { host: hosts[0]!, state: "connected", info: HOST_INFO },
        { host: hosts[1]!, state: "disconnected" },
      ],
    });
    expect(selectConnectedHosts(state)).toHaveLength(2);
    expect(state.fleet.local?.state).toBe("connected");
    expect(state.fleet.pi?.state).toBe("disconnected");
  });

  it("connect.started shows progress and clears the previous error", () => {
    let state = reducer(initialState(), {
      type: "connect.failed",
      hostName: "local",
      message: "boom",
    });
    expect(state.fleet.local?.error).toBe("boom");
    state = reducer(state, { type: "connect.started", hostName: "local" });
    expect(state.fleet.local?.connecting).toBe(true);
    expect(state.fleet.local?.error).toBeUndefined();
  });

  it("connect.started creates a stub fleet entry for an unknown host", () => {
    const state = reducer(initialState(), { type: "connect.started", hostName: "remote" });
    expect(state.fleet.remote?.host.name).toBe("remote");
    expect(state.fleet.remote?.state).toBe("disconnected");
    expect(state.fleet.remote?.connecting).toBe(true);
  });

  it("connect.succeeded stores host info and clears the error", () => {
    let state = reducer(initialState(), {
      type: "connect.failed",
      hostName: "local",
      message: "boom",
    });
    state = reducer(state, { type: "connect.succeeded", hostName: "local", info: HOST_INFO });
    expect(state.fleet.local?.state).toBe("connected");
    expect(state.fleet.local?.info?.hostname).toBe("devbox");
    expect(state.fleet.local?.error).toBeUndefined();
    expect(state.fleet.local?.connecting).toBe(false);
  });

  it("connect.failed keeps the host entry and records an inline error", () => {
    let state = reducer(initialState(), { type: "connect.started", hostName: "local" });
    state = reducer(state, { type: "connect.failed", hostName: "local", message: "auth failed" });
    expect(state.fleet.local?.state).toBe("disconnected");
    expect(state.fleet.local?.connecting).toBe(false);
    expect(state.fleet.local?.error).toBe("auth failed");
  });

  it("host.disconnect marks disconnected and drops that host's sessions", () => {
    let state = connectedState("local", [sessionInfo("s1", "/a")]);
    state = reducer(state, { type: "host.disconnect", hostName: "local" });
    expect(state.fleet.local?.state).toBe("disconnected");
    expect(selectSessions(state)).toEqual([]);
  });

  it("host.disconnect preserves sessions for other hosts", () => {
    let state = connectedState("local", [sessionInfo("s1", "/a")]);
    state = reducer(state, {
      type: "hosts.set",
      hosts: [...state.hosts, { name: "pi", url: "ws://pi", token: "t" }],
    });
    state = reducer(state, { type: "connect.succeeded", hostName: "pi", info: HOST_INFO });
    state = reducer(state, {
      type: "sessions.set",
      hostName: "pi",
      sessions: [sessionInfo("s2", "/b")],
    });
    state = reducer(state, { type: "host.disconnect", hostName: "local" });
    expect(selectSessions(state).map((s) => s.info.id)).toEqual(["s2"]);
  });

  it("host.remove deletes the fleet entry and drops its sessions", () => {
    let state = connectedState("local", [sessionInfo("s1", "/a")]);
    state = reducer(state, { type: "host.remove", hostName: "local" });
    expect(state.fleet.local).toBeUndefined();
    expect(selectSessions(state)).toEqual([]);
  });

  it("fleet.update patches an existing host entry", () => {
    let state = connectedState("local");
    state = reducer(state, {
      type: "fleet.update",
      hostName: "local",
      patch: { state: "disconnected" },
    });
    expect(state.fleet.local?.state).toBe("disconnected");
    expect(state.fleet.local?.info?.hostname).toBe("devbox");
  });

  it("fleet.update ignores unknown hosts", () => {
    const state = reducer(initialState(), {
      type: "fleet.update",
      hostName: "ghost",
      patch: { state: "connected" },
    });
    expect(state.fleet.ghost).toBeUndefined();
  });
});

describe("session tracking", () => {
  it("sessions.set adds sessions keyed by host:name and maps coarse busy to streaming", () => {
    let state = connectedState("local");
    state = reducer(state, {
      type: "sessions.set",
      hostName: "local",
      sessions: [sessionInfo("s1", "/a"), sessionInfo("s2", "/b", "busy")],
    });
    expect(entry(state, "local", "s1").live).toBe("idle");
    expect(entry(state, "local", "s2").live).toBe("streaming");
    expect(selectSessions(state).map((s) => makeSessionKey(s.hostName, s.info.id))).toEqual([
      "local:s1",
      "local:s2",
    ]);
  });

  it("sessions.set refreshes info but preserves live state of tracked sessions", () => {
    let state = connectedState("local", [sessionInfo("s1", "/a")]);
    state = reducer(
      state,
      event("local", EVENTS.SESSION_STATE, { sessionId: "s1", state: "error", error: "boom" }),
    );
    state = reducer(state, {
      type: "sessions.set",
      hostName: "local",
      sessions: [sessionInfo("s1", "/a-renamed"), sessionInfo("s2", "/b")],
    });
    expect(entry(state, "local", "s1").info.cwd).toBe("/a-renamed");
    expect(entry(state, "local", "s1").live).toBe("error");
  });

  it("session.created adds the session, makes it active, and maps coarse busy", () => {
    let state = connectedState("local");
    state = reducer(state, {
      type: "session.created",
      hostName: "local",
      info: sessionInfo("s9", "/proj", "busy"),
    });
    expect(state.activeSessionKey).toBe("local:s9");
    expect(entry(state, "local", "s9").info.cwd).toBe("/proj");
    expect(entry(state, "local", "s9").live).toBe("streaming");
  });

  it("session.selected switches only to known composite keys", () => {
    let state = connectedState("local", [sessionInfo("s1", "/a")]);
    state = reducer(state, { type: "session.selected", hostName: "local", sessionId: "nope" });
    expect(state.activeSessionKey).toBeUndefined();
    state = reducer(state, { type: "session.selected", hostName: "local", sessionId: "s1" });
    expect(state.activeSessionKey).toBe("local:s1");
    expect(selectActiveSession(state)?.info.id).toBe("s1");
  });

  it("the same sessionId on two hosts produces distinct entries", () => {
    let state = connectedState("local", [sessionInfo("s1", "/local")]);
    state = reducer(state, {
      type: "sessions.set",
      hostName: "pi",
      sessions: [sessionInfo("s1", "/pi")],
    });
    expect(entry(state, "local", "s1").info.cwd).toBe("/local");
    expect(entry(state, "pi", "s1").info.cwd).toBe("/pi");
    expect(selectSessions(state)).toHaveLength(2);
  });
});

describe("transcript: text streaming", () => {
  it("prompt.sent appends a user item and marks the session streaming", () => {
    let state = connectedState("local", [sessionInfo("s1", "/a")]);
    state = reducer(state, {
      type: "prompt.sent",
      hostName: "local",
      sessionId: "s1",
      text: "hello",
    });
    const transcript = entry(state, "local", "s1").transcript;
    expect(transcript).toEqual([{ kind: "user", id: "i1", text: "hello" }]);
    expect(entry(state, "local", "s1").live).toBe("streaming");
  });

  it("accumulates token events into a single assistant item", () => {
    let state = connectedState("local", [sessionInfo("s1", "/a")]);
    state = reducer(state, { type: "prompt.sent", hostName: "local", sessionId: "s1", text: "hi" });
    state = reducer(state, event("local", EVENTS.TOKEN, { sessionId: "s1", text: "Hello" }));
    state = reducer(state, event("local", EVENTS.TOKEN, { sessionId: "s1", text: ", " }));
    state = reducer(state, event("local", EVENTS.TOKEN, { sessionId: "s1", text: "world" }));
    const transcript = entry(state, "local", "s1").transcript;
    expect(transcript).toHaveLength(2);
    expect(transcript[1]).toEqual({ kind: "assistant", id: "i2", text: "Hello, world" });
  });

  it("starts a new assistant item after a user message or tool card", () => {
    let state = connectedState("local", [sessionInfo("s1", "/a")]);
    state = reducer(state, event("local", EVENTS.TOKEN, { sessionId: "s1", text: "first" }));
    state = reducer(
      state,
      event("local", EVENTS.TOOL_CALL, { sessionId: "s1", callId: "c1", tool: "read", args: {} }),
    );
    state = reducer(state, event("local", EVENTS.TOKEN, { sessionId: "s1", text: "second" }));
    state = reducer(state, {
      type: "prompt.sent",
      hostName: "local",
      sessionId: "s1",
      text: "again",
    });
    state = reducer(state, event("local", EVENTS.TOKEN, { sessionId: "s1", text: "third" }));
    const kinds = entry(state, "local", "s1").transcript.map(
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
    let state = connectedState("local", [sessionInfo("s1", "/a")]);
    state = reducer(state, event("local", EVENTS.THINKING, { sessionId: "s1", text: "hmm" }));
    state = reducer(state, event("local", EVENTS.THINKING, { sessionId: "s1", text: "…" }));
    state = reducer(state, event("local", EVENTS.TOKEN, { sessionId: "s1", text: "answer" }));
    const transcript = entry(state, "local", "s1").transcript;
    expect(transcript[0]).toEqual({ kind: "thinking", id: "i1", text: "hmm…" });
    expect(transcript[1]).toEqual({ kind: "assistant", id: "i2", text: "answer" });
  });

  it("tracks transcripts per session independently", () => {
    let state = connectedState("local", [sessionInfo("s1", "/a"), sessionInfo("s2", "/b")]);
    state = reducer(state, event("local", EVENTS.TOKEN, { sessionId: "s1", text: "one" }));
    state = reducer(state, event("local", EVENTS.TOKEN, { sessionId: "s2", text: "two" }));
    expect(entry(state, "local", "s1").transcript.map((i) => ("text" in i ? i.text : ""))).toEqual([
      "one",
    ]);
    expect(entry(state, "local", "s2").transcript.map((i) => ("text" in i ? i.text : ""))).toEqual([
      "two",
    ]);
  });

  it("keeps transcripts for the same sessionId on different hosts isolated", () => {
    let state = connectedState("local", [sessionInfo("s1", "/a")]);
    state = reducer(state, {
      type: "sessions.set",
      hostName: "pi",
      sessions: [sessionInfo("s1", "/b")],
    });
    state = reducer(state, event("local", EVENTS.TOKEN, { sessionId: "s1", text: "local-text" }));
    state = reducer(state, event("pi", EVENTS.TOKEN, { sessionId: "s1", text: "pi-text" }));
    expect((entry(state, "local", "s1").transcript[0] as { text: string }).text).toBe("local-text");
    expect((entry(state, "pi", "s1").transcript[0] as { text: string }).text).toBe("pi-text");
  });
});

describe("transcript: tool cards", () => {
  it("merges tool_result into the matching tool_call card by callId", () => {
    let state = connectedState("local", [sessionInfo("s1", "/a")]);
    state = reducer(
      state,
      event("local", EVENTS.TOOL_CALL, {
        sessionId: "s1",
        callId: "c1",
        tool: "read",
        args: { path: "x" },
      }),
    );
    let card = entry(state, "local", "s1").transcript[0];
    expect(card).toMatchObject({ kind: "tool", callId: "c1", done: false });

    state = reducer(
      state,
      event("local", EVENTS.TOOL_RESULT, {
        sessionId: "s1",
        callId: "c1",
        tool: "read",
        output: "file body",
        isError: false,
      }),
    );
    card = entry(state, "local", "s1").transcript[0];
    expect(card).toMatchObject({
      kind: "tool",
      callId: "c1",
      done: true,
      output: "file body",
      isError: false,
    });
    expect(entry(state, "local", "s1").transcript).toHaveLength(1);
  });

  it("keeps concurrent tool calls separate by callId", () => {
    let state = connectedState("local", [sessionInfo("s1", "/a")]);
    for (const callId of ["c1", "c2"]) {
      state = reducer(
        state,
        event("local", EVENTS.TOOL_CALL, { sessionId: "s1", callId, tool: "bash", args: {} }),
      );
    }
    state = reducer(
      state,
      event("local", EVENTS.TOOL_RESULT, {
        sessionId: "s1",
        callId: "c2",
        tool: "bash",
        output: "two",
        isError: false,
      }),
    );
    const transcript = entry(state, "local", "s1").transcript;
    expect(transcript[0]).toMatchObject({ callId: "c1", done: false });
    expect(transcript[1]).toMatchObject({ callId: "c2", done: true, output: "two" });
  });

  it("marks errored tool results", () => {
    let state = connectedState("local", [sessionInfo("s1", "/a")]);
    state = reducer(
      state,
      event("local", EVENTS.TOOL_CALL, { sessionId: "s1", callId: "c1", tool: "bash", args: {} }),
    );
    state = reducer(
      state,
      event("local", EVENTS.TOOL_RESULT, {
        sessionId: "s1",
        callId: "c1",
        tool: "bash",
        output: "denied",
        isError: true,
      }),
    );
    expect(entry(state, "local", "s1").transcript[0]).toMatchObject({
      done: true,
      isError: true,
      output: "denied",
    });
  });

  it("surfaces a tool_result with no matching call as a completed orphan card", () => {
    let state = connectedState("local", [sessionInfo("s1", "/a")]);
    state = reducer(
      state,
      event("local", EVENTS.TOOL_RESULT, {
        sessionId: "s1",
        callId: "ghost",
        tool: "read",
        output: "x",
        isError: false,
      }),
    );
    expect(entry(state, "local", "s1").transcript[0]).toMatchObject({
      kind: "tool",
      callId: "ghost",
      done: true,
    });
  });
});

describe("session_state events", () => {
  it("streaming then idle drives the live indicator and clears errors", () => {
    let state = connectedState("local", [sessionInfo("s1", "/a")]);
    state = reducer(
      state,
      event("local", EVENTS.SESSION_STATE, { sessionId: "s1", state: "streaming" }),
    );
    expect(entry(state, "local", "s1").live).toBe("streaming");
    state = reducer(
      state,
      event("local", EVENTS.SESSION_STATE, { sessionId: "s1", state: "error", error: "boom" }),
    );
    expect(entry(state, "local", "s1").live).toBe("error");
    expect(entry(state, "local", "s1").error).toBe("boom");
    state = reducer(
      state,
      event("local", EVENTS.SESSION_STATE, { sessionId: "s1", state: "idle" }),
    );
    expect(entry(state, "local", "s1").live).toBe("idle");
    expect(entry(state, "local", "s1").error).toBeUndefined();
  });

  it("maps the abort error after cancel to a cancelled state, not an error", () => {
    let state = connectedState("local", [sessionInfo("s1", "/a")]);
    state = reducer(
      state,
      event("local", EVENTS.SESSION_STATE, {
        sessionId: "s1",
        state: "error",
        error: "Request was aborted",
      }),
    );
    expect(entry(state, "local", "s1").live).toBe("cancelled");
    expect(entry(state, "local", "s1").error).toBeUndefined();
  });

  it("creates a stub entry for events from an untracked session", () => {
    let state = connectedState("local");
    state = reducer(state, event("local", EVENTS.TOKEN, { sessionId: "ghost", text: "hi" }));
    expect(entry(state, "local", "ghost").transcript).toHaveLength(1);
    expect(sessionDisplayName(entry(state, "local", "ghost"))).toContain("local:");
  });

  it("ignores malformed events and unknown methods", () => {
    const state = connectedState("local", [sessionInfo("s1", "/a")]);
    const cases: Action[] = [
      event("local", EVENTS.TOKEN, { text: "no session id" }),
      event("local", EVENTS.TOKEN, { sessionId: "s1" }),
      event("local", EVENTS.SESSION_STATE, { sessionId: "s1", state: "bogus" }),
      event("local", "event.bogus", { sessionId: "s1" }),
      event("local", EVENTS.TOOL_CALL, { sessionId: "s1", callId: "c1" }),
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
    let state = connectedState("local", [sessionInfo("s1", "/a")]);
    state = reducer(state, event("local", "event.approval_request", request));
    expect(selectCurrentApproval(state)).toMatchObject({
      approvalId: "a1",
      tool: "bash",
      hostName: "local",
    });
    expect(entry(state, "local", "s1").transcript[0]).toMatchObject({
      kind: "approval",
      approvalId: "a1",
    });
  });

  it("ignores a duplicate approval id", () => {
    let state = connectedState("local", [sessionInfo("s1", "/a")]);
    state = reducer(state, event("local", "event.approval_request", request));
    const again = reducer(state, event("local", "event.approval_request", request));
    expect(again).toBe(state);
  });

  it("responded approvals leave the queue head-first and record the decision", () => {
    let state = connectedState("local", [sessionInfo("s1", "/a")]);
    state = reducer(state, event("local", "event.approval_request", request));
    state = reducer(
      state,
      event("local", "event.approval_request", { ...request, approvalId: "a2", callId: "c2" }),
    );
    expect(state.pendingApprovals.map((a) => a.approvalId)).toEqual(["a1", "a2"]);

    state = reducer(state, { type: "approval.responded", approvalId: "a1", decision: "allow" });
    expect(state.pendingApprovals.map((a) => a.approvalId)).toEqual(["a2"]);
    expect(selectCurrentApproval(state)?.approvalId).toBe("a2");
    expect(entry(state, "local", "s1").transcript[0]).toMatchObject({
      kind: "approval",
      approvalId: "a1",
      decision: "allow",
    });
    expect(entry(state, "local", "s1").transcript[1]).toMatchObject({
      kind: "approval",
      approvalId: "a2",
    });
    expect(
      entry(state, "local", "s1").transcript[1] &&
        (entry(state, "local", "s1").transcript[1] as { decision?: string }).decision,
    ).toBeUndefined();
  });

  it("responding to an unknown approval id is a harmless no-op on transcripts", () => {
    let state = connectedState("local", [sessionInfo("s1", "/a")]);
    state = reducer(state, { type: "approval.responded", approvalId: "ghost", decision: "deny" });
    expect(state.pendingApprovals).toEqual([]);
    expect(entry(state, "local", "s1").transcript).toEqual([]);
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

  it("sessionDisplayName includes the host prefix and falls back to the id", () => {
    const state = connectedState("pi", [sessionInfo("s1", "/home/u/proj")], {
      name: "pi",
      url: "ws://pi",
      token: "t",
    });
    expect(sessionDisplayName(entry(state, "pi", "s1"))).toBe("pi: proj");
    expect(sessionDisplayName(entry(state, "pi", "s1"))).toContain("pi:");
  });

  it("selectSessions returns sessions in insertion order", () => {
    let state = connectedState("local", [sessionInfo("s1", "/a")]);
    state = reducer(state, event("local", EVENTS.TOKEN, { sessionId: "s0", text: "x" }));
    expect(selectSessions(state).map((s) => s.info.id)).toEqual(["s1", "s0"]);
  });

  it("selectActiveHostName reads the host from the active composite key", () => {
    let state = connectedState("local", [sessionInfo("s1", "/a")]);
    state = reducer(state, { type: "session.selected", hostName: "local", sessionId: "s1" });
    expect(selectActiveHostName(state)).toBe("local");
  });

  it("selectConnectedHosts returns the full fleet", () => {
    let state = reducer(initialState(), {
      type: "hosts.set",
      hosts: [{ name: "a", url: "ws://a", token: "t" }],
    });
    state = reducer(state, { type: "connect.succeeded", hostName: "a", info: HOST_INFO });
    expect(selectConnectedHosts(state).map((h) => h.host.name)).toEqual(["a"]);
  });
});
