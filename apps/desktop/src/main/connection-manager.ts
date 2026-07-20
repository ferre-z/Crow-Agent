import { CrowClient, CrowClientError, type CreateSessionParams } from "@crow/client";
import { type HostInfoResult, type SessionInfo } from "@crow/protocol";
import type * as electron from "electron";

import type {
  ApprovalDecision,
  ConnectResult,
  CreateSessionRequest,
  DaemonConnectionState,
  HostConnectionView,
} from "../shared/api.ts";
import type { KnownHost } from "../shared/hosts.ts";

export interface ConnectionManagerOptions {
  /** Called for every daemon notification from every connection. */
  onEvent(hostName: string, method: string, params: unknown): void;
  /** Called whenever a connection's state changes. */
  onStateChange(hostName: string, state: DaemonConnectionState): void;
  /**
   * Optional Electron Notification constructor for OS notifications. If omitted,
   * notifications are skipped.
   */
  Notification?: typeof electron.Notification;
  /** Whether the app window is currently focused — notifications are quiet when focused. */
  isFocused?(): boolean;
}

interface Connection {
  host: KnownHost;
  client: CrowClient;
  state: DaemonConnectionState;
  info?: HostInfoResult;
  error?: string;
}

function classifyConnectError(error: unknown): ConnectResult {
  const message = error instanceof Error ? error.message : String(error);
  if (error instanceof CrowClientError && message.includes("HTTP 401")) {
    return { ok: false, kind: "auth", message };
  }
  if (/refused|host not found|timed out/.test(message)) {
    return { ok: false, kind: "unreachable", message };
  }
  return { ok: false, kind: "error", message };
}

/** Holds zero or more daemon connections for the desktop hub. */
export class ConnectionManager {
  private readonly connections = new Map<string, Connection>();
  private readonly options: ConnectionManagerOptions;

  constructor(options: ConnectionManagerOptions) {
    this.options = options;
  }

  list(): HostConnectionView[] {
    return Array.from(this.connections.values()).map((c) => ({
      host: c.host,
      state: c.state,
      info: c.info,
      ...(c.error !== undefined ? { error: c.error } : {}),
    }));
  }

  get(hostName: string): Connection | undefined {
    return this.connections.get(hostName);
  }

  async add(host: KnownHost): Promise<ConnectResult> {
    await this.disconnect(host.name);
    return this.connect(host);
  }

  async connect(host: KnownHost): Promise<ConnectResult> {
    const existing = this.connections.get(host.name);
    if (existing?.state === "connected") {
      return { ok: true, info: existing.info! };
    }
    // Stale entry from a failed or dropped attempt — close its client and retry fresh.
    if (existing) {
      await existing.client.close().catch(() => undefined);
    }

    const client = new CrowClient({ url: host.url, token: host.token });
    const conn: Connection = { host, client, state: "disconnected" };
    this.connections.set(host.name, conn);

    this.wireClient(host.name, client);

    try {
      await client.connect();
      const info = await client.hostInfo();
      conn.info = info;
      conn.error = undefined;
      conn.state = "connected";
      this.options.onStateChange(host.name, "connected");
      return { ok: true, info };
    } catch (error) {
      // Keep the entry: the fleet shows the host disconnected with its error,
      // and a later connect() retries with a fresh client.
      conn.error = error instanceof Error ? error.message : String(error);
      conn.state = "disconnected";
      this.options.onStateChange(host.name, "disconnected");
      return classifyConnectError(error);
    }
  }

  async disconnect(hostName: string): Promise<void> {
    const conn = this.connections.get(hostName);
    if (!conn) return;
    this.connections.delete(hostName);
    this.options.onStateChange(hostName, "disconnected");
    await conn.client.close().catch(() => undefined);
  }

  async closeAll(): Promise<void> {
    for (const [name] of this.connections) {
      await this.disconnect(name);
    }
  }

  async createSession(
    hostName: string,
    params: CreateSessionRequest,
  ): Promise<{ sessionId: string }> {
    const conn = this.require(hostName);
    const createParams: CreateSessionParams = {
      cwd: params.cwd,
      approvalMode: params.approvalMode,
      autoApproveTools: params.autoApproveTools,
    };
    return conn.client.createSession(createParams);
  }

  async sendPrompt(hostName: string, sessionId: string, text: string): Promise<void> {
    return this.require(hostName).client.sendPrompt(sessionId, text);
  }

  async cancelSession(hostName: string, sessionId: string): Promise<void> {
    return this.require(hostName).client.cancelSession(sessionId);
  }

  async listSessions(hostName: string): Promise<SessionInfo[]> {
    return this.require(hostName).client.listSessions();
  }

  async attachSession(hostName: string, sessionId: string): Promise<void> {
    return this.require(hostName).client.attachSession(sessionId);
  }

  respondApproval(hostName: string, approvalId: string, decision: ApprovalDecision): void {
    this.require(hostName).client.respondApproval(approvalId, decision);
  }

  /** Generic JSON-RPC passthrough for methods without a dedicated CrowClient wrapper. */
  async call<T = unknown>(hostName: string, method: string, params?: unknown): Promise<T> {
    return this.require(hostName).client.call<T>(method, params);
  }

  private require(hostName: string): Connection {
    const conn = this.connections.get(hostName);
    if (!conn || conn.state !== "connected") {
      throw new Error(`not connected to host: ${hostName}`);
    }
    return conn;
  }

  private wireClient(hostName: string, client: CrowClient): void {
    let wasStreaming = false;
    client.onEvent((method, params) => {
      if (method === "event.session_state") {
        const p = params as { state?: string } | undefined;
        if (p?.state === "streaming") wasStreaming = true;
        if ((p?.state === "idle" || p?.state === "error") && wasStreaming) {
          wasStreaming = false;
          this.notify(`Session on ${hostName} finished`, p.state === "error" ? "with error" : "");
        }
      }
      if (method === "event.approval_request") {
        this.notify(`Approval requested on ${hostName}`, "A tool is waiting for approval");
      }
      this.options.onEvent(hostName, method, params);
    });
    client.onStateChange((state) => {
      if (state === "disconnected") {
        const conn = this.connections.get(hostName);
        if (conn) {
          conn.state = "disconnected";
          this.options.onStateChange(hostName, "disconnected");
        }
      }
    });
  }

  private setState(hostName: string, state: DaemonConnectionState): void {
    const conn = this.connections.get(hostName);
    if (conn) conn.state = state;
    this.options.onStateChange(hostName, state);
  }

  private notify(title: string, body: string): void {
    const { Notification, isFocused } = this.options;
    if (!Notification) return;
    if (isFocused?.() ?? true) return;
    new Notification({ title, body });
  }
}
