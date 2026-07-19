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
  cwd: string;
  approvalMode?: "auto" | "ask";
  autoApproveTools?: string[];
}

export interface SendPromptRequest {
  sessionId: string;
  text: string;
}

export interface ApprovalRespondRequest {
  approvalId: string;
  decision: ApprovalDecision;
}

/** A daemon notification as forwarded over IPC ("daemon:event"). */
export interface DaemonEventFrame {
  method: string;
  params: unknown;
}

export type DaemonConnectionState = "connected" | "disconnected";

/** The contextBridge surface exposed to the renderer as `window.crow`. */
export interface CrowBridge {
  hostsList(): Promise<KnownHost[]>;
  hostsAdd(host: KnownHost): Promise<KnownHost[]>;
  hostsRemove(name: string): Promise<KnownHost[]>;
  hostConnect(host: KnownHost): Promise<ConnectResult>;
  hostDisconnect(): Promise<void>;
  sessionCreate(params: CreateSessionRequest): Promise<{ sessionId: string }>;
  sessionSend(params: SendPromptRequest): Promise<void>;
  sessionCancel(sessionId: string): Promise<void>;
  sessionList(): Promise<SessionInfo[]>;
  sessionAttach(sessionId: string): Promise<void>;
  approvalRespond(params: ApprovalRespondRequest): Promise<void>;
  /** Subscribe to forwarded daemon events. Returns the unsubscribe function. */
  onDaemonEvent(listener: (frame: DaemonEventFrame) => void): () => void;
  /** Subscribe to connection state pushes. Returns the unsubscribe function. */
  onDaemonState(listener: (state: DaemonConnectionState) => void): () => void;
}
