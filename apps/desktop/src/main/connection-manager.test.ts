import { beforeEach, describe, expect, it, vi } from "vitest";

import type {
  ApprovalDecision,
  ConnectionStateListener,
  CrowClientOptions,
  DaemonEventListener,
  HostInfoResult,
  SessionInfo,
} from "@crow/client";

import { ConnectionManager } from "./connection-manager.ts";

const HOST_INFO: HostInfoResult = {
  hostname: "testbox",
  platform: "linux",
  arch: "x64",
  node: "v22.0.0",
  daemonVersion: "0.1.0",
  protocolVersion: "0.1.0",
  sessions: 0,
};

interface FakeClientConfig {
  connectError?: unknown;
  hostInfo?: HostInfoResult;
  sessions?: SessionInfo[];
  createSessionId?: string;
}

const mocks = vi.hoisted(() => {
  const clientConfigs = new Map<string, FakeClientConfig>();

  class FakeCrowClientError extends Error {
    readonly code: number | undefined;
    override readonly cause: unknown;

    constructor(message: string, options: { code?: number; cause?: unknown } = {}) {
      super(message);
      this.name = "CrowClientError";
      this.code = options.code;
      this.cause = options.cause ?? undefined;
    }
  }

  class FakeCrowClient {
    static clients: FakeCrowClient[] = [];

    readonly options: CrowClientOptions;
    private readonly eventListeners = new Set<DaemonEventListener>();
    private readonly stateListeners = new Set<ConnectionStateListener>();
    private config: FakeClientConfig;
    closed = false;
    sendPromptCalls: { sessionId: string; text: string }[] = [];
    cancelSessionCalls: string[] = [];
    attachSessionCalls: string[] = [];
    respondApprovalCalls: { approvalId: string; decision: ApprovalDecision }[] = [];

    constructor(options: CrowClientOptions) {
      this.options = options;
      this.config = clientConfigs.get(options.url) ?? {};
      FakeCrowClient.clients.push(this);
    }

    setConfig(config: FakeClientConfig): void {
      this.config = config;
    }

    async connect(): Promise<void> {
      if (this.config.connectError) throw this.config.connectError;
    }

    async close(): Promise<void> {
      this.closed = true;
    }

    async hostInfo(): Promise<HostInfoResult> {
      return this.config.hostInfo ?? HOST_INFO;
    }

    async createSession(): Promise<{ sessionId: string }> {
      return { sessionId: this.config.createSessionId ?? "new-session" };
    }

    async sendPrompt(sessionId: string, text: string): Promise<void> {
      this.sendPromptCalls.push({ sessionId, text });
    }

    async cancelSession(sessionId: string): Promise<void> {
      this.cancelSessionCalls.push(sessionId);
    }

    async listSessions(): Promise<SessionInfo[]> {
      return this.config.sessions ?? [];
    }

    async attachSession(sessionId: string): Promise<void> {
      this.attachSessionCalls.push(sessionId);
    }

    respondApproval(approvalId: string, decision: ApprovalDecision): void {
      this.respondApprovalCalls.push({ approvalId, decision });
    }

    onEvent(listener: DaemonEventListener): () => void {
      this.eventListeners.add(listener);
      return () => this.eventListeners.delete(listener);
    }

    onStateChange(listener: ConnectionStateListener): () => void {
      this.stateListeners.add(listener);
      return () => this.stateListeners.delete(listener);
    }

    emitEvent(method: string, params: unknown): void {
      for (const listener of this.eventListeners) {
        listener(method, params);
      }
    }

    setState(state: "connected" | "disconnected"): void {
      for (const listener of this.stateListeners) {
        listener(state);
      }
    }
  }

  return { FakeCrowClient, FakeCrowClientError, clientConfigs };
});

vi.mock("@crow/client", () => ({
  CrowClient: mocks.FakeCrowClient,
  CrowClientError: mocks.FakeCrowClientError,
}));

function createManager() {
  const events: { hostName: string; method: string; params: unknown }[] = [];
  const states: { hostName: string; state: "connected" | "disconnected" }[] = [];

  const manager = new ConnectionManager({
    onEvent: (hostName, method, params) => events.push({ hostName, method, params }),
    onStateChange: (hostName, state) => states.push({ hostName, state }),
  });

  return { manager, events, states };
}

function host(name: string): { name: string; url: string; token: string } {
  return { name, url: `ws://${name}`, token: "secret" };
}

beforeEach(() => {
  mocks.clientConfigs.clear();
  mocks.FakeCrowClient.clients = [];
});

