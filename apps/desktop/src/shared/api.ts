import type {
  ApprovalDecision,
  ApprovalRequestEvent,
  HostInfoResult,
  SessionInfo,
} from "@crow/protocol";

import type { KnownHost } from "./hosts.ts";

export type { ApprovalDecision, ApprovalRequestEvent, HostInfoResult, KnownHost, SessionInfo };

/** Result of a host:connect attempt — structured so the renderer can tell auth from unreachable. */
export type ConnectResult =
  | { ok: true; info: HostInfoResult }
  | { ok: false; kind: "auth" | "unreachable" | "error"; message: string };

export interface CreateSessionRequest {
  hostName: string;
  cwd: string;
  approvalMode?: "auto" | "ask";
  autoApproveTools?: string[];
}

export interface SendPromptRequest {
  hostName: string;
  sessionId: string;
  text: string;
}

export interface ApprovalRespondRequest {
  hostName: string;
  approvalId: string;
  decision: ApprovalDecision;
}

/** A daemon notification as forwarded over IPC ("daemon:event"). */
export interface DaemonEventFrame {
  hostName: string;
  method: string;
  params: unknown;
}

export interface DaemonConnectionStateFrame {
  hostName: string;
  state: DaemonConnectionState;
}

export type DaemonConnectionState = "connected" | "disconnected";

/** Per-host view used by the fleet sidebar. */
export interface HostConnectionView {
  host: KnownHost;
  state: DaemonConnectionState;
  info?: HostInfoResult;
  error?: string;
}

/** The contextBridge surface exposed to the renderer as `window.crow`. */
export interface CrowBridge {
  hostsList(): Promise<KnownHost[]>;
  hostsAdd(host: KnownHost): Promise<KnownHost[]>;
  hostsRemove(name: string): Promise<KnownHost[]>;
  /** Connect (or reconnect) to a host. Idempotent: already-connected returns cached info. */
  hostConnect(host: KnownHost): Promise<ConnectResult>;
  hostDisconnect(hostName: string): Promise<void>;
  fleetList(): Promise<HostConnectionView[]>;
  sessionCreate(params: CreateSessionRequest): Promise<{ sessionId: string }>;
  sessionSend(params: SendPromptRequest): Promise<void>;
  sessionCancel(params: { hostName: string; sessionId: string }): Promise<void>;
  sessionList(hostName: string): Promise<SessionInfo[]>;
  sessionAttach(params: { hostName: string; sessionId: string }): Promise<void>;
  approvalRespond(params: ApprovalRespondRequest): Promise<void>;
  /** Subscribe to forwarded daemon events. Returns the unsubscribe function. */
  onDaemonEvent(listener: (frame: DaemonEventFrame) => void): () => void;
  /** Subscribe to per-host connection state pushes. Returns the unsubscribe function. */
  onDaemonState(listener: (frame: DaemonConnectionStateFrame) => void): () => void;
}
