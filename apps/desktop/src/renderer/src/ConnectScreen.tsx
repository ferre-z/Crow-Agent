import { useState } from "react";

import type { ConnectResult, KnownHost } from "../../shared/api.ts";
import type { ScreenProps } from "./App.tsx";

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

export default function ConnectScreen({ state, dispatch }: ScreenProps) {
  const [name, setName] = useState("");
  const [url, setUrl] = useState(DEFAULT_URL);
  const [token, setToken] = useState("");

  async function handleConnect(host: KnownHost) {
    dispatch({ type: "connect.started" });
    const result = await window.crow.hostConnect(host);
    if (!result.ok) {
      dispatch({ type: "connect.failed", message: describeConnectFailure(result) });
      return;
    }
    const sessions = await window.crow.sessionList().catch(() => []);
    dispatch({ type: "connect.succeeded", hostName: host.name, info: result.info, sessions });
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

  async function handleRemove(hostName: string) {
    const hosts = await window.crow.hostsRemove(hostName);
    dispatch({ type: "hosts.set", hosts });
  }

  const addDisabled = !name.trim() || !url.trim() || !token.trim();

  return (
    <div className="flex h-full items-center justify-center bg-ink">
      <div className="w-[560px] max-w-full rounded-lg border border-line bg-ink-1 p-8">
        <div className="mb-6">
          <h1 className="text-xl font-semibold text-crow">crow</h1>
          <p className="mt-1 text-sm text-fg-dim">Connect to a daemon host.</p>
        </div>

        {state.connectError ? (
          <div className="mb-4 rounded border border-danger/40 bg-danger/10 px-3 py-2 text-sm text-danger">
            {state.connectError}
          </div>
        ) : null}

        {state.hosts.length > 0 ? (
          <ul className="mb-6 divide-y divide-line rounded border border-line">
            {state.hosts.map((host) => (
              <li key={host.name} className="flex items-center gap-3 px-3 py-2">
                <div className="min-w-0 flex-1">
                  <div className="truncate text-sm font-medium">{host.name}</div>
                  <div className="truncate font-mono text-xs text-fg-dim">{host.url}</div>
                </div>
                <button
                  onClick={() => void handleConnect(host)}
                  disabled={state.connecting}
                  className="rounded bg-crow px-3 py-1 text-xs font-semibold text-ink hover:bg-crow-dim disabled:opacity-50"
                >
                  {state.connecting ? "connecting…" : "connect"}
                </button>
                <button
                  onClick={() => void handleRemove(host.name)}
                  disabled={state.connecting}
                  className="rounded border border-line px-2 py-1 text-xs text-fg-dim hover:text-danger disabled:opacity-50"
                  title={`delete ${host.name}`}
                >
                  ✕
                </button>
              </li>
            ))}
          </ul>
        ) : (
          <p className="mb-6 text-sm text-fg-dim">No saved hosts yet — add one below.</p>
        )}

        <div className="space-y-2">
          <div className="text-xs font-semibold uppercase tracking-wide text-fg-dim">add host</div>
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
