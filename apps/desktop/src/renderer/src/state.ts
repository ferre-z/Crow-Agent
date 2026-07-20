import { EVENTS, type SessionInfo } from "@crow/protocol";

import type {
  ApprovalDecision,
  DaemonConnectionState,
  DaemonEventFrame,
  HostConnectionView,
  HostInfoResult,
  KnownHost,
} from "../../shared/api.ts";

/**
 * Renderer store: a single pure reducer fed by daemon events and UI actions.
 * P3: multihost. Sessions are keyed by `${hostName}:${sessionId}` so the same
 * id on two hosts stays distinct. The fleet slice tracks each saved host's
 * connection state and host.info.
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
  hostName: string;
  info: SessionInfo;
  live: LiveSessionState;
  error?: string;
  transcript: TranscriptItem[];
  nextItemSeq: number;
}

export interface PendingApproval {
  hostName: string;
  sessionId: string;
  approvalId: string;
  callId: string;
  tool: string;
  args: unknown;
}

export interface FleetHostEntry {
  host: KnownHost;
  state: DaemonConnectionState;
  info?: HostInfoResult;
  error?: string;
  connecting?: boolean;
}

export interface AppState {
  hosts: KnownHost[];
  fleet: Record<string, FleetHostEntry>;
  sessions: Record<string, SessionEntry>;
  sessionOrder: string[];
  activeSessionKey?: string;
  pendingApprovals: PendingApproval[];
}

export type Action =
  | { type: "hosts.set"; hosts: KnownHost[] }
  | { type: "fleet.set"; views: HostConnectionView[] }
  | { type: "fleet.update"; hostName: string; patch: Partial<FleetHostEntry> }
  | { type: "connect.started"; hostName: string }
  | { type: "connect.succeeded"; hostName: string; info: HostInfoResult }
  | { type: "connect.failed"; hostName: string; message: string }
  | { type: "host.disconnect"; hostName: string }
  | { type: "host.remove"; hostName: string }
  | { type: "sessions.set"; hostName: string; sessions: SessionInfo[] }
  | { type: "session.created"; hostName: string; info: SessionInfo }
  | { type: "session.selected"; hostName: string; sessionId: string }
  | { type: "prompt.sent"; hostName: string; sessionId: string; text: string }
  | { type: "daemon.event"; frame: DaemonEventFrame }
  | { type: "approval.responded"; approvalId: string; decision: ApprovalDecision };

export function initialState(): AppState {
  return {
    hosts: [],
    fleet: {},
    sessions: {},
    sessionOrder: [],
    pendingApprovals: [],
  };
}

export function makeSessionKey(hostName: string, sessionId: string): string {
  return `${hostName}:${sessionId}`;
}

export function parseSessionKey(key: string): { hostName: string; sessionId: string } {
  const idx = key.indexOf(":");
  if (idx === -1) return { hostName: "", sessionId: key };
  return { hostName: key.slice(0, idx), sessionId: key.slice(idx + 1) };
}

export function reducer(state: AppState, action: Action): AppState {
  switch (action.type) {
    case "hosts.set":
      return { ...state, hosts: action.hosts };

    case "fleet.set": {
      const fleet: Record<string, FleetHostEntry> = {};
      for (const view of action.views) {
        fleet[view.host.name] = {
          host: view.host,
          state: view.state,
          info: view.info,
          ...(view.error !== undefined ? { error: view.error } : {}),
        };
      }
      return { ...state, fleet };
    }

    case "fleet.update": {
      const existing = state.fleet[action.hostName];
      if (!existing) return state;
      return {
        ...state,
        fleet: {
          ...state.fleet,
          [action.hostName]: { ...existing, ...action.patch },
        },
      };
    }

    case "connect.started":
      return setFleetHost(state, action.hostName, {
        state: "disconnected",
        connecting: true,
        error: undefined,
      });

    case "connect.succeeded":
      return setFleetHost(state, action.hostName, {
        state: "connected",
        connecting: false,
        info: action.info,
        error: undefined,
      });

    case "connect.failed":
      return setFleetHost(state, action.hostName, {
        state: "disconnected",
        connecting: false,
        error: action.message,
      });

    case "host.disconnect":
      return dropHostSessions(
        setFleetHost(state, action.hostName, {
          state: "disconnected",
          connecting: false,
          error: undefined,
        }),
        [action.hostName],
      );

    case "host.remove": {
      const fleet = { ...state.fleet };
      delete fleet[action.hostName];
      return dropHostSessions({ ...state, fleet }, [action.hostName]);
    }

    case "sessions.set": {
      let next = state;
      for (const info of action.sessions) {
        const key = makeSessionKey(action.hostName, info.id);
        next = updateSession(next, key, (entry) =>
          next.sessions[key]
            ? { ...entry, info }
            : { ...entry, info, live: coarseToLive(info.state) },
        );
      }
      return next;
    }

    case "session.created": {
      const key = makeSessionKey(action.hostName, action.info.id);
      const next = updateSession(state, key, (entry) => ({
        ...entry,
        hostName: action.hostName,
        info: action.info,
        live: coarseToLive(action.info.state),
      }));
      return { ...next, activeSessionKey: key };
    }

    case "session.selected": {
      const key = makeSessionKey(action.hostName, action.sessionId);
      if (!state.sessions[key]) return state;
      return { ...state, activeSessionKey: key };
    }

    case "prompt.sent": {
      const key = makeSessionKey(action.hostName, action.sessionId);
      return updateSession(state, key, (entry) => ({
        ...pushItem(entry, (id) => ({ kind: "user", id, text: action.text })),
        live: "streaming",
        error: undefined,
      }));
    }

    case "daemon.event":
      return reduceDaemonEvent(state, action.frame);

    case "approval.responded": {
      const approval = state.pendingApprovals.find((a) => a.approvalId === action.approvalId);
      const next: AppState = {
        ...state,
        pendingApprovals: state.pendingApprovals.filter((a) => a.approvalId !== action.approvalId),
      };
      if (!approval) return next;
      const key = makeSessionKey(approval.hostName, approval.sessionId);
      return updateSession(next, key, (entry) => ({
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

function setFleetHost(state: AppState, hostName: string, patch: Partial<FleetHostEntry>): AppState {
  const existing = state.fleet[hostName];
  return {
    ...state,
    fleet: {
      ...state.fleet,
      [hostName]: existing
        ? { ...existing, ...patch }
        : {
            host: state.hosts.find((h) => h.name === hostName) ?? {
              name: hostName,
              url: "",
              token: "",
            },
            state: "disconnected",
            ...patch,
          },
    },
  };
}

function dropHostSessions(state: AppState, hostNames: string[]): AppState {
  const prefixes = hostNames.map((h) => `${h}:`);
  const sessions: Record<string, SessionEntry> = {};
  const sessionOrder: string[] = [];
  let activeSessionKey = state.activeSessionKey;
  for (const key of state.sessionOrder) {
    if (prefixes.some((p) => key.startsWith(p))) continue;
    sessions[key] = state.sessions[key]!;
    sessionOrder.push(key);
  }
  if (activeSessionKey && prefixes.some((p) => activeSessionKey!.startsWith(p))) {
    activeSessionKey = sessionOrder[0];
  }
  return { ...state, sessions, sessionOrder, activeSessionKey };
}

function reduceDaemonEvent(state: AppState, frame: DaemonEventFrame): AppState {
  const params = asRecord(frame.params);
  const sessionId = asString(params?.sessionId);
  if (!sessionId) return state;
  const key = makeSessionKey(frame.hostName, sessionId);

  switch (frame.method) {
    case EVENTS.TOKEN: {
      const text = asString(params?.text);
      if (!text) return state;
      return updateSession(state, key, (entry) => {
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
      const text = asString(params?.text);
      if (!text) return state;
      return updateSession(state, key, (entry) => {
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
      const callId = asString(params?.callId);
      const tool = asString(params?.tool);
      if (!callId || !tool) return state;
      return updateSession(state, key, (entry) =>
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
      const callId = asString(params?.callId);
      const tool = asString(params?.tool);
      const output = asString(params?.output) ?? "";
      const isError = params?.isError === true;
      if (!callId || !tool) return state;
      return updateSession(state, key, (entry) => {
        const index = findLastIndex(
          entry.transcript,
          (item) => item.kind === "tool" && item.callId === callId,
        );
        if (index === -1) {
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
      const wireState = asString(params?.state);
      if (!wireState) return state;
      const error = asString(params?.error);
      return updateSession(state, key, (entry) => {
        switch (wireState) {
          case "idle":
            return { ...entry, live: "idle", error: undefined };
          case "streaming":
            return { ...entry, live: "streaming" };
          case "error":
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
      const approval = parseApprovalRequest(frame.hostName, params);
      if (!approval) return state;
      if (state.pendingApprovals.some((a) => a.approvalId === approval.approvalId)) {
        return state;
      }
      const next: AppState = {
        ...state,
        pendingApprovals: [...state.pendingApprovals, approval],
      };
      return updateSession(next, key, (entry) =>
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

function stubEntry(hostName: string, sessionId: string): SessionEntry {
  return {
    hostName,
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
  key: string,
  fn: (entry: SessionEntry) => SessionEntry,
): AppState {
  const existing = state.sessions[key];
  const { hostName, sessionId } = parseSessionKey(key);
  const entry = existing ?? stubEntry(hostName, sessionId);
  const updated = fn(entry);
  if (updated === entry && existing) return state;
  return {
    ...state,
    sessions: { ...state.sessions, [key]: updated },
    sessionOrder: state.sessionOrder.includes(key)
      ? state.sessionOrder
      : [...state.sessionOrder, key],
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
  hostName: string,
  params: Record<string, unknown> | undefined,
): PendingApproval | undefined {
  const sessionId = asString(params?.sessionId);
  const approvalId = asString(params?.approvalId);
  const callId = asString(params?.callId);
  const tool = asString(params?.tool);
  if (!sessionId || !approvalId || !callId || !tool) return undefined;
  return { hostName, sessionId, approvalId, callId, tool, args: params?.args };
}

// --- selectors ---------------------------------------------------------------

export function selectSessions(state: AppState): SessionEntry[] {
  return state.sessionOrder
    .map((key) => state.sessions[key])
    .filter((entry): entry is SessionEntry => entry !== undefined);
}

export function selectActiveSession(state: AppState): SessionEntry | undefined {
  return state.activeSessionKey ? state.sessions[state.activeSessionKey] : undefined;
}

export function selectCurrentApproval(state: AppState): PendingApproval | undefined {
  return state.pendingApprovals[0];
}

export function selectConnectedHosts(state: AppState): FleetHostEntry[] {
  return Object.values(state.fleet);
}

export function selectActiveHostName(state: AppState): string | undefined {
  return state.activeSessionKey ? parseSessionKey(state.activeSessionKey).hostName : undefined;
}

export function sessionDisplayName(entry: SessionEntry): string {
  const base = basename(entry.info.cwd);
  return `${entry.hostName}: ${base || entry.info.id.slice(0, 8)}`;
}

export function basename(cwd: string): string {
  const trimmed = cwd.replace(/[/\\]+$/, "");
  if (!trimmed) return "";
  const parts = trimmed.split(/[/\\]/);
  return parts.at(-1) ?? "";
}
