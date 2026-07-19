import { mkdir, mkdtemp, rm } from "node:fs/promises";
import net from "node:net";
import os from "node:os";
import path from "node:path";

import { testing } from "@crow/core";
import { CrowDaemon } from "@crow/daemon";
import { EVENTS, PROTOCOL_VERSION, RPC_ERRORS } from "@crow/protocol";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { WebSocketServer, type WebSocket } from "ws";

import {
  APPROVAL_REQUEST_EVENT,
  CrowClient,
  CrowClientError,
  type ApprovalRequestEvent,
  type SessionStateEvent,
  type TokenEvent,
} from "./index.ts";

const waitFor = (cond: () => boolean, description: string) =>
  vi.waitFor(
    () => {
      if (!cond()) throw new Error(`still waiting: ${description}`);
    },
    { timeout: 8000, interval: 25 },
  );

interface CollectedEvent {
  method: string;
  params: unknown;
}

function collectEvents(client: CrowClient): CollectedEvent[] {
  const events: CollectedEvent[] = [];
  client.onEvent((method, params) => events.push({ method, params }));
  return events;
}

describe("CrowClient against a real daemon", () => {
  let tmp: string;
  let workdir: string;
  let daemon: CrowDaemon;
  let port: number;
  let faux: ReturnType<typeof testing.makeFauxModels>["faux"];
  const clients: CrowClient[] = [];

  const openClient = async (options?: { timeoutMs?: number }) => {
    const client = new CrowClient({
      url: `ws://127.0.0.1:${port}`,
      token: "test-token",
      ...options,
    });
    await client.connect();
    clients.push(client);
    return client;
  };

  beforeEach(async () => {
    tmp = await mkdtemp(path.join(os.tmpdir(), "crow-client-test-"));
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

  it("connects and returns host info", async () => {
    const client = await openClient();
    expect(client.connectionState).toBe("connected");

    const info = await client.hostInfo();
    expect(info.protocolVersion).toBe(PROTOCOL_VERSION);
    expect(typeof info.hostname).toBe("string");
    expect(info.sessions).toBe(0);
  });

  it("rejects a bad token with a CrowClientError naming HTTP 401", async () => {
    const client = new CrowClient({ url: `ws://127.0.0.1:${port}`, token: "wrong-token" });
    const error = await client.connect().catch((e: unknown) => e);
    expect(error).toBeInstanceOf(CrowClientError);
    expect((error as Error).message).toContain("401");
  });

  it("rejects cleanly when the daemon is unreachable", async () => {
    // Reserve-then-release a port so nothing listens on it.
    const server = net.createServer();
    await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
    const address = server.address();
    const deadPort = typeof address === "object" && address !== null ? address.port : 0;
    await new Promise<void>((resolve) => server.close(() => resolve()));

    const client = new CrowClient({ url: `ws://127.0.0.1:${deadPort}`, token: "x" });
    const error = await client.connect().catch((e: unknown) => e);
    expect(error).toBeInstanceOf(CrowClientError);
    expect((error as Error).message).toMatch(/refused/);
  });

  it("times out when the upgrade is never answered", async () => {
    const sockets: net.Socket[] = [];
    const server = net.createServer((socket) => {
      sockets.push(socket); // accept the TCP connection, then stay silent
    });
    await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
    const address = server.address();
    const silentPort = typeof address === "object" && address !== null ? address.port : 0;

    const client = new CrowClient({
      url: `ws://127.0.0.1:${silentPort}`,
      token: "x",
      timeoutMs: 300,
    });
    const error = await client.connect().catch((e: unknown) => e);
    expect(error).toBeInstanceOf(CrowClientError);
    expect((error as Error).message).toMatch(/timed out/);

    for (const socket of sockets) socket.destroy();
    await new Promise<void>((resolve) => server.close(() => resolve()));
  });

  it("runs a full session flow: create, stream, list, attach", { timeout: 15000 }, async () => {
    faux.setResponses([testing.fauxAssistantMessage([testing.fauxText("hello from crow")])]);
    const client = await openClient();
    const events = collectEvents(client);

    const { sessionId } = await client.createSession({ cwd: workdir });
    expect(sessionId).toBeTruthy();

    await client.sendPrompt(sessionId, "say hi");

    await waitFor(
      () =>
        events.some(
          (e) =>
            e.method === EVENTS.SESSION_STATE && (e.params as SessionStateEvent).state === "idle",
        ),
      "session_state idle",
    );

    const tokens = events
      .filter((e) => e.method === EVENTS.TOKEN)
      .map((e) => (e.params as TokenEvent).text)
      .join("");
    expect(tokens).toContain("hello from crow");

    const sessions = await client.listSessions();
    expect(sessions.map((s) => s.id)).toContain(sessionId);
    expect(sessions.find((s) => s.id === sessionId)?.state).toBe("idle");

    // Attach is idempotent for the creator (already auto-attached).
    await client.attachSession(sessionId);
  });

  it("maps daemon errors to CrowClientError with the wire code", async () => {
    const client = await openClient();

    const notFound = await client.sendPrompt("no-such-session", "hi").catch((e: unknown) => e);
    expect(notFound).toBeInstanceOf(CrowClientError);
    expect((notFound as CrowClientError).code).toBe(RPC_ERRORS.SESSION_NOT_FOUND);

    const unknownMethod = await client.call("totally.bogus", {}).catch((e: unknown) => e);
    expect((unknownMethod as CrowClientError).code).toBe(RPC_ERRORS.METHOD_NOT_FOUND);
  });

  it("rejects invalid outgoing params locally, before the wire", async () => {
    const client = await openClient();
    const error = await client.createSession({ cwd: "" }).catch((e: unknown) => e);
    expect(error).toBeInstanceOf(CrowClientError);
    expect((error as CrowClientError).code).toBe(RPC_ERRORS.INVALID_PARAMS);
  });

  it(
    "cancels a streaming session and surfaces the abort as an error state",
    { timeout: 15000 },
    async () => {
      // A deliberately slow provider so the run is still streaming when cancel lands.
      const made = testing.makeFauxModels({ tokensPerSecond: 30 });
      const slowDaemon = new CrowDaemon({
        host: "127.0.0.1",
        port: 0,
        token: "test-token",
        dataDir: path.join(tmp, "slow"),
        models: made.models,
        defaultModelRef: testing.FAUX_MODEL_REF,
      });
      const { port: slowPort } = await slowDaemon.start();
      const client = new CrowClient({ url: `ws://127.0.0.1:${slowPort}`, token: "test-token" });
      clients.push(client);
      try {
        made.faux.setResponses([
          testing.fauxAssistantMessage([testing.fauxText("word ".repeat(400))]),
        ]);
        await client.connect();
        const events = collectEvents(client);

        const { sessionId } = await client.createSession({ cwd: workdir });
        await client.sendPrompt(sessionId, "stream a long reply");
        await waitFor(() => events.some((e) => e.method === EVENTS.TOKEN), "first streamed token");

        await client.cancelSession(sessionId);

        await waitFor(
          () =>
            events.some(
              (e) =>
                e.method === EVENTS.SESSION_STATE &&
                (e.params as SessionStateEvent).state === "error",
            ),
          "session_state error after cancel",
        );
        const errorEvent = events.find(
          (e) =>
            e.method === EVENTS.SESSION_STATE && (e.params as SessionStateEvent).state === "error",
        );
        expect((errorEvent?.params as SessionStateEvent).error ?? "").toMatch(/abort/i);
      } finally {
        await slowDaemon.stop();
      }
    },
  );
});

describe("CrowClient protocol handling (fake ws server)", () => {
  interface FakeServer {
    port: number;
    received: string[];
    close(): Promise<void>;
  }

  const servers: FakeServer[] = [];
  const clients: CrowClient[] = [];

  async function startFakeServer(options?: {
    onConnection?: (ws: WebSocket) => void;
    onMessage?: (ws: WebSocket, data: string) => void;
  }): Promise<FakeServer> {
    const received: string[] = [];
    const connections: WebSocket[] = [];
    const wss = new WebSocketServer({ host: "127.0.0.1", port: 0 });
    await new Promise<void>((resolve) => wss.on("listening", resolve));
    wss.on("connection", (ws) => {
      connections.push(ws);
      ws.on("message", (data: Buffer) => {
        const text = data.toString("utf8");
        received.push(text);
        options?.onMessage?.(ws, text);
      });
      options?.onConnection?.(ws);
    });
    const address = wss.address();
    const port = typeof address === "object" && address !== null ? address.port : 0;
    const server: FakeServer = {
      port,
      received,
      close: () =>
        new Promise<void>((resolve) => {
          for (const ws of connections) ws.terminate();
          wss.close(() => resolve());
        }),
    };
    servers.push(server);
    return server;
  }

  async function openClient(server: FakeServer, options?: { timeoutMs?: number }) {
    const client = new CrowClient({
      url: `ws://127.0.0.1:${server.port}`,
      token: "test-token",
      ...options,
    });
    await client.connect();
    clients.push(client);
    return client;
  }

  afterEach(async () => {
    await Promise.all(clients.splice(0).map((c) => c.close()));
    await Promise.all(servers.splice(0).map((s) => s.close()));
  });

  it("rejects all pending calls when the server drops the connection", async () => {
    const server = await startFakeServer({
      onMessage: (ws) => ws.close(1011, "boom"),
    });
    const client = await openClient(server);
    const states: string[] = [];
    client.onStateChange((state) => states.push(state));

    const error = await client.call("host.info").catch((e: unknown) => e);
    expect(error).toBeInstanceOf(CrowClientError);
    expect((error as Error).message).toMatch(/disconnected/);

    await waitFor(() => states.includes("disconnected"), "disconnected state");
    expect(client.connectionState).toBe("disconnected");
  });

  it("rejects pending calls on close() and refuses later calls", async () => {
    const server = await startFakeServer(); // never responds
    const client = await openClient(server);

    const pending = client.call("host.info");
    const assertion = expect(pending).rejects.toThrowError(/closed/);
    await client.close();
    await assertion;

    await expect(client.call("host.info")).rejects.toThrowError(/not connected/);
    expect(client.connectionState).toBe("disconnected");
  });

  it("times out calls the server never answers", async () => {
    const server = await startFakeServer(); // never responds
    const client = await openClient(server, { timeoutMs: 250 });

    await expect(client.call("host.info")).rejects.toThrowError(/timed out/);
    // The pending map is cleaned: a late response would be ignored, the next
    // call gets a fresh id and its own timeout.
    await expect(client.call("host.info")).rejects.toThrowError(/timed out/);
  });

  it("handles NDJSON frames split and coalesced across ws messages", async () => {
    const server = await startFakeServer({
      onMessage: (() => {
        const requestIds: (string | number)[] = [];
        return (ws, data) => {
          for (const line of data.split("\n")) {
            if (line.trim().length === 0) continue;
            requestIds.push((JSON.parse(line) as { id: string | number }).id);
          }
          if (requestIds.length < 2) return;
          const frameA = `${JSON.stringify({ jsonrpc: "2.0", id: requestIds[0], result: { n: "a" } })}\n`;
          const frameB = `${JSON.stringify({ jsonrpc: "2.0", id: requestIds[1], result: { n: "b" } })}\n`;
          // First chunk ends mid-frame; second delivers the rest of A plus all of B.
          ws.send(frameA.slice(0, 7));
          ws.send(frameA.slice(7) + frameB);
        };
      })(),
    });
    const client = await openClient(server);

    const [a, b] = await Promise.all([
      client.call<{ n: string }>("test.a"),
      client.call<{ n: string }>("test.b"),
    ]);
    expect(a.n).toBe("a");
    expect(b.n).toBe("b");
  });

  it("routes only known event methods to onEvent listeners", async () => {
    const server = await startFakeServer({
      onConnection: (ws) => {
        const frames = [
          { jsonrpc: "2.0", method: EVENTS.TOKEN, params: { sessionId: "s", text: "hi" } },
          {
            jsonrpc: "2.0",
            method: APPROVAL_REQUEST_EVENT,
            params: { sessionId: "s", approvalId: "a1", callId: "c1", tool: "bash", args: {} },
          },
          { jsonrpc: "2.0", method: "event.bogus", params: { x: 1 } },
          { jsonrpc: "2.0", method: "progress.update", params: { y: 2 } },
          { jsonrpc: "2.0", id: 99, method: "daemon.ping" }, // a request: must be ignored
        ];
        ws.send(frames.map((f) => JSON.stringify(f)).join("\n") + "\n");
      },
    });
    const client = new CrowClient({ url: `ws://127.0.0.1:${server.port}`, token: "test-token" });
    clients.push(client);
    // Subscribe before connecting: the server pushes its frames at connection
    // time, and events only reach listeners registered by then.
    const events = collectEvents(client);
    await client.connect();
    await waitFor(() => events.length === 2, "exactly the two known events");
    expect(events.map((e) => e.method)).toEqual([EVENTS.TOKEN, APPROVAL_REQUEST_EVENT]);
    const approval = events[1]?.params as ApprovalRequestEvent;
    expect(approval.approvalId).toBe("a1");
    expect(approval.tool).toBe("bash");
  });

  it("sends approval.respond as a well-formed notification (no id)", async () => {
    const server = await startFakeServer();
    const client = await openClient(server);

    client.respondApproval("a1", "always");

    await waitFor(
      () => server.received.some((raw) => raw.includes("approval.respond")),
      "approval.respond on the wire",
    );
    const raw = server.received.find((r) => r.includes("approval.respond")) ?? "";
    const frame = JSON.parse(raw.trim()) as Record<string, unknown>;
    expect(frame).toEqual({
      jsonrpc: "2.0",
      method: "approval.respond",
      params: { approvalId: "a1", decision: "always" },
    });
    expect("id" in frame).toBe(false);
  });

  it("throws synchronously on an invalid approval decision", async () => {
    const server = await startFakeServer();
    const client = await openClient(server);

    expect(() => client.respondApproval("a1", "maybe" as never)).toThrow(CrowClientError);
    expect(server.received).toHaveLength(0);
  });
});
