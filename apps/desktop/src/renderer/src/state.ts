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

export type AgentRunState = "started" | "done" | "error";

export interface AgentRun {
  hostName: string;
  agentId: string;
  state: AgentRunState;
  output?: string;
  error?: string;
  prompt: string;
}

export interface TeamRunStep {
  step: number;
  agent: string;
  state: "running" | "done" | "error";
  output?: string;
  error?: string;
}

export type TeamRunState = "running" | "done" | "error";

export interface TeamRun {
  hostName: string;
  runId: string;
  team: string;
  input: string;
  state: TeamRunState;
  steps: TeamRunStep[];
  output?: string;
  error?: string;
}

/** What the main pane shows: a session chat or a team run timeline. */
export type ActiveView = { kind: "session"; key: string } | { kind: "team"; runId: string };

export interface AppState {
  hosts: KnownHost[];
  fleet: Record<string, FleetHostEntry>;
  sessions: Record<string, SessionEntry>;
  sessionOrder: string[];
  activeView?: ActiveView;
  agentRuns: Record<string, AgentRun>;
  teamRuns: Record<string, TeamRun>;
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
  | { type: "agent.spawned"; hostName: string; agentId: string; prompt: string }
  | { type: "team.started"; hostName: string; runId: string; team: string; input: string }
  | { type: "team.selected"; hostName: string; runId: string }
  | { type: "daemon.event"; frame: DaemonEventFrame }
  | { type: "approval.responded"; approvalId: string; decision: ApprovalDecision };

export function initialState(): AppState {
  return {
    hosts: [],
    fleet: {},
    sessions: {},
    sessionOrder: [],
    agentRuns: {},
    teamRuns: {},
    pendingApprovals: [],
  };
}

export function makeSessionKey(hostName: string, sessionId: string): string {
  return `${hostName}:${sessionId}`;
}

/** Composite key for agent/team runs — the same id on two hosts stays distinct. */
export function makeRunKey(hostName: string, runId: string): string {
  return `${hostName}:${runId}`;
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
      return { ...next, activeView: { kind: "session", key } };
    }

    case "session.selected": {
      const key = makeSessionKey(action.hostName, action.sessionId);
      if (!state.sessions[key]) return state;
      return { ...state, activeView: { kind: "session", key } };
    }

    case "prompt.sent": {
      const key = makeSessionKey(action.hostName, action.sessionId);
      return updateSession(state, key, (entry) => ({
        ...pushItem(entry, (id) => ({ kind: "user", id, text: action.text })),
        live: "streaming",
        error: undefined,
      }));
    }

    case "agent.spawned": {
      const key = makeRunKey(action.hostName, action.agentId);
      const run: AgentRun = {
        hostName: action.hostName,
        agentId: action.agentId,
        state: "started",
        prompt: action.prompt,
      };
      return { ...state, agentRuns: { ...state.agentRuns, [key]: run } };
    }

    case "team.started": {
      const key = makeRunKey(action.hostName, action.runId);
      const run: TeamRun = {
        hostName: action.hostName,
        runId: action.runId,
        team: action.team,
        input: action.input,
        state: "running",
        steps: [],
      };
      return {
        ...state,
        teamRuns: { ...state.teamRuns, [key]: run },
        activeView: { kind: "team", runId: key },
      };
    }

