import type { Dispatch } from "react";

import type { ConnectResult, KnownHost } from "../../shared/api.ts";
import type { Action } from "./state.ts";

export function describeConnectFailure(result: Exclude<ConnectResult, { ok: true }>): string {
  switch (result.kind) {
    case "auth":
      return "Authentication failed (HTTP 401) — check the token from ~/.crow/daemon.json on that host.";
    case "unreachable":
      return `Host unreachable — ${result.message}`;
    default:
      return result.message;
  }
}

/**
 * The one connect path — used for first connect, reconnect after failure,
 * add+connect, and startup auto-connect. Never throws: every failure lands
 * in the fleet entry's error so the UI always shows what happened.
 */
export async function connectHost(host: KnownHost, dispatch: Dispatch<Action>): Promise<void> {
  dispatch({ type: "connect.started", hostName: host.name });
  let result: ConnectResult;
  try {
    result = await window.crow.hostConnect(host);
  } catch (error) {
    result = {
      ok: false,
      kind: "error",
      message: error instanceof Error ? error.message : String(error),
    };
  }
  if (!result.ok) {
    dispatch({
      type: "connect.failed",
      hostName: host.name,
      message: describeConnectFailure(result),
    });
    return;
  }
  dispatch({ type: "connect.succeeded", hostName: host.name, info: result.info });
  const sessions = await window.crow.sessionList(host.name).catch(() => []);
  dispatch({ type: "sessions.set", hostName: host.name, sessions });
}
