import {
  encodeFrame,
  EVENTS,
  makeNotification,
  makeRequest,
  methodParamsSchemas,
  METHODS,
  RPC_ERRORS,
  type HostInfoResult,
  type RequestId,
  type SessionCreateParams,
  type SessionCreateResult,
  type SessionInfo,
  type SessionListResult,
} from "@crow/protocol";
import WebSocket from "ws";

import {
  APPROVAL_REQUEST_EVENT,
  APPROVAL_RESPOND_METHOD,
  approvalRespondParamsSchema,
  type ApprovalDecision,
} from "./approval.ts";
import { CrowClientError } from "./errors.ts";

export interface CrowClientOptions {
  /** Daemon WebSocket URL, e.g. `ws://127.0.0.1:7749`. */
  url: string;
  /** Bearer token from `~/.crow/daemon.json` on the daemon host. */
  token: string;
  /** Connect and per-call timeout in milliseconds. Defaults to 10_000. */
  timeoutMs?: number;
}

export type ConnectionState = "connected" | "disconnected";
export type DaemonEventListener = (method: string, params: unknown) => void;
export type ConnectionStateListener = (state: ConnectionState) => void;

const DEFAULT_TIMEOUT_MS = 10_000;

/** session.create params, including the P2 approval fields. */
export type CreateSessionParams = SessionCreateParams;

/** Minimal structural view of the zod param validators. */
interface ParamsValidator {
  safeParse(
    input: unknown,
  ):
    { success: true; data: unknown } | { success: false; error: { issues: { message: string }[] } };
}

const outgoingValidators: Record<string, ParamsValidator> = {
  ...methodParamsSchemas,
  [APPROVAL_RESPOND_METHOD]: approvalRespondParamsSchema,
};

/** Notification methods routed to onEvent listeners; everything else is dropped. */
const KNOWN_EVENT_METHODS: ReadonlySet<string> = new Set<string>([
  ...Object.values(EVENTS),
  APPROVAL_REQUEST_EVENT,
]);

interface PendingCall {
  resolve: (value: unknown) => void;
  reject: (error: CrowClientError) => void;
  timer: NodeJS.Timeout;
}

/**
 * Typed client for the Crow daemon wire protocol: JSON-RPC 2.0 over
 * newline-delimited WebSocket frames, bearer-token auth at upgrade time.
 *
 * One CrowClient per (app, daemon) pair. No auto-reconnect — when the socket
 * drops, pending calls reject with a "disconnected" CrowClientError and the
 * state listeners fire; the app decides whether to dial again.
 */
export class CrowClient {
  private readonly url: string;
  private readonly token: string;
  private readonly timeoutMs: number;
  private ws: WebSocket | undefined;
  private nextId = 1;
  private buffer = "";
  private state: ConnectionState = "disconnected";
  private readonly pending = new Map<RequestId, PendingCall>();
  private readonly eventListeners = new Set<DaemonEventListener>();
  private readonly stateListeners = new Set<ConnectionStateListener>();

  constructor(options: CrowClientOptions) {
    this.url = options.url;
    this.token = options.token;
    this.timeoutMs = options.timeoutMs ?? DEFAULT_TIMEOUT_MS;
  }

  get connectionState(): ConnectionState {
    return this.state;
  }

  connect(): Promise<void> {
    if (this.state === "connected" && this.ws?.readyState === WebSocket.OPEN) {
      return Promise.resolve();
    }
    return new Promise<void>((resolve, reject) => {
      const ws = new WebSocket(this.url, {
        headers: { Authorization: `Bearer ${this.token}` },
      });
      this.ws = ws;
      const onOpen = () => {
        cleanup();
        this.attachSocket(ws);
        this.setState("connected");
        resolve();
      };
      const onError = (error: Error) => {
        cleanup();
        this.ws = undefined;
        reject(toConnectError(this.url, error));
      };
      const onClose = () => {
        cleanup();
        this.ws = undefined;
        reject(new CrowClientError(`connection to daemon at ${this.url} closed during handshake`));
      };
      const cleanup = () => {
        clearTimeout(timer);
        ws.removeListener("open", onOpen);
        ws.removeListener("error", onError);
        ws.removeListener("close", onClose);
      };
      const timer = setTimeout(() => {
        cleanup();
        this.ws = undefined;
        // terminate() on a connecting socket makes ws raise a late "error"
        // event ("closed before the connection was established"); consume it.
        ws.on("error", () => {});
        ws.terminate();
        reject(
          new CrowClientError(
            `timed out connecting to daemon at ${this.url} after ${this.timeoutMs}ms`,
          ),
        );
      }, this.timeoutMs);
      timer.unref();
      ws.once("open", onOpen);
      ws.once("error", onError);
      ws.once("close", onClose);
    });
  }

