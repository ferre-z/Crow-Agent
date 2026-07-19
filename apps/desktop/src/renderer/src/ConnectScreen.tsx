import { useState } from "react";

import type { ConnectResult, KnownHost } from "../../shared/api.ts";
import type { ScreenProps } from "./App.tsx";
import type { FleetHostEntry } from "./state.ts";

const DEFAULT_URL = "ws://127.0.0.1:7749";

function describeConnectFailure(result: Exclude<ConnectResult, { ok: true }>): string {
  switch (result.kind) {
    case "auth":
      return "Authentication failed (HTTP 401) — check the token from ~/.crow/daemon.json on that host.";
    case "unreachable":
      return `Host unreachable — ${result.message}`;
    default:
      return result.message;
  }
}

function fleetEntryOrDefault(state: ScreenProps["state"], hostName: string): FleetHostEntry {
  return (
    state.fleet[hostName] ?? {
      host: state.hosts.find((h) => h.name === hostName) ?? { name: hostName, url: "", token: "" },
      state: "disconnected",
    }
  );
}

export default function ConnectScreen({ state, dispatch }: ScreenProps) {
  const [name, setName] = useState("");
  const [url, setUrl] = useState(DEFAULT_URL);
  const [token, setToken] = useState("");

  async function handleConnect(host: KnownHost) {
    dispatch({ type: "connect.started", hostName: host.name });
    const result = await window.crow.hostConnect(host);
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

  async function handleReconnect(hostName: string) {
    dispatch({ type: "connect.started", hostName });
    const result = await window.crow.hostReconnect(hostName);
    if (!result.ok) {
      dispatch({ type: "connect.failed", hostName, message: describeConnectFailure(result) });
      return;
    }
    dispatch({ type: "connect.succeeded", hostName, info: result.info });
    const sessions = await window.crow.sessionList(hostName).catch(() => []);
    dispatch({ type: "sessions.set", hostName, sessions });
  }

  async function handleDisconnect(hostName: string) {
    await window.crow.hostDisconnect(hostName).catch(() => undefined);
    dispatch({ type: "host.disconnect", hostName });
  }

  async function handleRemove(hostName: string) {
    const hosts = await window.crow.hostsRemove(hostName);
    dispatch({ type: "hosts.set", hosts });
    dispatch({ type: "host.remove", hostName });
  }

  async function handleAdd() {
    const host = { name: name.trim(), url: url.trim(), token: token.trim() };
    if (!host.name || !host.url || !host.token) return;
    const hosts = await window.crow.hostsAdd(host);
    dispatch({ type: "hosts.set", hosts });
    setName("");
    setUrl(DEFAULT_URL);
    setToken("");
  }

  async function handleRefresh() {
    const hosts = await window.crow.hostsList().catch(() => []);
    dispatch({ type: "hosts.set", hosts });
  }

  const addDisabled = !name.trim() || !url.trim() || !token.trim();

  return (
    <div className="flex h-full items-center justify-center bg-ink">
      <div className="w-[620px] max-w-full rounded-lg border border-line bg-ink-1 p-8">
        <div className="mb-6">
          <h1 className="text-xl font-semibold text-crow">crow</h1>
          <p className="mt-1 text-sm text-fg-dim">Manage daemon hosts.</p>
        </div>

        {state.hosts.length > 0 ? (
          <ul className="mb-6 divide-y divide-line rounded border border-line">
            {state.hosts.map((host) => {
              const entry = fleetEntryOrDefault(state, host.name);
              const isConnected = entry.state === "connected";
              const isConnecting = entry.connecting === true;
              return (
                <li key={host.name} className="px-3 py-2">
                  <div className="flex items-center gap-3">
                    <span
                      className={`h-2.5 w-2.5 shrink-0 rounded-full ${
                        isConnected ? "bg-crow" : entry.error ? "bg-danger" : "bg-fg-dim/40"
                      }`}
                      title={entry.state}
                    />
                    <div className="min-w-0 flex-1">
                      <div className="truncate text-sm font-medium">{host.name}</div>
                      <div className="truncate font-mono text-xs text-fg-dim">{host.url}</div>
                    </div>
                    <div className="flex shrink-0 gap-1.5">
                      {!isConnected ? (
                        <button
                          onClick={() => void handleConnect(host)}
                          disabled={isConnecting}
                          className="rounded bg-crow px-2.5 py-1 text-xs font-semibold text-ink hover:bg-crow-dim disabled:opacity-50"
                        >
                          {isConnecting ? "…" : "connect"}
                        </button>
                      ) : null}
                      <button
                        onClick={() => void handleReconnect(host.name)}
                        disabled={isConnecting}
                        className="rounded border border-line px-2 py-1 text-xs text-fg-dim hover:text-crow disabled:opacity-50"
                      >
                        reconnect
                      </button>
                      {isConnected ? (
                        <button
                          onClick={() => void handleDisconnect(host.name)}
                          className="rounded border border-line px-2 py-1 text-xs text-fg-dim hover:text-danger"
                        >
                          disconnect
                        </button>
                      ) : null}
                      <button
                        onClick={() => void handleRemove(host.name)}
                        disabled={isConnecting}
                        className="rounded border border-line px-2 py-1 text-xs text-fg-dim hover:text-danger disabled:opacity-50"
                        title={`delete ${host.name}`}
                      >
                        ✕
                      </button>
                    </div>
                  </div>
                  {entry.error ? (
                    <div className="mt-1.5 rounded border border-danger/40 bg-danger/10 px-2 py-1 text-xs text-danger">
                      {entry.error}
                    </div>
                  ) : null}
                </li>
              );
            })}
          </ul>
        ) : (
          <p className="mb-6 text-sm text-fg-dim">No saved hosts yet — add one below.</p>
        )}

        <div className="space-y-2">
          <div className="flex items-center justify-between">
            <div className="text-xs font-semibold uppercase tracking-wide text-fg-dim">
              add host
            </div>
            <button
              onClick={() => void handleRefresh()}
              className="text-xs text-fg-dim hover:text-crow"
            >
              refresh list
            </button>
          </div>
          <input
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="name (e.g. local, pi-5)"
            className="w-full rounded border border-line bg-ink px-3 py-1.5 text-sm outline-none focus:border-crow"
          />
          <input
            value={url}
            onChange={(e) => setUrl(e.target.value)}
            placeholder={DEFAULT_URL}
            className="w-full rounded border border-line bg-ink px-3 py-1.5 font-mono text-sm outline-none focus:border-crow"
          />
          <input
            value={token}
            onChange={(e) => setToken(e.target.value)}
            placeholder="daemon token"
            type="password"
            className="w-full rounded border border-line bg-ink px-3 py-1.5 font-mono text-sm outline-none focus:border-crow"
          />
          <button
            onClick={() => void handleAdd()}
            disabled={addDisabled}
            className="rounded border border-crow px-3 py-1.5 text-sm text-crow hover:bg-crow/10 disabled:opacity-50"
          >
            add host
          </button>
        </div>
      </div>
    </div>
  );
}
