export type SessionId = string;
export type RunId = string;
export type MessageId = string;
export type ToolCallId = string;

export type Timestamp = number;

export interface Usage {
  input_tokens: number;
  output_tokens: number;
}

export type StopReason =
  | "EndTurn"
  | "ToolUse"
  | "MaxTokens"
  | "Cancellation"
  | "Error";

export type ToolOutcome =
  | { Success: { output: string; truncated: boolean } }
  | { Error: { code: string; message: string; truncated: boolean } };

export type ToolStream = "stdout" | "stderr";

export type AgentEvent =
  | {
      type: "RunStarted";
      run_id: RunId;
      session_id: SessionId;
      started_at: Timestamp;
    }
  | { type: "ModelStarted" }
  | { type: "TextDelta"; text: string }
  | { type: "ReasoningDelta"; text: string }
  | {
      type: "ToolStarted";
      call_id: ToolCallId;
      name: string;
      args: unknown;
    }
  | {
      type: "ToolOutput";
      call_id: ToolCallId;
      stream: ToolStream;
      chunk: number[];
    }
  | {
      type: "ToolFinished";
      call_id: ToolCallId;
      result: ToolOutcome;
    }
  | {
      type: "ModelFinished";
      usage: Usage;
      stop_reason: StopReason;
    }
  | { type: "RunFinished"; message: string }
  | { type: "RunCancelled" }
  | {
      type: "RunFailed";
      code: string;
      retryable: boolean;
      message: string;
    };

export type Part =
  | { kind: "Text"; text: string }
  | { kind: "Reasoning"; text: string }
  | {
      kind: "ToolCall";
      id: ToolCallId;
      name: string;
      args: unknown;
    }
  | {
      kind: "ToolResult";
      call_id: ToolCallId;
      output: string;
      is_error: boolean;
      truncated: boolean;
      display: DisplayDetails | null;
    };

export interface DisplayDetails {
  path: string | null;
  line_count: number | null;
  byte_size: number | null;
}

export type Role = "user" | "assistant" | "toolresult";

export interface Message {
  id: MessageId;
  role: Role;
  parts: Part[];
}

export type ReplayEvent =
  | { kind: "session_started" }
  | { kind: "user_message"; id: MessageId; content: string }
  | { kind: "assistant_message"; id: MessageId; parts: Part[] }
  | {
      kind: "tool_started";
      call_id: ToolCallId;
      name: string;
      args: unknown;
    }
  | {
      kind: "tool_finished";
      call_id: ToolCallId;
      outcome: ToolOutcome;
    }
  | { kind: "run_finished"; message: string }
  | { kind: "run_interrupted"; active_call: ToolCallId | null }
  | {
      kind: "run_failed";
      code: string;
      retryable: boolean;
      message: string;
    };

export interface EventEnvelope {
  session_id: SessionId;
  run_id: RunId;
  seq: number;
  event: AgentEvent;
}

export interface AskNotification {
  ask_id: string;
  call: { name: string; args: unknown };
}

export interface SessionStartResult {
  session_id: SessionId;
  path: string;
}

export interface SessionListResult {
  sessions: Array<{
    session_id: SessionId;
    started_at: Timestamp;
    schema_version: number;
    path: string;
  }>;
}

export interface SessionLoadResult {
  session_id: SessionId;
  events: ReplayEvent[];
}

export interface SubmitResult {
  run_id: RunId;
  session_id: SessionId;
}
