import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";

import { testing } from "@crow/core";
import {
  encodeFrame,
  EVENTS,
  makeNotification,
  makeRequest,
  METHODS,
  NOTIFICATIONS,
  PROTOCOL_VERSION,
  RPC_ERRORS,
} from "@crow/protocol";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import WebSocket from "ws";

import { CrowDaemon } from "./server.ts";

interface CollectedFrame {
  id?: string | number;
  method?: string;
  params?: Record<string, unknown>;
  result?: unknown;
  error?: { code: number; message: string };
}

interface TestClient {
  ws: WebSocket;
  notifications: CollectedFrame[];
  responses: CollectedFrame[];
  call(method: string, params?: unknown): Promise<unknown>;
  notify(method: string, params?: unknown): void;
  sendRaw(raw: string): void;
  close(): Promise<void>;
}

function connect(port: number, token?: string): Promise<WebSocket> {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(
      `ws://127.0.0.1:${port}`,
      token ? { headers: { authorization: `Bearer ${token}` } } : {},
    );
    const onOpen = () => {
      ws.removeListener("error", onError);
      resolve(ws);
    };
    const onError = (error: Error) => {
      ws.removeListener("open", onOpen);
      reject(error);
    };
    ws.once("open", onOpen);
    ws.once("error", onError);
  });
}

function makeClient(ws: WebSocket): TestClient {
  const notifications: CollectedFrame[] = [];
  const responses: CollectedFrame[] = [];
  const pending = new Map<
    number,
    { resolve: (v: unknown) => void; reject: (e: unknown) => void }
  >();
  let nextId = 1;
  let buffer = "";
  ws.on("message", (data: Buffer) => {
    buffer += data.toString("utf8");
    const lines = buffer.split("\n");
    buffer = lines.pop() ?? "";
    for (const line of lines) {
      if (line.trim().length === 0) continue;
      const frame = JSON.parse(line) as CollectedFrame;
      if (frame.method) {
        notifications.push(frame);
        continue;
      }
      responses.push(frame);
      const id = typeof frame.id === "number" ? frame.id : undefined;
      if (id === undefined) continue;
      const entry = pending.get(id);
      if (entry) {
        pending.delete(id);
        if (frame.error) entry.reject(frame.error);
        else entry.resolve(frame.result);
      }
    }
  });
  return {
    ws,
    notifications,
    responses,
    call(method, params) {
      const id = nextId++;
      return new Promise((resolve, reject) => {
        pending.set(id, { resolve, reject });
        ws.send(encodeFrame(makeRequest(id, method, params)));
      });
    },
    notify(method, params) {
      ws.send(encodeFrame(makeNotification(method, params)));
    },
    sendRaw(raw) {
      ws.send(raw);
    },
    close() {
      return new Promise((resolve) => {
        if (ws.readyState === WebSocket.CLOSED) {
          resolve();
          return;
        }
        ws.once("close", () => resolve());
        ws.close();
      });
    },
  };
}

const waitFor = (cond: () => boolean | Promise<boolean>, description: string) =>
  vi.waitFor(
    async () => {
      if (!(await cond())) throw new Error(`still waiting: ${description}`);
    },
    { timeout: 8000, interval: 25 },
  );