  async close(): Promise<void> {
    const ws = this.ws;
    this.ws = undefined;
    this.rejectAllPending(new CrowClientError("client closed the connection to the daemon"));
    this.setState("disconnected");
    if (!ws || ws.readyState === WebSocket.CLOSED) return;
    await new Promise<void>((resolve) => {
      ws.once("close", () => resolve());
      ws.once("error", () => resolve());
      if (ws.readyState === WebSocket.CONNECTING) ws.terminate();
      else ws.close();
    });
  }

  call<T = unknown>(method: string, params?: unknown): Promise<T> {
    const invalid = validateOutgoing(method, params);
    if (invalid) return Promise.reject(invalid);
    const ws = this.ws;
    if (this.state !== "connected" || !ws || ws.readyState !== WebSocket.OPEN) {
      return Promise.reject(new CrowClientError(`not connected to a daemon (call ${method})`));
    }
    const id = this.nextId++;
    return new Promise<T>((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new CrowClientError(`call ${method} timed out after ${this.timeoutMs}ms`));
      }, this.timeoutMs);
      timer.unref();
      this.pending.set(id, {
        resolve: (value: unknown) => resolve(value as T),
        reject,
        timer,
      });
      try {
        ws.send(encodeFrame(makeRequest(id, method, params)));
      } catch (error) {
        clearTimeout(timer);
        this.pending.delete(id);
        reject(
          new CrowClientError(`failed to send call ${method}: socket unavailable`, {
            cause: error,
          }),
        );
      }
    });
  }

  /** Fire-and-forget notification. Throws synchronously on invalid params or a dead socket. */
  notify(method: string, params?: unknown): void {
    const invalid = validateOutgoing(method, params);
    if (invalid) throw invalid;
    const ws = this.ws;
    if (this.state !== "connected" || !ws || ws.readyState !== WebSocket.OPEN) {
      throw new CrowClientError(`not connected to a daemon (notify ${method})`);
    }
    ws.send(encodeFrame(makeNotification(method, params)));
  }

  /** Subscribe to daemon `event.*` notifications. Returns the unsubscribe function. */
  onEvent(listener: DaemonEventListener): () => void {
    this.eventListeners.add(listener);
    return () => this.eventListeners.delete(listener);
  }

  /** Subscribe to connection state transitions. Returns the unsubscribe function. */
  onStateChange(listener: ConnectionStateListener): () => void {
    this.stateListeners.add(listener);
    return () => this.stateListeners.delete(listener);
  }

  // --- Typed convenience wrappers -------------------------------------------

  hostInfo(): Promise<HostInfoResult> {
    return this.call(METHODS.HOST_INFO, {});
  }

  createSession(params: CreateSessionParams): Promise<SessionCreateResult> {
    return this.call(METHODS.SESSION_CREATE, params);
  }

  async sendPrompt(sessionId: string, text: string): Promise<void> {
    await this.call(METHODS.SESSION_SEND, { sessionId, text });
  }

  async cancelSession(sessionId: string): Promise<void> {
    await this.call(METHODS.SESSION_CANCEL, { sessionId });
  }

  async listSessions(): Promise<SessionInfo[]> {
    const result = await this.call<SessionListResult>(METHODS.SESSION_LIST, {});
    return result.sessions;
  }

  async attachSession(sessionId: string): Promise<void> {
    await this.call(METHODS.SESSION_ATTACH, { sessionId });
  }

  respondApproval(approvalId: string, decision: ApprovalDecision): void {
    this.notify(APPROVAL_RESPOND_METHOD, { approvalId, decision });
  }

  // --- Internals --------------------------------------------------------------

  private attachSocket(ws: WebSocket): void {
    ws.on("message", (data: Buffer) => this.onMessage(data));
    ws.on("close", () => this.onSocketClosed());
    ws.on("error", () => {
      // The "close" handler does the real cleanup; swallow transport errors so
      // ws never raises an unhandled "error" event in the host process.
    });
  }

  private onSocketClosed(): void {
    this.ws = undefined;
    this.setState("disconnected");
    this.rejectAllPending(new CrowClientError("disconnected from daemon"));
  }

  private onMessage(data: Buffer): void {
    this.buffer += data.toString("utf8");
    const lines = this.buffer.split("\n");
    this.buffer = lines.pop() ?? "";
    for (const line of lines) {
      if (line.trim().length === 0) continue;
      this.onLine(line);
    }
  }

  private onLine(line: string): void {
    let frame: unknown;
    try {
      frame = JSON.parse(line);
    } catch {
      return; // the daemon is trusted, but never let one bad line kill the loop
    }
    if (typeof frame !== "object" || frame === null) return;
    const f = frame as Record<string, unknown>;
    if (typeof f.method === "string") {
      // Notifications route to event listeners; requests from the daemon are
      // not part of the protocol and are ignored.
      if (!("id" in f) && KNOWN_EVENT_METHODS.has(f.method)) {
        this.emitEvent(f.method, f.params);
      }
      return;
    }
    const id = f.id;
    if (typeof id !== "string" && typeof id !== "number") return;
    const entry = this.pending.get(id);
    if (!entry) return; // late response after timeout/disconnect
    this.pending.delete(id);
    clearTimeout(entry.timer);
    const error = f.error as { code?: unknown; message?: unknown } | undefined;
    if (typeof error === "object" && error !== null) {
      entry.reject(
        new CrowClientError(
          typeof error.message === "string" ? error.message : "daemon call failed",
          { code: typeof error.code === "number" ? error.code : undefined },
        ),
      );
    } else {
      entry.resolve(f.result);
    }
  }

  private emitEvent(method: string, params: unknown): void {
    for (const listener of this.eventListeners) {
      try {
        listener(method, params);
      } catch {
        // A throwing listener must not break frame processing for everyone else.
      }
    }
  }

  private setState(state: ConnectionState): void {
    if (this.state === state) return;
    this.state = state;
    for (const listener of this.stateListeners) {
      try {
        listener(state);
      } catch {
        // Same rationale as emitEvent.
      }
    }
  }

  private rejectAllPending(error: CrowClientError): void {
    for (const entry of this.pending.values()) {
      clearTimeout(entry.timer);
      entry.reject(error);
    }
    this.pending.clear();
  }
}

