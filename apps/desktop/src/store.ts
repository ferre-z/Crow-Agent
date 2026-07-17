import { create } from "zustand";
import type {
  AgentEvent,
  ReplayEvent,
  ToolOutcome,
} from "./ipc/events";

export type StreamEntry =
  | { kind: "user"; id: string; content: string }
  | { kind: "assistant"; id: string; text: string }
  | { kind: "reasoning"; id: string; text: string }
  | {
      kind: "tool";
      call_id: string;
      name: string;
      args: unknown;
      outputChunks: string[];
      is_error: boolean;
      status: "running" | "success" | "error";
    }
  | { kind: "run_finished"; message: string }
  | {
      kind: "run_failed";
      code: string;
      message: string;
      retryable: boolean;
    }
  | { kind: "run_cancelled" };

export type AgentState = "idle" | "sampling" | "executing_tool" | "failed";

export interface UsageInfo {
  input_tokens: number;
  output_tokens: number;
}

export interface ApprovalRequest {
  ask_id: string;
  name: string;
  args: unknown;
}

export interface SessionMeta {
  session_id: string;
  started_at: number;
  schema_version: number;
  path: string;
}

interface CrowState {
  sessions: SessionMeta[];
  activeSessionId: string | null;
  activeSessionPath: string | null;
  runId: string | null;
  agentState: AgentState;
  entries: StreamEntry[];
  usage: UsageInfo;
  approvalQueue: ApprovalRequest[];
  selectedEntryIndex: number | null;

  setSessions: (sessions: SessionMeta[]) => void;
  setActiveSession: (id: string, path: string) => void;
  applyLiveEvent: (event: AgentEvent) => void;
  applyReplayEvents: (events: ReplayEvent[]) => void;
  addApproval: (req: ApprovalRequest) => void;
  removeApproval: (askId: string) => void;
  setSelectedEntry: (index: number | null) => void;
  reset: () => void;
}

function decodeChunk(chunk: number[]): string {
  return new TextDecoder().decode(new Uint8Array(chunk));
}

function outcomeOutput(outcome: ToolOutcome): string {
  if ("Success" in outcome) return outcome.Success.output;
  return outcome.Error.message;
}

function nextId(): string {
  return crypto.randomUUID();
}

function appendToLastAssistant(
  entries: StreamEntry[],
  text: string,
): StreamEntry[] {
  const last = entries[entries.length - 1];
  if (last && last.kind === "assistant") {
    return [
      ...entries.slice(0, -1),
      { ...last, text: last.text + text },
    ];
  }
  return [...entries, { kind: "assistant", id: nextId(), text }];
}

function appendToLastReasoning(
  entries: StreamEntry[],
  text: string,
): StreamEntry[] {
  const last = entries[entries.length - 1];
  if (last && last.kind === "reasoning") {
    return [
      ...entries.slice(0, -1),
      { ...last, text: last.text + text },
    ];
  }
  return [...entries, { kind: "reasoning", id: nextId(), text }];
}

function appendToolOutput(
  entries: StreamEntry[],
  callId: string,
  text: string,
): StreamEntry[] {
  return entries.map((e) => {
    if (e.kind === "tool" && e.call_id === callId) {
      return { ...e, outputChunks: [...e.outputChunks, text] };
    }
    return e;
  });
}

function finishTool(
  entries: StreamEntry[],
  callId: string,
  is_error: boolean,
): StreamEntry[] {
  return entries.map((e) => {
    if (e.kind === "tool" && e.call_id === callId) {
      return {
        ...e,
        status: (is_error ? "error" : "success") as "success" | "error",
        is_error,
      };
    }
    return e;
  });
}

function deriveAgentState(
  event: AgentEvent,
  current: AgentState,
): AgentState {
  switch (event.type) {
    case "RunStarted":
    case "ModelStarted":
      return "sampling";
    case "TextDelta":
    case "ReasoningDelta":
      return current === "executing_tool" ? current : "sampling";
    case "ToolStarted":
      return "executing_tool";
    case "ToolOutput":
      return "executing_tool";
    case "ToolFinished":
      return "sampling";
    case "ModelFinished":
      return "idle";
    case "RunFinished":
    case "RunCancelled":
      return "idle";
    case "RunFailed":
      return "failed";
    default:
      return current;
  }
}

function toolOutcomeIsError(outcome: ToolOutcome): boolean {
  return "Error" in outcome;
}