describe("CrowDaemon", () => {
  let tmp: string;
  let workdir: string;
  let daemon: CrowDaemon;
  let port: number;
  let faux: ReturnType<typeof testing.makeFauxModels>["faux"];
  const clients: TestClient[] = [];

  const openClient = async (token = "test-token") => {
    const client = makeClient(await connect(port, token));
    clients.push(client);
    return client;
  };

  beforeEach(async () => {
    tmp = await mkdtemp(path.join(os.tmpdir(), "crowd-test-"));
    workdir = path.join(tmp, "work");
    await mkdir(workdir, { recursive: true });
    const made = testing.makeFauxModels();
    faux = made.faux;
    daemon = new CrowDaemon({
      host: "127.0.0.1",
      port: 0,
      token: "test-token",
      dataDir: tmp,
      models: made.models,
      defaultModelRef: testing.FAUX_MODEL_REF,
    });
    ({ port } = await daemon.start());
  });

  afterEach(async () => {
    await Promise.all(clients.splice(0).map((c) => c.close()));
    await daemon.stop();
    await rm(tmp, { recursive: true, force: true });
  });

  it("rejects connections without a valid bearer token", async () => {
    await expect(connect(port)).rejects.toThrow();
    await expect(connect(port, "wrong-token")).rejects.toThrow();
  });

  it("runs a full session flow over the wire", { timeout: 15000 }, async () => {
    await writeFile(path.join(workdir, "hello.txt"), "hello from disk");
    faux.setResponses([
      testing.fauxAssistantMessage([testing.fauxToolCall("read", { path: "hello.txt" })], {
        stopReason: "toolUse",
      }),
      testing.fauxAssistantMessage([testing.fauxText("The file says: hello from disk")]),
    ]);
    const client = await openClient();

    const created = (await client.call(METHODS.SESSION_CREATE, { cwd: workdir })) as {
      sessionId: string;
    };
    expect(created.sessionId).toBeTruthy();

    const sent = await client.call(METHODS.SESSION_SEND, {
      sessionId: created.sessionId,
      text: "read the file",
    });
    expect(sent).toEqual({});

    await waitFor(
      () =>
        client.notifications.some(
          (n) => n.method === EVENTS.SESSION_STATE && n.params?.state === "idle",
        ),
      "session_state idle",
    );

    const tokens = client.notifications
      .filter((n) => n.method === EVENTS.TOKEN)
      .map((n) => String(n.params?.text ?? ""))
      .join("");
    expect(tokens).toContain("The file says: hello from disk");

    const toolCalls = client.notifications.filter((n) => n.method === EVENTS.TOOL_CALL);
    expect(toolCalls).toHaveLength(1);
    expect(toolCalls[0]?.params).toMatchObject({
      sessionId: created.sessionId,
      tool: "read",
      args: { path: "hello.txt" },
    });

    const toolResults = client.notifications.filter((n) => n.method === EVENTS.TOOL_RESULT);
    expect(toolResults).toHaveLength(1);
    expect(toolResults[0]?.params).toMatchObject({ tool: "read", isError: false });
    expect(String(toolResults[0]?.params?.output)).toContain("hello from disk");

    const states = client.notifications.filter((n) => n.method === EVENTS.SESSION_STATE);
    expect(states[0]?.params).toMatchObject({ sessionId: created.sessionId, state: "streaming" });
    expect(states.at(-1)?.params).toMatchObject({ sessionId: created.sessionId, state: "idle" });

    const listed = (await client.call(METHODS.SESSION_LIST, {})) as {
      sessions: { id: string; state: string }[];
    };
    expect(listed.sessions).toHaveLength(1);
    expect(listed.sessions[0]).toMatchObject({ id: created.sessionId, state: "idle" });

    const info = (await client.call(METHODS.HOST_INFO, {})) as Record<string, unknown>;
    expect(info.sessions).toBe(1);
    expect(info.protocolVersion).toBe(PROTOCOL_VERSION);
    expect(typeof info.hostname).toBe("string");
  });

  it("maps failures to the right JSON-RPC error codes", async () => {
    const client = await openClient();

    await expect(
      client.call(METHODS.SESSION_SEND, { sessionId: "no-such-session", text: "hi" }),
    ).rejects.toMatchObject({ code: RPC_ERRORS.SESSION_NOT_FOUND });

    await expect(client.call("totally.bogus", {})).rejects.toMatchObject({
      code: RPC_ERRORS.METHOD_NOT_FOUND,
    });

    await expect(client.call(METHODS.SESSION_SEND, { sessionId: 42 })).rejects.toMatchObject({
      code: RPC_ERRORS.INVALID_PARAMS,
    });

    client.sendRaw("this is not json\n");
    await waitFor(
      () => client.responses.some((r) => r.error?.code === RPC_ERRORS.PARSE_ERROR),
      "PARSE_ERROR response",
    );

    // Oversized NDJSON accumulator (no newline) closes the socket with 1009.
    const greedy = await openClient();
    const closed = new Promise<{ code: number }>((resolve) => {
      greedy.ws.once("close", (code: number) => resolve({ code }));
    });
    greedy.sendRaw("x".repeat(2 * 1024 * 1024));
    expect((await closed).code).toBe(1009);
  });

  it("fans session events out to every attached client", { timeout: 15000 }, async () => {
    faux.setResponses([testing.fauxAssistantMessage([testing.fauxText("broadcast")])]);
    const a = await openClient();
    const b = await openClient();

    const created = (await a.call(METHODS.SESSION_CREATE, { cwd: workdir })) as {
      sessionId: string;
    };
    const attached = await b.call(METHODS.SESSION_ATTACH, { sessionId: created.sessionId });
    expect(attached).toEqual({});

    await a.call(METHODS.SESSION_SEND, { sessionId: created.sessionId, text: "hi" });

    await waitFor(
      () =>
        b.notifications.some(
          (n) => n.method === EVENTS.SESSION_STATE && n.params?.state === "idle",
        ),
      "client B session_state idle",
    );
    const bTokens = b.notifications
      .filter((n) => n.method === EVENTS.TOKEN)
      .map((n) => String(n.params?.text ?? ""))
      .join("");
    expect(bTokens).toContain("broadcast");

    // Attach to an unknown session is an error.
    await expect(
      b.call(METHODS.SESSION_ATTACH, { sessionId: "no-such-session" }),
    ).rejects.toMatchObject({ code: RPC_ERRORS.SESSION_NOT_FOUND });
  });

  describe("tool-call approvals", () => {
    const createAskSession = async (client: TestClient, extra?: Record<string, unknown>) => {
      const created = (await client.call(METHODS.SESSION_CREATE, {
        cwd: workdir,
        approvalMode: "ask",
        ...extra,
      })) as { sessionId: string };
      return created.sessionId;
    };

    const nextApprovalRequest = async (client: TestClient) => {
      await waitFor(
        () => client.notifications.some((n) => n.method === EVENTS.APPROVAL_REQUEST),
        "event.approval_request",
      );
      return client.notifications.find((n) => n.method === EVENTS.APPROVAL_REQUEST);
    };

    const waitForIdle = async (client: TestClient) => {
      await waitFor(
        () =>
          client.notifications.some(
            (n) => n.method === EVENTS.SESSION_STATE && n.params?.state === "idle",
          ),
        "session_state idle",
      );
    };

    it("asks attached clients and runs the tool on allow", { timeout: 15000 }, async () => {
      await writeFile(path.join(workdir, "hello.txt"), "hello from disk");
      faux.setResponses([
        testing.fauxAssistantMessage([testing.fauxToolCall("read", { path: "hello.txt" })], {
          stopReason: "toolUse",
        }),
        testing.fauxAssistantMessage([testing.fauxText("approved and read")]),
      ]);
      const client = await openClient();
      const sessionId = await createAskSession(client);

      await client.call(METHODS.SESSION_SEND, { sessionId, text: "read the file" });

      const approval = await nextApprovalRequest(client);
      expect(approval?.params).toMatchObject({
        sessionId,
        tool: "read",
        args: { path: "hello.txt" },
      });
      expect(String(approval?.params?.approvalId)).toMatch(/^appr_/);
      // The request carries the same callId as the event.tool_call that
      // preceded it, so clients can correlate the two.
      const toolCall = client.notifications.find((n) => n.method === EVENTS.TOOL_CALL);
      expect(approval?.params?.callId).toBe(toolCall?.params?.callId);

      client.notify(NOTIFICATIONS.APPROVAL_RESPOND, {
        approvalId: approval?.params?.approvalId,
        decision: "allow",
      });

      await waitForIdle(client);
      const toolResults = client.notifications.filter((n) => n.method === EVENTS.TOOL_RESULT);
      expect(toolResults).toHaveLength(1);
      expect(toolResults[0]?.params).toMatchObject({ tool: "read", isError: false });
      expect(String(toolResults[0]?.params?.output)).toContain("hello from disk");
      const tokens = client.notifications
        .filter((n) => n.method === EVENTS.TOKEN)
        .map((n) => String(n.params?.text ?? ""))
        .join("");
      expect(tokens).toContain("approved and read");
      // The tool ran exactly once, so exactly one approval was requested.
      expect(client.notifications.filter((n) => n.method === EVENTS.APPROVAL_REQUEST)).toHaveLength(
        1,
      );
    });

    it(
      "blocks the tool on deny and the agent receives the reason",
      { timeout: 15000 },
      async () => {
        let followUpMessages: unknown;
        faux.setResponses([
          testing.fauxAssistantMessage([testing.fauxToolCall("read", { path: "hello.txt" })], {
            stopReason: "toolUse",
          }),
          (context) => {
            followUpMessages = context.messages;
            return testing.fauxAssistantMessage([testing.fauxText("ok, not reading it")]);
          },
        ]);
        const client = await openClient();
        const sessionId = await createAskSession(client);

        await client.call(METHODS.SESSION_SEND, { sessionId, text: "read the file" });
        const approval = await nextApprovalRequest(client);
        client.notify(NOTIFICATIONS.APPROVAL_RESPOND, {
          approvalId: approval?.params?.approvalId,
          decision: "deny",
        });

        await waitForIdle(client);
        const toolResults = client.notifications.filter((n) => n.method === EVENTS.TOOL_RESULT);
        expect(toolResults).toHaveLength(1);
        expect(toolResults[0]?.params).toMatchObject({ tool: "read", isError: true });
        expect(String(toolResults[0]?.params?.output)).toContain("denied by user");
        // The loop continued: the follow-up LLM call saw the error tool result.
        expect(faux.state.callCount).toBe(2);
        expect(JSON.stringify(followUpMessages)).toContain("denied by user");
      },
    );

    it("denies the call when the approval times out", { timeout: 15000 }, async () => {
      const timeoutDir = await mkdtemp(path.join(os.tmpdir(), "crowd-approval-timeout-"));
      const timeoutWorkdir = path.join(timeoutDir, "work");
      await mkdir(timeoutWorkdir, { recursive: true });
      const made = testing.makeFauxModels();
      const shortFuse = new CrowDaemon({
        host: "127.0.0.1",
        port: 0,
        token: "test-token",
        dataDir: timeoutDir,
        models: made.models,
        defaultModelRef: testing.FAUX_MODEL_REF,
        approvalTimeoutMs: 50,
      });
      const { port: timeoutPort } = await shortFuse.start();
      const client = makeClient(await connect(timeoutPort, "test-token"));
      clients.push(client);
      try {
        made.faux.setResponses([
          testing.fauxAssistantMessage([testing.fauxToolCall("read", { path: "x" })], {
            stopReason: "toolUse",
          }),
          testing.fauxAssistantMessage([testing.fauxText("gave up waiting")]),
        ]);
        const created = (await client.call(METHODS.SESSION_CREATE, {
          cwd: timeoutWorkdir,
          approvalMode: "ask",
        })) as { sessionId: string };
        await client.call(METHODS.SESSION_SEND, {
          sessionId: created.sessionId,
          text: "read something",
        });

        await nextApprovalRequest(client);
        // No approval.respond: the 50 ms fuse fires and denies the call.
        await waitFor(
          () =>
            client.notifications.some(
              (n) => n.method === EVENTS.TOOL_RESULT && n.params?.isError === true,
            ),
          "error tool_result",
        );
        await waitForIdle(client);
        const toolResult = client.notifications.find((n) => n.method === EVENTS.TOOL_RESULT);
        expect(String(toolResult?.params?.output)).toContain("timed out");
      } finally {
        await shortFuse.stop();
        await rm(timeoutDir, { recursive: true, force: true });
      }
    });

    it("reports approvalMode in session.list", async () => {
      const client = await openClient();
      const askId = await createAskSession(client);
      const auto = (await client.call(METHODS.SESSION_CREATE, { cwd: workdir })) as {
        sessionId: string;
      };

      const listed = (await client.call(METHODS.SESSION_LIST, {})) as {
        sessions: { id: string; approvalMode: string }[];
      };
      const byId = new Map(listed.sessions.map((s) => [s.id, s.approvalMode]));
      expect(byId.get(askId)).toBe("ask");
      expect(byId.get(auto.sessionId)).toBe("auto");
    });

    it(
      "ignores responds from unattached connections and unknown approvalIds",
      { timeout: 15000 },
      async () => {
        await writeFile(path.join(workdir, "hello.txt"), "hello from disk");
        faux.setResponses([
          testing.fauxAssistantMessage([testing.fauxToolCall("read", { path: "hello.txt" })], {
            stopReason: "toolUse",
          }),
          testing.fauxAssistantMessage([testing.fauxText("read anyway")]),
        ]);
        const a = await openClient();
        const b = await openClient(); // never attaches
        const sessionId = await createAskSession(a);

        await a.call(METHODS.SESSION_SEND, { sessionId, text: "read the file" });
        const approval = await nextApprovalRequest(a);

        // The request only went to attached connections.
        expect(b.notifications.some((n) => n.method === EVENTS.APPROVAL_REQUEST)).toBe(false);

        // A deny from an unattached connection must not resolve the approval.
        b.notify(NOTIFICATIONS.APPROVAL_RESPOND, {
          approvalId: approval?.params?.approvalId,
          decision: "deny",
        });
        // Unknown approvalIds are dropped silently.
        a.notify(NOTIFICATIONS.APPROVAL_RESPOND, {
          approvalId: "appr_bogus",
          decision: "deny",
        });

        // The real allow from the attached creator still lands.
        a.notify(NOTIFICATIONS.APPROVAL_RESPOND, {
          approvalId: approval?.params?.approvalId,
          decision: "allow",
        });

        await waitForIdle(a);
        const toolResults = a.notifications.filter((n) => n.method === EVENTS.TOOL_RESULT);
        expect(toolResults).toHaveLength(1);
        expect(toolResults[0]?.params).toMatchObject({ tool: "read", isError: false });
      },
    );

    it("never asks for tools in autoApproveTools", { timeout: 15000 }, async () => {
      await writeFile(path.join(workdir, "hello.txt"), "hello from disk");
      faux.setResponses([
        testing.fauxAssistantMessage([testing.fauxToolCall("read", { path: "hello.txt" })], {
          stopReason: "toolUse",
        }),
        testing.fauxAssistantMessage([testing.fauxText("read without asking")]),
      ]);
      const client = await openClient();
      const sessionId = await createAskSession(client, { autoApproveTools: ["read"] });

      await client.call(METHODS.SESSION_SEND, { sessionId, text: "read the file" });
      await waitForIdle(client);

      expect(client.notifications.some((n) => n.method === EVENTS.APPROVAL_REQUEST)).toBe(false);
      const toolResult = client.notifications.find((n) => n.method === EVENTS.TOOL_RESULT);
      expect(toolResult?.params).toMatchObject({ tool: "read", isError: false });
    });

    it("denies immediately when no client is attached", { timeout: 15000 }, async () => {
      let followUpMessages: unknown;
      faux.setResponses([
        testing.fauxAssistantMessage([testing.fauxToolCall("read", { path: "hello.txt" })], {
          stopReason: "toolUse",
        }),
        (context) => {
          followUpMessages = context.messages;
          return testing.fauxAssistantMessage([testing.fauxText("no approver around")]);
        },
      ]);
      const creator = await openClient();
      const sessionId = await createAskSession(creator);
      await creator.close();

      const outsider = await openClient(); // connected, but not attached
      await outsider.call(METHODS.SESSION_SEND, { sessionId, text: "read the file" });

      // The tool call is denied without any approval_request on the wire; the
      // follow-up LLM call carries the reason in its context.
      await waitFor(() => faux.state.callCount >= 2, "follow-up llm call");
      expect(JSON.stringify(followUpMessages)).toContain("no client attached to approve");
      expect(outsider.notifications.some((n) => n.method === EVENTS.APPROVAL_REQUEST)).toBe(false);

      await waitFor(async () => {
        const listed = (await outsider.call(METHODS.SESSION_LIST, {})) as {
          sessions: { id: string; state: string }[];
        };
        return listed.sessions.find((s) => s.id === sessionId)?.state === "idle";
      }, "session back to idle");
    });
  });

  describe("sub-agents and teams (P4)", () => {
    const teamEvents = (client: TestClient) =>
      client.notifications.filter((n) => n.method === EVENTS.TEAM).map((n) => n.params ?? {});

    it("spawns a sub-agent and broadcasts started/done to every client", async () => {
      faux.setResponses([testing.fauxAssistantMessage([testing.fauxText("sub-agent output")])]);
      const spawner = await openClient();
      const observer = await openClient(); // connected, not the spawner

      const spawned = (await spawner.call(METHODS.AGENT_SPAWN, {
        prompt: "do the thing",
        cwd: workdir,
      })) as { agentId: string };
      expect(spawned.agentId).toMatch(/^agent_/);

      await waitFor(
        () =>
          spawner.notifications.some(
            (n) => n.method === EVENTS.AGENT && n.params?.state === "done",
          ),
        "event.agent done",
      );

      for (const client of [spawner, observer]) {
        const events = client.notifications
          .filter((n) => n.method === EVENTS.AGENT)
          .map((n) => n.params ?? {});
        expect(events[0]).toMatchObject({ agentId: spawned.agentId, state: "started" });
        expect(events.at(-1)).toMatchObject({
          agentId: spawned.agentId,
          state: "done",
          output: "sub-agent output",
        });
      }
    });

    it("surfaces a run failure as an event.agent error, not an RPC error", async () => {
      faux.setResponses([
        testing.fauxAssistantMessage("boom", { stopReason: "error", errorMessage: "kaboom" }),
      ]);
      const client = await openClient();

      const spawned = (await client.call(METHODS.AGENT_SPAWN, {
        prompt: "go",
        cwd: workdir,
      })) as { agentId: string };

      await waitFor(
        () =>
          client.notifications.some(
            (n) => n.method === EVENTS.AGENT && n.params?.state === "error",
          ),
        "event.agent error",
      );
      const errorEvent = client.notifications.find(
        (n) => n.method === EVENTS.AGENT && n.params?.state === "error",
      );
      expect(errorEvent?.params).toMatchObject({ agentId: spawned.agentId, error: "kaboom" });
    });

    it("lists the built-in team presets", async () => {
      const client = await openClient();
      const listed = (await client.call(METHODS.TEAM_LIST, {})) as {
        teams: { name: string; description: string; agents: { name: string; role: string }[] }[];
      };
      const byName = new Map(listed.teams.map((team) => [team.name, team]));
      expect(byName.has("plan-implement-review")).toBe(true);
      expect(byName.has("solo-review")).toBe(true);
      const pir = byName.get("plan-implement-review");
      expect(pir?.description).toBeTruthy();
      expect(pir?.agents.map((a) => a.name)).toEqual(["planner", "implementer", "reviewer"]);
      for (const team of listed.teams) {
        for (const agent of team.agents) {
          expect(agent.role).toBeTruthy();
        }
      }
    });

    it("runs a team and broadcasts the step sequence plus final done", async () => {
      faux.setResponses([
        testing.fauxAssistantMessage([testing.fauxText("PLAN: step one")]),
        testing.fauxAssistantMessage([testing.fauxText("IMPLEMENTED")]),
        testing.fauxAssistantMessage([testing.fauxText("VERDICT: acceptable")]),
      ]);
      const spawner = await openClient();
      const observer = await openClient();

      const started = (await spawner.call(METHODS.TEAM_RUN, {
        team: "plan-implement-review",
        input: "add a feature",
        cwd: workdir,
      })) as { runId: string };
      expect(started.runId).toMatch(/^run_/);

      await waitFor(
        () =>
          spawner.notifications.some((n) => n.method === EVENTS.TEAM && n.params?.state === "done"),
        "event.team done",
      );

      const expected = [
        { state: "step_started", step: 1, agent: "planner" },
        { state: "step_done", step: 1, agent: "planner", output: "PLAN: step one" },
        { state: "step_started", step: 2, agent: "implementer" },
        { state: "step_done", step: 2, agent: "implementer", output: "IMPLEMENTED" },
        { state: "step_started", step: 3, agent: "reviewer" },
        { state: "step_done", step: 3, agent: "reviewer", output: "VERDICT: acceptable" },
        { state: "done", output: "VERDICT: acceptable" },
      ];
      for (const client of [spawner, observer]) {
        const events = teamEvents(client);
        expect(events).toHaveLength(expected.length);
        for (const [index, want] of expected.entries()) {
          expect(events[index]).toMatchObject({ runId: started.runId, ...want });
        }
      }
    });

    it("rejects an unknown team with INVALID_PARAMS", async () => {
      const client = await openClient();
      await expect(
        client.call(METHODS.TEAM_RUN, { team: "no-such-team", input: "x", cwd: workdir }),
      ).rejects.toMatchObject({ code: RPC_ERRORS.INVALID_PARAMS });
      // No run was started, so no team events flow.
      expect(teamEvents(client)).toHaveLength(0);
    });
  });

  describe("A2A HTTP surface", () => {
    let a2aDaemon: CrowDaemon;
    let a2aTmp: string;
    let a2aWsPort: number;
    let a2aBaseUrl: string;

    beforeEach(async () => {
      a2aTmp = await mkdtemp(path.join(os.tmpdir(), "crow-a2a-"));
      const { models: m, faux: f } = testing.makeFauxModels();
      faux = f;
      a2aDaemon = new CrowDaemon({
        host: "127.0.0.1",
        port: 0,
        token: "a2a-token",
        dataDir: a2aTmp,
        models: m,
        defaultModelRef: "faux/faux-1",
        a2a: { port: 0 },
      });
      const info = await a2aDaemon.start();
      a2aWsPort = info.port;
      const ws = await connect(a2aWsPort, "a2a-token");
      const probe = makeClient(ws);
      try {
        const hostInfo = (await probe.call(METHODS.HOST_INFO, {})) as { a2a?: string };
        a2aBaseUrl = hostInfo.a2a ?? "";
      } finally {
        await probe.close();
      }
      expect(a2aBaseUrl).toMatch(/^http:\/\/127\.0\.0\.1:\d+$/);
    });

    afterEach(async () => {
      await a2aDaemon.stop();
      await rm(a2aTmp, { recursive: true, force: true });
    });

    it("serves /.well-known/agent.json on the A2A port", async () => {
      const res = await fetch(`${a2aBaseUrl}/.well-known/agent.json`, {
        headers: { authorization: "Bearer a2a-token" },
      });
      expect(res.status).toBe(200);
      const body = (await res.json()) as { name: string; endpoint: string };
      expect(body.name).toBe("crow");
      expect(body.endpoint).toBe(`${a2aBaseUrl}/a2a/tasks`);
    });

    it("rejects A2A requests without a valid token", async () => {
      const res = await fetch(`${a2aBaseUrl}/.well-known/agent.json`);
      expect(res.status).toBe(401);
    });

    it("runs a delegated task end-to-end (POST → poll → done)", async () => {
      faux.setResponses([testing.fauxAssistantMessage([testing.fauxText("remote output")])]);
      const post = await fetch(`${a2aBaseUrl}/a2a/tasks`, {
        method: "POST",
        headers: { authorization: "Bearer a2a-token", "content-type": "application/json" },
        body: JSON.stringify({ prompt: "hi", cwd: workdir }),
      });
      expect(post.status).toBe(201);
      const { taskId } = (await post.json()) as { taskId: string };
      let final: { state: string; output?: string; error?: string } | undefined;
      await vi.waitFor(
        async () => {
          const r = await fetch(`${a2aBaseUrl}/a2a/tasks/${taskId}`, {
            headers: { authorization: "Bearer a2a-token" },
          });
          final = (await r.json()) as typeof final;
          if (final?.state === "running") throw new Error("still running");
        },
        { timeout: 5000, interval: 25 },
      );
      expect(final?.state).toBe("done");
      expect(final?.output).toBe("remote output");
    });

    it("returns the A2A base URL in host.info over WS", async () => {
      const client = makeClient(await connect(a2aWsPort, "a2a-token"));
      try {
        const info = (await client.call(METHODS.HOST_INFO, {})) as { a2a?: string };
        expect(info.a2a).toBe(a2aBaseUrl);
      } finally {
        await client.close();
      }
    });
  });

  describe("A2A delegation via WS agent.spawn", () => {
    let remote: CrowDaemon;
    let local: CrowDaemon;
    let localPort: number;
    let remoteBaseUrl: string;
    let tmpA: string;
    let tmpB: string;

    beforeEach(async () => {
      tmpA = await mkdtemp(path.join(os.tmpdir(), "crow-a2a-local-"));
      tmpB = await mkdtemp(path.join(os.tmpdir(), "crow-a2a-remote-"));
      const { models: localModels, faux: _local } = testing.makeFauxModels();
      const { models: remoteModels, faux: remoteFaux } = testing.makeFauxModels();
      faux = remoteFaux;
      local = new CrowDaemon({
        host: "127.0.0.1",
        port: 0,
        token: "shared-token",
        dataDir: tmpA,
        models: localModels,
        defaultModelRef: "faux/faux-1",
      });
      localPort = (await local.start()).port;
      remote = new CrowDaemon({
        host: "127.0.0.1",
        port: 0,
        token: "shared-token",
        dataDir: tmpB,
        models: remoteModels,
        defaultModelRef: "faux/faux-1",
        a2a: { port: 0 },
      });
      const info = await remote.start();
      const remoteWsPort = info.port;
      const ws = await connect(remoteWsPort, "shared-token");
      const probe = makeClient(ws);
      try {
        const hostInfo = (await probe.call(METHODS.HOST_INFO, {})) as { a2a?: string };
        remoteBaseUrl = hostInfo.a2a ?? "";
      } finally {
        await probe.close();
      }
      expect(remoteBaseUrl).toMatch(/^http:\/\/127\.0\.0\.1:\d+$/);
    });

    afterEach(async () => {
      await local.stop();
      await remote.stop();
      await rm(tmpA, { recursive: true, force: true });
      await rm(tmpB, { recursive: true, force: true });
    });

    it("delegates agent.spawn to a remote A2A endpoint and streams done events locally", async () => {
      faux.setResponses([testing.fauxAssistantMessage([testing.fauxText("delegated answer")])]);
      const client = makeClient(await connect(localPort, "shared-token"));
      clients.push(client);

      const { agentId } = (await client.call(METHODS.AGENT_SPAWN, {
        prompt: "do it",
        cwd: workdir,
        host: remoteBaseUrl,
      })) as { agentId: string };
      expect(agentId).toMatch(/^agent_/);

      await waitFor(
        () =>
          client.notifications.some(
            (n) =>
              n.method === EVENTS.AGENT &&
              (n.params as { agentId?: string; state?: string }).agentId === agentId &&
              (n.params as { state?: string }).state === "done",
          ),
        "delegation done event",
      );
      const doneParams = client.notifications
        .map((n) => n.params)
        .find(
          (p) =>
            (p as { agentId?: string }).agentId === agentId &&
            (p as { state?: string }).state === "done",
        ) as { output: string } | undefined;
      expect(doneParams?.output).toBe("delegated answer");
    });
  });
});
