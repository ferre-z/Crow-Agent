import { EVENTS, type SessionInfo } from "@crow/protocol";

import type {
  ApprovalDecision,
  DaemonConnectionState,
  DaemonEventFrame,
  HostInfoResult,
  KnownHost,
} from "../../shared/api.ts";

/**
 * Renderer store: a single pure reducer fed by daemon events and UI actions.
 * No react imports here — App.tsx wires it with useReducer. Everything the UI
 * renders is derived from AppState via the select* helpers at the bottom.
 */

export type LiveSessionState = "idle" | "streaming" | "error" | "cancelled";

export interface TranscriptUserItem {
  kind: "user";
  id: string;
  text: string;
}

export interface TranscriptAssistantItem {
  kind: "assistant";
  id: string;
  text: string;
}

export interface TranscriptThinkingItem {
  kind: "thinking";
  id: string;
  text: string;
}

export interface TranscriptToolItem {
  kind: "tool";
  id: string;
  callId: string;
  tool: string;
  args: unknown;
  done: boolean;
  output?: string;
  isError?: boolean;
}

export interface TranscriptApprovalItem {
  kind: "approval";
  id: string;
  approvalId: string;
  tool: string;
  args: unknown;
  decision?: ApprovalDecision;
}

export type TranscriptItem =
  | TranscriptUserItem
  | TranscriptAssistantItem
  | TranscriptThinkingItem
  | TranscriptToolItem
  | TranscriptApprovalItem;

export interface SessionEntry {
  info: SessionInfo;
  /** Live state from event.session_state; richer than SessionInfo's coarse idle/busy. */
  live: LiveSessionState;
  error?: string;
  transcript: TranscriptItem[];
  /** Per-session item id counter — keeps the reducer pure (no Date.now/Math.random). */
  nextItemSeq: number;
}

export interface PendingApproval {
  approvalId: string;
  sessionId: string;
  callId: string;
  tool: string;
  args: unknown;
}

export interface AppState {
  connection: DaemonConnectionState;
  connecting: boolean;
  hosts: KnownHost[];
  /** Inline error on the connect screen (connect failure or lost connection). */
  connectError?: string;
  hostName?: string;
  hostInfo?: HostInfoResult;
  sessions: Record<string, SessionEntry>;
  sessionOrder: string[];
  activeSessionId?: string;
  /** FIFO queue; the modal shows the head. */
  pendingApprovals: PendingApproval[];
}

export type Action =
  | { type: "hosts.set"; hosts: KnownHost[] }
  | { type: "connect.started" }
  | {
      type: "connect.succeeded";
      hostName: string;
      info: HostInfoResult;
      sessions: SessionInfo[];
    }
  | { type: "connect.failed"; message: string }
  | { type: "disconnect.requested" }
  | { type: "daemon.connection"; state: DaemonConnectionState }
  | { type: "sessions.set"; sessions: SessionInfo[] }
  | { type: "session.created"; info: SessionInfo }
  | { type: "session.selected"; sessionId: string }
  | { type: "prompt.sent"; sessionId: string; text: string }
  | { type: "daemon.event"; frame: DaemonEventFrame }
  | { type: "approval.responded"; approvalId: string; decision: ApprovalDecision };

export function initialState(): AppState {
  return {
    connection: "disconnected",
    connecting: false,
    hosts: [],
    sessions: {},
    sessionOrder: [],
    pendingApprovals: [],
  };
}