/** Cheap outgoing-param safety: validate against the wire schemas when we know them. */
function validateOutgoing(method: string, params: unknown): CrowClientError | undefined {
  const validator = outgoingValidators[method];
  if (!validator) return undefined;
  const parsed = validator.safeParse(params ?? {});
  if (parsed.success) return undefined;
  const detail = parsed.error.issues.map((issue) => issue.message).join("; ");
  return new CrowClientError(`invalid params for ${method}: ${detail}`, {
    code: RPC_ERRORS.INVALID_PARAMS,
  });
}

function toConnectError(url: string, error: Error): CrowClientError {
  const response = /Unexpected server response: (\d+)/.exec(error.message);
  if (response?.[1] === "401") {
    return new CrowClientError(
      `daemon at ${url} rejected the connection (HTTP 401): check the auth token`,
      { cause: error },
    );
  }
  if (response) {
    return new CrowClientError(
      `daemon at ${url} rejected the WebSocket upgrade (HTTP ${response[1]})`,
      { cause: error },
    );
  }
  const code = (error as NodeJS.ErrnoException).code;
  if (code === "ECONNREFUSED") {
    return new CrowClientError(`cannot reach daemon at ${url}: connection refused`, {
      cause: error,
    });
  }
  if (code === "ENOTFOUND" || code === "EAI_AGAIN") {
    return new CrowClientError(`cannot reach daemon at ${url}: host not found (${code})`, {
      cause: error,
    });
  }
  return new CrowClientError(`failed to connect to daemon at ${url}: ${error.message}`, {
    cause: error,
  });
}
