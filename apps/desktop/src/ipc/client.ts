import { invoke, Channel } from "@tauri-apps/api/core";
import type {
  AgentEvent,
  AskNotification,
  SessionId,
  SessionStartResult,
  SessionListResult,
  SessionLoadResult,
  SubmitResult,
} from "./events";

export async function initialize(): Promise<{ protocol_version: number }> {
  return invoke("initialize");
}

export async function sessionStart(
  projectRoot: string,
): Promise<SessionStartResult> {
  return invoke("session_start", { projectRoot });
}

export async function sessionList(
  projectRoot: string,
): Promise<SessionListResult> {
  return invoke("session_list", { projectRoot });
}

export async function sessionLoad(
  sessionId: SessionId,
  path: string,
): Promise<SessionLoadResult> {
  return invoke("session_load", { sessionId, path });
}

export async function submit(
  sessionId: SessionId,
  path: string,
  userMessage: string,
  eventChannel: Channel<AgentEvent>,
  askChannel: Channel<AskNotification>,
): Promise<SubmitResult> {
  return invoke("submit", {
    sessionId,
    path,
    userMessage,
    eventChannel,
    askChannel,
  });
}

export async function interrupt(
  sessionId: SessionId,
): Promise<{ cancelled: boolean }> {
  return invoke("interrupt", { sessionId });
}

export async function askResolve(
  askId: string,
  decision: "allow" | "deny",
): Promise<void> {
  return invoke("ask_resolve", { askId, decision });
}

export async function setProjectRoot(root: string): Promise<void> {
  return invoke("set_project_root", { root });
}