function applyLiveEvent(
  state: CrowState,
  event: AgentEvent,
): Partial<CrowState> {
  const updates: Partial<CrowState> = {};
  let entries = state.entries;

  switch (event.type) {
    case "RunStarted":
      entries = [];
      updates.runId = event.run_id;
      break;
    case "TextDelta":
      entries = appendToLastAssistant(entries, event.text);
      break;
    case "ReasoningDelta":
      entries = appendToLastReasoning(entries, event.text);
      break;
    case "ToolStarted":
      entries = [
        ...entries,
        {
          kind: "tool",
          call_id: event.call_id,
          name: event.name,
          args: event.args,
          outputChunks: [],
          is_error: false,
          status: "running",
        },
      ];
      break;
    case "ToolOutput":
      entries = appendToolOutput(
        entries,
        event.call_id,
        decodeChunk(event.chunk),
      );
      break;
    case "ToolFinished": {
      const isError = toolOutcomeIsError(event.result);
      entries = finishTool(entries, event.call_id, isError);
      break;
    }
    case "ModelFinished":
      updates.usage = event.usage;
      break;
    case "RunFinished":
      entries = [
        ...entries,
        { kind: "run_finished", message: event.message },
      ];
      break;
    case "RunCancelled":
      entries = [...entries, { kind: "run_cancelled" }];
      break;
    case "RunFailed":
      entries = [
        ...entries,
        {
          kind: "run_failed",
          code: event.code,
          message: event.message,
          retryable: event.retryable,
        },
      ];
      break;
    default:
      break;
  }

  updates.entries = entries;
  updates.agentState = deriveAgentState(event, state.agentState);
  return updates;
}

function applyReplayEvents(
  _state: CrowState,
  events: ReplayEvent[],
): Partial<CrowState> {
  const entries: StreamEntry[] = [];

  for (const ev of events) {
    switch (ev.kind) {
      case "session_started":
        break;
      case "user_message":
        entries.push({ kind: "user", id: ev.id, content: ev.content });
        break;
      case "assistant_message": {
        let text = "";
        let reasoningText = "";
        for (const part of ev.parts) {
          if (part.kind === "Text") {
            text += part.text;
          } else if (part.kind === "Reasoning") {
            reasoningText += part.text;
          }
        }
        if (reasoningText) {
          entries.push({
            kind: "reasoning",
            id: nextId(),
            text: reasoningText,
          });
        }
        if (text) {
          entries.push({
            kind: "assistant",
            id: ev.id,
            text,
          });
        }
        break;
      }
      case "tool_started":
        entries.push({
          kind: "tool",
          call_id: ev.call_id,
          name: ev.name,
          args: ev.args,
          outputChunks: [],
          is_error: false,
          status: "running",
        });
        break;
      case "tool_finished": {
        const isError = toolOutcomeIsError(ev.outcome);
        let idx = -1;
        for (let i = entries.length - 1; i >= 0; i--) {
          const e = entries[i];
          if (e.kind === "tool" && e.call_id === ev.call_id) {
            idx = i;
            break;
          }
        }
        if (idx >= 0) {
          const toolEntry = entries[idx];
          if (toolEntry.kind === "tool") {
            entries[idx] = {
              ...toolEntry,
              outputChunks: [outcomeOutput(ev.outcome)],
              status: isError ? "error" : "success",
              is_error: isError,
            };
          }
        }
        break;
      }
      case "run_finished":
        entries.push({ kind: "run_finished", message: ev.message });
        break;
      case "run_interrupted":
        entries.push({ kind: "run_cancelled" });
        break;
      case "run_failed":
        entries.push({
          kind: "run_failed",
          code: ev.code,
          message: ev.message,
          retryable: ev.retryable,
        });
        break;
    }
  }

  return {
    entries,
    agentState: "idle",
  };
}

export const useCrowStore = create<CrowState>((set) => ({
  sessions: [],
  activeSessionId: null,
  activeSessionPath: null,
  runId: null,
  agentState: "idle",
  entries: [],
  usage: { input_tokens: 0, output_tokens: 0 },
  approvalQueue: [],
  selectedEntryIndex: null,

  setSessions: (sessions) => set({ sessions }),

  setActiveSession: (id, path) =>
    set({
      activeSessionId: id,
      activeSessionPath: path,
      entries: [],
      agentState: "idle",
      runId: null,
      usage: { input_tokens: 0, output_tokens: 0 },
      selectedEntryIndex: null,
    }),

  applyLiveEvent: (event) =>
    set((state) => applyLiveEvent(state, event)),

  applyReplayEvents: (events) =>
    set((state) => applyReplayEvents(state, events)),

  addApproval: (req) =>
    set((state) => ({
      approvalQueue: [...state.approvalQueue, req],
    })),

  removeApproval: (askId) =>
    set((state) => ({
      approvalQueue: state.approvalQueue.filter((r) => r.ask_id !== askId),
    })),

  setSelectedEntry: (index) => set({ selectedEntryIndex: index }),

  reset: () =>
    set({
      entries: [],
      agentState: "idle",
      runId: null,
      usage: { input_tokens: 0, output_tokens: 0 },
      approvalQueue: [],
      selectedEntryIndex: null,
    }),
}));