describe("ConnectionManager", () => {
  it("connect succeeds and reports state changes", async () => {
    const { manager, states } = createManager();
    const result = await manager.connect(host("local"));
    expect(result.ok).toBe(true);
    if (result.ok) expect(result.info.hostname).toBe("testbox");
    expect(states).toContainEqual({ hostName: "local", state: "connected" });
    expect(manager.list()).toHaveLength(1);
  });

  it("connect returns auth error for HTTP 401", async () => {
    mocks.clientConfigs.set("ws://local", {
      connectError: new mocks.FakeCrowClientError(
        "daemon at ws://local rejected the connection (HTTP 401): check the auth token",
      ),
    });
    const { manager, states } = createManager();
    const result = await manager.connect(host("local"));
    expect(result.ok).toBe(false);
    if (!result.ok) expect(result.kind).toBe("auth");
    expect(states).toContainEqual({ hostName: "local", state: "disconnected" });
  });

  it("connect returns unreachable error for connection refused", async () => {
    mocks.clientConfigs.set("ws://local", {
      connectError: new mocks.FakeCrowClientError(
        "cannot reach daemon at ws://local: connection refused",
      ),
    });
    const { manager } = createManager();
    const result = await manager.connect(host("local"));
    expect(result.ok).toBe(false);
    if (!result.ok) expect(result.kind).toBe("unreachable");
  });

  it("disconnect removes the connection and closes the client", async () => {
    const { manager, states } = createManager();
    await manager.connect(host("local"));
    const client = mocks.FakeCrowClient.clients[mocks.FakeCrowClient.clients.length - 1]!;
    await manager.disconnect("local");
    expect(client.closed).toBe(true);
    expect(states).toContainEqual({ hostName: "local", state: "disconnected" });
    expect(manager.list()).toHaveLength(0);
  });

  it("list returns current connection views", async () => {
    const { manager } = createManager();
    await manager.connect(host("local"));
    const views = manager.list();
    expect(views).toHaveLength(1);
    expect(views[0]).toMatchObject({ host: { name: "local" }, state: "connected" });
  });

  it("forwards daemon events stamped with the host name", async () => {
    const { manager, events } = createManager();
    await manager.connect(host("local"));
    const client = mocks.FakeCrowClient.clients[mocks.FakeCrowClient.clients.length - 1]!;
    client.emitEvent("event.token", { sessionId: "s1", text: "hi" });
    expect(events).toContainEqual({
      hostName: "local",
      method: "event.token",
      params: { sessionId: "s1", text: "hi" },
    });
  });

  it("keeps events from two hosts isolated", async () => {
    const { manager, events } = createManager();
    await manager.connect(host("local"));
    await manager.connect(host("pi"));
    const local = mocks.FakeCrowClient.clients[mocks.FakeCrowClient.clients.length - 2]!;
    const pi = mocks.FakeCrowClient.clients[mocks.FakeCrowClient.clients.length - 1]!;
    local.emitEvent("event.token", { sessionId: "s1", text: "local" });
    pi.emitEvent("event.token", { sessionId: "s1", text: "pi" });
    expect(events).toContainEqual({
      hostName: "local",
      method: "event.token",
      params: { sessionId: "s1", text: "local" },
    });
    expect(events).toContainEqual({
      hostName: "pi",
      method: "event.token",
      params: { sessionId: "s1", text: "pi" },
    });
  });

  it("createSession delegates to the right client", async () => {
    const { manager } = createManager();
    await manager.connect(host("local"));
    const client = mocks.FakeCrowClient.clients[mocks.FakeCrowClient.clients.length - 1]!;
    client.setConfig({ createSessionId: "abc-123" });
    const result = await manager.createSession("local", { hostName: "local", cwd: "/tmp" });
    expect(result.sessionId).toBe("abc-123");
  });

  it("sendPrompt delegates to the right client", async () => {
    const { manager } = createManager();
    await manager.connect(host("local"));
    const client = mocks.FakeCrowClient.clients[mocks.FakeCrowClient.clients.length - 1]!;
    await manager.sendPrompt("local", "s1", "hello");
    expect(client.sendPromptCalls).toEqual([{ sessionId: "s1", text: "hello" }]);
  });

  it("cancelSession delegates to the right client", async () => {
    const { manager } = createManager();
    await manager.connect(host("local"));
    const client = mocks.FakeCrowClient.clients[mocks.FakeCrowClient.clients.length - 1]!;
    await manager.cancelSession("local", "s1");
    expect(client.cancelSessionCalls).toEqual(["s1"]);
  });

  it("listSessions delegates to the right client", async () => {
    const { manager } = createManager();
    await manager.connect(host("local"));
    const client = mocks.FakeCrowClient.clients[mocks.FakeCrowClient.clients.length - 1]!;
    client.setConfig({
      sessions: [
        { id: "s1", cwd: "/a", state: "idle", createdAt: "", approvalMode: "auto", model: null },
      ],
    });
    const sessions = await manager.listSessions("local");
    expect(sessions).toHaveLength(1);
    expect(sessions[0]?.id).toBe("s1");
  });

  it("attachSession delegates to the right client", async () => {
    const { manager } = createManager();
    await manager.connect(host("local"));
    const client = mocks.FakeCrowClient.clients[mocks.FakeCrowClient.clients.length - 1]!;
    await manager.attachSession("local", "s1");
    expect(client.attachSessionCalls).toEqual(["s1"]);
  });

  it("respondApproval delegates to the right client", async () => {
    const { manager } = createManager();
    await manager.connect(host("local"));
    const client = mocks.FakeCrowClient.clients[mocks.FakeCrowClient.clients.length - 1]!;
    manager.respondApproval("local", "a1", "allow");
    expect(client.respondApprovalCalls).toEqual([{ approvalId: "a1", decision: "allow" }]);
  });

  it("throws when operating on an unknown or disconnected host", async () => {
    const { manager } = createManager();
    await expect(manager.sendPrompt("ghost", "s1", "hello")).rejects.toThrow(
      "not connected to host: ghost",
    );
  });

  it("closeAll disconnects every connection", async () => {
    const { manager } = createManager();
    await manager.connect(host("local"));
    await manager.connect(host("pi"));
    await manager.closeAll();
    expect(manager.list()).toHaveLength(0);
  });
});
