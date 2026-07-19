import os from "node:os";
import path from "node:path";

import {
  CrowSessionManager,
  createCrowModels,
  DEFAULT_MODEL_REF,
  type CrowSession,
  type CrowSessionEvent,
  type CrowSessionInfo,
  type Models,
} from "@crow/core";
import {
  encodeFrame,
  EVENTS,
  jsonRpcFrameSchema,
  makeError,
  makeNotification,
  makeResult,
  METHODS,
  methodParamsSchemas,
  PROTOCOL_VERSION,
  RPC_ERRORS,
  type HostInfoResult,
  type JsonRpcFrame,
  type JsonRpcRequest,
  type RequestId,
  type SessionAttachParams,
  type SessionCancelParams,
  type SessionCreateParams,
  type SessionInfo,
  type SessionSendParams,
} from "@crow/protocol";
import { WebSocket, WebSocketServer } from "ws";

export const DAEMON_VERSION = "0.1.0" as const;

/** Close policy for a runaway NDJSON accumulator (no newline seen). */
const MAX_BUFFER_BYTES = 1024 * 1024;

/** Minimal structural view of the zod param validators (zod is not a direct dep). */
interface ParamsValidator {
  safeParse(
    input: unknown,
  ):
    { success: true; data: unknown } | { success: false; error: { issues: { message: string }[] } };
}

const paramsValidators: Record<string, ParamsValidator> = methodParamsSchemas;

export interface CrowDaemonOptions {
  host: string;
  port: number;
  token: string;
  dataDir: string;
  /** Injectable for tests (e.g. the faux provider); defaults to all built-in providers. */
  models?: Models;
  defaultModelRef?: string;
}

interface ConnectionState {
  ws: WebSocket;
  attachedSessionIds: Set<string>;
  buffer: string;
}

/** Map the richer crow session state onto the wire's coarse idle/busy. */
function toWireSessionInfo(info: CrowSessionInfo): SessionInfo {
  return {
    id: info.id,
    cwd: info.cwd,
    model: info.modelRef,
    state: info.state === "streaming" ? "busy" : "idle",
    createdAt: info.createdAt,
  };
}

export class CrowDaemon {
  private readonly options: CrowDaemonOptions;
  private readonly manager: CrowSessionManager;
  private readonly connections = new Set<ConnectionState>();
  private readonly sessionSubscriptions = new Map<string, () => void>();
  private wss: WebSocketServer | undefined;

  constructor(options: CrowDaemonOptions) {
    this.options = options;
    this.manager = new CrowSessionManager({
      sessionsRoot: path.join(options.dataDir, "sessions"),
      models: options.models ?? createCrowModels(),
      defaultModelRef: options.defaultModelRef ?? DEFAULT_MODEL_REF,
    });
  }

  start(): Promise<{ port: number }> {
    return new Promise((resolve, reject) => {
      const wss = new WebSocketServer({
        host: this.options.host,
        port: this.options.port,
        verifyClient: (info, done) => {
          const expected = `Bearer ${this.options.token}`;
          // Rejected upgrades get an HTTP 401 before any WS frames flow.
          done(info.req.headers.authorization === expected, 401, "unauthorized");
        },
      });
      wss.on("error", reject);
      wss.on("listening", () => {
        this.wss = wss;
        const address = wss.address();
        resolve({
          port: typeof address === "object" && address !== null ? address.port : this.options.port,
        });
      });
      wss.on("connection", (ws) => this.onConnection(ws));
    });
  }

  async stop(): Promise<void> {
    for (const conn of this.connections) {
      conn.ws.close();
    }
    this.connections.clear();
    for (const unsubscribe of this.sessionSubscriptions.values()) {
      unsubscribe();
    }
    this.sessionSubscriptions.clear();
    await this.manager.shutdown();
    await new Promise<void>((resolve) => {
      if (!this.wss) {
        resolve();
        return;
      }
      this.wss.close(() => resolve());
    });
  }

  private onConnection(ws: WebSocket): void {
    const conn: ConnectionState = { ws, attachedSessionIds: new Set(), buffer: "" };
    this.connections.add(conn);
    ws.on("message", (data: Buffer) => this.onMessage(conn, data));
    ws.on("close", () => {
      this.connections.delete(conn);
    });
    ws.on("error", () => {
      // The "close" handler above does the cleanup; swallow transport errors.
    });
  }

  private onMessage(conn: ConnectionState, data: Buffer): void {
    conn.buffer += data.toString("utf8");
    if (conn.buffer.length > MAX_BUFFER_BYTES) {
      conn.buffer = "";
      conn.ws.close(1009, "message too big");
      return;
    }
    const lines = conn.buffer.split("\n");
    conn.buffer = lines.pop() ?? "";
    for (const line of lines) {
      if (line.trim().length === 0) continue;
      this.onLine(conn, line);
    }
  }

  private onLine(conn: ConnectionState, line: string): void {
    let raw: unknown;
    try {
      raw = JSON.parse(line);
    } catch {
      this.sendFrame(conn, makeError("unknown", RPC_ERRORS.PARSE_ERROR, "invalid JSON"));
      return;
    }
    const parsed = jsonRpcFrameSchema.safeParse(raw);
    if (!parsed.success) {
      this.sendFrame(
        conn,
        makeError(extractId(raw), RPC_ERRORS.INVALID_REQUEST, "invalid JSON-RPC frame"),
      );
      return;
    }
    const frame = parsed.data;
    if (!("method" in frame)) return; // responses from clients: ignore
    if (!("id" in frame)) return; // notifications: ignored in P1 (reserved for approval.respond)
    void this.dispatch(conn, frame);
  }