export function reducer(state: AppState, action: Action): AppState {
  switch (action.type) {
    case "hosts.set":
      return { ...state, hosts: action.hosts };

    case "connect.started":
      return { ...state, connecting: true, connectError: undefined };

    case "connect.succeeded": {
      const sessions: Record<string, SessionEntry> = {};
      const sessionOrder: string[] = [];
      for (const info of action.sessions) {
        sessions[info.id] = {
          info,
          live: coarseToLive(info.state),
          transcript: [],
          nextItemSeq: 1,
        };
        sessionOrder.push(info.id);
      }
      return {
        ...state,
        connection: "connected",
        connecting: false,
        connectError: undefined,
        hostName: action.hostName,
        hostInfo: action.info,
        sessions,
        sessionOrder,
        activeSessionId: undefined,
        pendingApprovals: [],
      };
    }

    case "connect.failed":
      return { ...state, connecting: false, connectError: action.message };

    case "disconnect.requested":
      return {
        ...initialState(),
        hosts: state.hosts,
      };

    case "daemon.connection": {
      if (action.state === "connected") return state; // connect.succeeded drives that transition
      if (state.connection !== "connected") return state; // already on the connect screen
      return {
        ...initialState(),
        hosts: state.hosts,
        connectError: `lost connection to ${state.hostName ?? "the daemon"}`,
      };
    }

    case "sessions.set": {
      let next = state;
      for (const info of action.sessions) {
        next = updateSession(next, info.id, (entry) =>
          // Listed sessions we already track keep their live event state; the
          // coarse list only fills in fresh entries and refreshes info fields.
          state.sessions[info.id]
            ? { ...entry, info }
            : { ...entry, info, live: coarseToLive(info.state) },
        );
      }
      return next;
    }

    case "session.created": {
      const next = updateSession(state, action.info.id, (entry) => ({
        ...entry,
        info: action.info,
        live: coarseToLive(action.info.state),
      }));
      return { ...next, activeSessionId: action.info.id };
    }

    case "session.selected":
      if (!state.sessions[action.sessionId]) return state;
      return { ...state, activeSessionId: action.sessionId };

    case "prompt.sent":
      return updateSession(state, action.sessionId, (entry) => ({
        ...pushItem(entry, (id) => ({ kind: "user", id, text: action.text })),
        live: "streaming",
        error: undefined,
      }));

    case "daemon.event":
      return reduceDaemonEvent(state, action.frame);

    case "approval.responded": {
      const approval = state.pendingApprovals.find((a) => a.approvalId === action.approvalId);
      const next: AppState = {
        ...state,
        pendingApprovals: state.pendingApprovals.filter((a) => a.approvalId !== action.approvalId),
      };
      if (!approval) return next;
      return updateSession(next, approval.sessionId, (entry) => ({
        ...entry,
        transcript: entry.transcript.map((item) =>
          item.kind === "approval" && item.approvalId === action.approvalId
            ? { ...item, decision: action.decision }
            : item,
        ),
      }));
    }
  }
}

function reduceDaemonEvent(state: AppState, frame: DaemonEventFrame): AppState {
  const params = asRecord(frame.params);
  switch (frame.method) {
    case EVENTS.TOKEN: {
      const sessionId = asString(params?.sessionId);
      const text = asString(params?.text);
      if (!sessionId || !text) return state;
      return updateSession(state, sessionId, (entry) => {
        const last = entry.transcript.at(-1);
        if (last?.kind === "assistant") {
          return {
            ...entry,
            transcript: replaceLast(entry.transcript, { ...last, text: last.text + text }),
          };
        }
        return pushItem(entry, (id) => ({ kind: "assistant", id, text }));
      });
    }

    case EVENTS.THINKING: {
      const sessionId = asString(params?.sessionId);
      const text = asString(params?.text);
      if (!sessionId || !text) return state;
      return updateSession(state, sessionId, (entry) => {
        const last = entry.transcript.at(-1);
        if (last?.kind === "thinking") {
          return {
            ...entry,
            transcript: replaceLast(entry.transcript, { ...last, text: last.text + text }),
          };
        }
        return pushItem(entry, (id) => ({ kind: "thinking", id, text }));
      });
    }

    case EVENTS.TOOL_CALL: {
      const sessionId = asString(params?.sessionId);
      const callId = asString(params?.callId);
      const tool = asString(params?.tool);
      if (!sessionId || !callId || !tool) return state;
      return updateSession(state, sessionId, (entry) =>
        pushItem(entry, (id) => ({
          kind: "tool",
          id,
          callId,
          tool,
          args: params?.args,
          done: false,
        })),
      );
    }

    case EVENTS.TOOL_RESULT: {
      const sessionId = asString(params?.sessionId);
      const callId = asString(params?.callId);
      const tool = asString(params?.tool);
      const output = asString(params?.output) ?? "";
      const isError = params?.isError === true;
      if (!sessionId || !callId || !tool) return state;
      return updateSession(state, sessionId, (entry) => {
        const index = findLastIndex(
          entry.transcript,
          (item) => item.kind === "tool" && item.callId === callId,
        );
        if (index === -1) {
          // Result without a matching call (shouldn't happen) — surface it anyway.
          return pushItem(entry, (id) => ({
            kind: "tool",
            id,
            callId,
            tool,
            args: undefined,
            done: true,
            output,
            isError,
          }));
        }
        const item = entry.transcript[index];
        if (item?.kind !== "tool") return entry;
        const merged: TranscriptToolItem = { ...item, done: true, output, isError };
        return { ...entry, transcript: replaceAt(entry.transcript, index, merged) };
      });
    }

    case EVENTS.SESSION_STATE: {
      const sessionId = asString(params?.sessionId);
      const wireState = asString(params?.state);
      if (!sessionId || !wireState) return state;
      const error = asString(params?.error);
      return updateSession(state, sessionId, (entry) => {
        switch (wireState) {
          case "idle":
            return { ...entry, live: "idle", error: undefined };
          case "streaming":
            return { ...entry, live: "streaming" };
          case "error":
            // Abort is the normal cancel path — present it as "cancelled", not an error.
            if (error && /abort/i.test(error)) {
              return { ...entry, live: "cancelled", error: undefined };
            }
            return { ...entry, live: "error", error };
          default:
            return entry;
        }
      });
    }

    case "event.approval_request": {
      const approval = parseApprovalRequest(params);
      if (!approval) return state;
      if (state.pendingApprovals.some((a) => a.approvalId === approval.approvalId)) {
        return state;
      }
      const next: AppState = {
        ...state,
        pendingApprovals: [...state.pendingApprovals, approval],
      };
      return updateSession(next, approval.sessionId, (entry) =>
        pushItem(entry, (id) => ({
          kind: "approval",
          id,
          approvalId: approval.approvalId,
          tool: approval.tool,
          args: approval.args,
        })),
      );
    }

    default:
      return state;
  }
}