    case "team.selected": {
      const key = makeRunKey(action.hostName, action.runId);
      if (!state.teamRuns[key]) return state;
      return { ...state, activeView: { kind: "team", runId: key } };
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
  const dropped = (key: string): boolean => prefixes.some((p) => key.startsWith(p));
  const sessions: Record<string, SessionEntry> = {};
  const sessionOrder: string[] = [];
  for (const key of state.sessionOrder) {
    if (dropped(key)) continue;
    sessions[key] = state.sessions[key]!;
    sessionOrder.push(key);
  }
  const agentRuns: Record<string, AgentRun> = {};
  for (const [key, run] of Object.entries(state.agentRuns)) {
    if (!dropped(key)) agentRuns[key] = run;
  }
  const teamRuns: Record<string, TeamRun> = {};
  for (const [key, run] of Object.entries(state.teamRuns)) {
    if (!dropped(key)) teamRuns[key] = run;
  }
  let activeView = state.activeView;
  if (activeView?.kind === "session" && dropped(activeView.key)) {
    const fallback = sessionOrder[0];
    activeView = fallback ? { kind: "session", key: fallback } : undefined;
  } else if (activeView?.kind === "team" && dropped(activeView.runId)) {
    activeView = undefined;
  }
  return { ...state, sessions, sessionOrder, agentRuns, teamRuns, activeView };
}

function reduceDaemonEvent(state: AppState, frame: DaemonEventFrame): AppState {
  const params = asRecord(frame.params);
  // Agent/team events carry no sessionId — route them before the session guard.
  if (frame.method === EVENTS.AGENT) return reduceAgentEvent(state, frame.hostName, params);
  if (frame.method === EVENTS.TEAM) return reduceTeamEvent(state, frame.hostName, params);

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

function reduceAgentEvent(
  state: AppState,
  hostName: string,
  params: Record<string, unknown> | undefined,
): AppState {
  const agentId = asString(params?.agentId);
  const wireState = asString(params?.state);
  if (!agentId || !wireState) return state;
  const key = makeRunKey(hostName, agentId);
  const run = state.agentRuns[key];
  if (!run) return state;
  let updated: AgentRun;
  switch (wireState) {
    case "started":
      updated = { ...run, state: "started" };
      break;
    case "done":
      updated = { ...run, state: "done", output: asString(params?.output) };
      break;
    case "error":
      updated = { ...run, state: "error", error: asString(params?.error) };
      break;
    default:
      return state;
  }
  return { ...state, agentRuns: { ...state.agentRuns, [key]: updated } };
}

function reduceTeamEvent(
  state: AppState,
  hostName: string,
  params: Record<string, unknown> | undefined,
): AppState {
  const runId = asString(params?.runId);
  const wireState = asString(params?.state);
  if (!runId || !wireState) return state;
  const key = makeRunKey(hostName, runId);
  const run = state.teamRuns[key];
  if (!run) return state;

  switch (wireState) {
    case "step_started": {
      const step = asNumber(params?.step);
      const agent = asString(params?.agent);
      if (step === undefined || !agent) return state;
      const next: TeamRunStep = { step, agent, state: "running" };
      const steps = run.steps.some((s) => s.step === step)
        ? run.steps.map((s) => (s.step === step ? next : s))
        : [...run.steps, next];
      return { ...state, teamRuns: { ...state.teamRuns, [key]: { ...run, steps } } };
    }

    case "step_done": {
      const step = asNumber(params?.step);
      const output = asString(params?.output);
      const index = run.steps.findIndex(
        (s) =>
          (step !== undefined ? s.step === step : s.state === "running") && s.state === "running",
      );
      if (index === -1) return state;
      const steps = run.steps.map((s, i) =>
        i === index ? { ...s, state: "done" as const, output } : s,
      );
      return { ...state, teamRuns: { ...state.teamRuns, [key]: { ...run, steps } } };
    }

    case "done":
      return {
        ...state,
        teamRuns: {
          ...state.teamRuns,
          [key]: { ...run, state: "done", output: asString(params?.output) },
        },
      };

    case "error": {
      const error = asString(params?.error);
      const step = asNumber(params?.step);
      const steps = run.steps.map((s) => {
        const failed = step !== undefined ? s.step === step : s.state === "running";
        return failed && s.state === "running" ? { ...s, state: "error" as const, error } : s;
      });
      return {
        ...state,
        teamRuns: { ...state.teamRuns, [key]: { ...run, state: "error", error, steps } },
      };
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

function asNumber(value: unknown): number | undefined {
  return typeof value === "number" && Number.isFinite(value) ? value : undefined;
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
  return state.activeView?.kind === "session" ? state.sessions[state.activeView.key] : undefined;
}

export function selectCurrentApproval(state: AppState): PendingApproval | undefined {
  return state.pendingApprovals[0];
}

export function selectConnectedHosts(state: AppState): FleetHostEntry[] {
  return Object.values(state.fleet);
}

export function selectTeamRuns(state: AppState): TeamRun[] {
  return Object.values(state.teamRuns);
}

export function selectAgentRuns(state: AppState): AgentRun[] {
  return Object.values(state.agentRuns);
}

export function selectActiveTeamRun(state: AppState): TeamRun | undefined {
  return state.activeView?.kind === "team" ? state.teamRuns[state.activeView.runId] : undefined;
}

export function selectActiveHostName(state: AppState): string | undefined {
  const view = state.activeView;
  if (view?.kind === "session") return parseSessionKey(view.key).hostName;
  if (view?.kind === "team") return state.teamRuns[view.runId]?.hostName;
  return undefined;
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