  private async dispatch(conn: ConnectionState, request: JsonRpcRequest): Promise<void> {
    const { id, method } = request;
    try {
      const validator = paramsValidators[method];
      if (!validator) {
        this.sendFrame(
          conn,
          makeError(id, RPC_ERRORS.METHOD_NOT_FOUND, `unknown method: ${method}`),
        );
        return;
      }
      const params = validator.safeParse(request.params ?? {});
      if (!params.success) {
        const detail = params.error.issues.map((i) => i.message).join("; ");
        this.sendFrame(conn, makeError(id, RPC_ERRORS.INVALID_PARAMS, `invalid params: ${detail}`));
        return;
      }
      const result = await this.handle(conn, method, params.data);
      this.sendFrame(conn, makeResult(id, result ?? {}));
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      const code = message.includes("not found")
        ? RPC_ERRORS.SESSION_NOT_FOUND
        : message.includes("busy")
          ? RPC_ERRORS.SESSION_BUSY
          : RPC_ERRORS.INTERNAL_ERROR;
      this.sendFrame(conn, makeError(id, code, message));
    }
  }

  private async handle(conn: ConnectionState, method: string, params: unknown): Promise<unknown> {
    switch (method) {
      case METHODS.SESSION_CREATE: {
        const p = params as SessionCreateParams;
        const session = await this.manager.create({
          cwd: p.cwd,
          modelRef: p.model,
          systemPrompt: p.systemPrompt,
          skillDirs: p.skillDirs,
        });
        this.ensureSessionSubscription(session);
        // The creator is implicitly attached to its own session's events.
        conn.attachedSessionIds.add(session.id);
        return { sessionId: session.id };
      }
      case METHODS.SESSION_SEND: {
        const p = params as SessionSendParams;
        const session = this.requireSession(p.sessionId);
        // Fire-and-forget: tokens/thinking/tool events stream as
        // notifications; a rejected run surfaces as a session_state error.
        session.prompt(p.text).catch((error: unknown) => {
          this.broadcastSessionEvent(p.sessionId, {
            type: "state",
            state: "error",
            error: error instanceof Error ? error.message : String(error),
          });
        });
        return {};
      }
      case METHODS.SESSION_CANCEL: {
        const p = params as SessionCancelParams;
        await this.requireSession(p.sessionId).cancel();
        return {};
      }
      case METHODS.SESSION_LIST: {
        return { sessions: this.manager.list().map(toWireSessionInfo) };
      }
      case METHODS.SESSION_ATTACH: {
        const p = params as SessionAttachParams;
        this.requireSession(p.sessionId);
        // P1 has no replay buffer: `since` is accepted and ignored; the client
        // only sees events from this point on.
        conn.attachedSessionIds.add(p.sessionId);
        return {};
      }
      case METHODS.HOST_INFO: {
        const result: HostInfoResult = {
          hostname: os.hostname(),
          platform: process.platform,
          arch: process.arch,
          node: process.version,
          daemonVersion: DAEMON_VERSION,
          protocolVersion: PROTOCOL_VERSION,
          sessions: this.manager.list().length,
        };
        return result;
      }
      default:
        // Unreachable: paramsValidators gates unknown methods first.
        throw new Error(`unhandled method: ${method}`);
    }
  }

  private requireSession(sessionId: string): CrowSession {
    const session = this.manager.get(sessionId);
    if (!session) {
      throw new Error(`session not found: ${sessionId}`);
    }
    return session;
  }

  /** One crow-listener per session, fanning events out to every attached connection. */
  private ensureSessionSubscription(session: CrowSession): void {
    if (this.sessionSubscriptions.has(session.id)) return;
    const unsubscribe = session.subscribe((event) => {
      this.broadcastSessionEvent(session.id, event);
    });
    this.sessionSubscriptions.set(session.id, unsubscribe);
  }

  private broadcastSessionEvent(sessionId: string, event: CrowSessionEvent): void {
    const notification = mapSessionEvent(sessionId, event);
    const frame = encodeFrame(notification);
    for (const conn of this.connections) {
      if (!conn.attachedSessionIds.has(sessionId)) continue;
      if (conn.ws.readyState === WebSocket.OPEN) {
        conn.ws.send(frame);
      }
    }
  }

  private sendFrame(conn: ConnectionState, frame: JsonRpcFrame): void {
    if (conn.ws.readyState === WebSocket.OPEN) {
      conn.ws.send(encodeFrame(frame));
    }
  }
}

function mapSessionEvent(sessionId: string, event: CrowSessionEvent) {
  switch (event.type) {
    case "token":
      return makeNotification(EVENTS.TOKEN, { sessionId, text: event.text });
    case "thinking":
      return makeNotification(EVENTS.THINKING, { sessionId, text: event.text });
    case "tool_call":
      return makeNotification(EVENTS.TOOL_CALL, {
        sessionId,
        callId: event.callId,
        tool: event.tool,
        args: event.args,
      });
    case "tool_result":
      return makeNotification(EVENTS.TOOL_RESULT, {
        sessionId,
        callId: event.callId,
        tool: event.tool,
        output: event.output,
        isError: event.isError,
      });
    case "state":
      return makeNotification(EVENTS.SESSION_STATE, {
        sessionId,
        state: event.state,
        ...(event.error !== undefined ? { error: event.error } : {}),
      });
  }
}

/** Best-effort id recovery for malformed frames that still carry a usable id. */
function extractId(raw: unknown): RequestId {
  if (typeof raw === "object" && raw !== null) {
    const id = (raw as { id?: unknown }).id;
    if (typeof id === "string" || typeof id === "number") return id;
  }
  return "unknown";
}