// --- internals ---------------------------------------------------------------

function coarseToLive(coarse: SessionInfo["state"]): LiveSessionState {
  return coarse === "busy" ? "streaming" : "idle";
}

function stubEntry(sessionId: string): SessionEntry {
  return {
    info: {
      id: sessionId,
      cwd: "",
      model: null,
      state: "idle",
      createdAt: "",
      approvalMode: "auto",
    },
    live: "idle",
    transcript: [],
    nextItemSeq: 1,
  };
}

function updateSession(
  state: AppState,
  sessionId: string,
  fn: (entry: SessionEntry) => SessionEntry,
): AppState {
  const existing = state.sessions[sessionId];
  const entry = existing ?? stubEntry(sessionId);
  const updated = fn(entry);
  if (updated === entry && existing) return state;
  return {
    ...state,
    sessions: { ...state.sessions, [sessionId]: updated },
    sessionOrder: state.sessionOrder.includes(sessionId)
      ? state.sessionOrder
      : [...state.sessionOrder, sessionId],
  };
}

function pushItem(entry: SessionEntry, make: (id: string) => TranscriptItem): SessionEntry {
  const id = `i${entry.nextItemSeq}`;
  return {
    ...entry,
    transcript: [...entry.transcript, make(id)],
    nextItemSeq: entry.nextItemSeq + 1,
  };
}

function replaceLast(transcript: TranscriptItem[], item: TranscriptItem): TranscriptItem[] {
  return replaceAt(transcript, transcript.length - 1, item);
}

function replaceAt(
  transcript: TranscriptItem[],
  index: number,
  item: TranscriptItem,
): TranscriptItem[] {
  return transcript.map((existing, i) => (i === index ? item : existing));
}

function findLastIndex<T>(items: T[], pred: (item: T) => boolean): number {
  for (let i = items.length - 1; i >= 0; i--) {
    const item = items[i];
    if (item !== undefined && pred(item)) return i;
  }
  return -1;
}

function asRecord(value: unknown): Record<string, unknown> | undefined {
  return typeof value === "object" && value !== null
    ? (value as Record<string, unknown>)
    : undefined;
}

function asString(value: unknown): string | undefined {
  return typeof value === "string" ? value : undefined;
}

function parseApprovalRequest(
  params: Record<string, unknown> | undefined,
): PendingApproval | undefined {
  const sessionId = asString(params?.sessionId);
  const approvalId = asString(params?.approvalId);
  const callId = asString(params?.callId);
  const tool = asString(params?.tool);
  if (!sessionId || !approvalId || !callId || !tool) return undefined;
  return { sessionId, approvalId, callId, tool, args: params?.args };
}

// --- selectors ---------------------------------------------------------------

export function selectSessions(state: AppState): SessionEntry[] {
  return state.sessionOrder
    .map((id) => state.sessions[id])
    .filter((entry): entry is SessionEntry => entry !== undefined);
}

export function selectActiveSession(state: AppState): SessionEntry | undefined {
  return state.activeSessionId ? state.sessions[state.activeSessionId] : undefined;
}

export function selectCurrentApproval(state: AppState): PendingApproval | undefined {
  return state.pendingApprovals[0];
}

/** Short label for the sidebar: cwd basename, falling back to the session id. */
export function sessionDisplayName(entry: SessionEntry): string {
  const base = basename(entry.info.cwd);
  return base || `session ${entry.info.id.slice(0, 8)}`;
}

export function basename(cwd: string): string {
  const trimmed = cwd.replace(/[/\\]+$/, "");
  if (!trimmed) return "";
  const parts = trimmed.split(/[/\\]/);
  return parts.at(-1) ?? "";
}
