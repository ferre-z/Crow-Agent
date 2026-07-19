import { useEffect, useMemo, useState } from "react";

import type { ScreenProps } from "./App.tsx";
import ApprovalModal from "./ApprovalModal.tsx";
import ChatView from "./ChatView.tsx";
import {
  makeSessionKey,
  selectActiveSession,
  selectConnectedHosts,
  selectCurrentApproval,
  selectSessions,
  sessionDisplayName,
  type LiveSessionState,
  type SessionEntry,
} from "./state.ts";

const DOT_CLASS: Record<LiveSessionState, string> = {
  idle: "bg-crow",
  streaming: "bg-crow animate-pulse",
  error: "bg-danger",
  cancelled: "bg-warn",
};

function SessionRow({
  session,
  active,
  onSelect,
}: {
  session: SessionEntry;
  active: boolean;
  onSelect: (hostName: string, sessionId: string) => void;
}) {
  return (
    <button
      onClick={() => onSelect(session.hostName, session.info.id)}
      className={`flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-xs hover:bg-ink-2 ${
        active ? "bg-ink-2 text-crow" : ""
      }`}
    >
      <span
        className={`h-2 w-2 shrink-0 rounded-full ${DOT_CLASS[session.live]}`}
        title={session.live}
      />
      <span className="min-w-0 flex-1 truncate">{sessionDisplayName(session)}</span>
      <span className="shrink-0 text-[10px] text-fg-dim">{session.live}</span>
    </button>
  );
}

export default function MainScreen({ state, dispatch }: ScreenProps) {
  const connectedHosts = selectConnectedHosts(state).filter((h) => h.state === "connected");
  const sessions = selectSessions(state);
  const active = selectActiveSession(state);
  const currentApproval = selectCurrentApproval(state);

  const [cwd, setCwd] = useState("~");
  const [approvalMode, setApprovalMode] = useState<"auto" | "ask">("ask");
  const [autoApproveTools, setAutoApproveTools] = useState("");
  const [selectedHost, setSelectedHost] = useState<string>(connectedHosts[0]?.host.name ?? "");

  // Keep the selected host valid if the fleet changes.
  const hostOptions = useMemo(() => connectedHosts.map((h) => h.host.name), [connectedHosts]);
  const canCreate = hostOptions.length > 0;
  useEffect(() => {
    if (!hostOptions.includes(selectedHost)) {
      setSelectedHost(hostOptions[0] ?? "");
    }
  }, [hostOptions, selectedHost]);

  async function handleCreate() {
    if (!selectedHost) return;
    const target = cwd.trim() || "~";
    const tools = autoApproveTools
      .split(",")
      .map((t) => t.trim())
      .filter(Boolean);
    const { sessionId } = await window.crow.sessionCreate({
      hostName: selectedHost,
      cwd: target,
      approvalMode,
      ...(tools.length > 0 ? { autoApproveTools: tools } : {}),
    });
    dispatch({
      type: "session.created",
      hostName: selectedHost,
      info: {
        id: sessionId,
        cwd: target,
        model: null,
        state: "idle",
        createdAt: new Date().toISOString(),
        approvalMode,
      },
    });
  }

  function handleSelect(hostName: string, sessionId: string) {
    dispatch({ type: "session.selected", hostName, sessionId });
    void window.crow.sessionAttach({ hostName, sessionId }).catch(() => undefined);
  }

  const sessionsByHost = useMemo(() => {
    const map = new Map<string, SessionEntry[]>();
    for (const host of connectedHosts) {
      map.set(host.host.name, []);
    }
    for (const session of sessions) {
      const list = map.get(session.hostName);
      if (list) list.push(session);
    }
    return map;
  }, [connectedHosts, sessions]);

  return (
    <div className="flex h-full bg-ink">
      <aside className="flex w-80 shrink-0 flex-col border-r border-line bg-ink-1">
        <div className="border-b border-line p-3">
          <div className="flex items-center justify-between">
            <span className="text-sm font-semibold text-crow">crow</span>
            <span className="text-xs text-fg-dim">{connectedHosts.length} host(s) connected</span>
          </div>
        </div>

        <div className="border-b border-line p-3">
          <div className="mb-1.5 text-xs font-semibold uppercase tracking-wide text-fg-dim">
            new session
          </div>
          <select
            value={selectedHost}
            onChange={(e) => setSelectedHost(e.target.value)}
            disabled={!canCreate}
            className="mb-1.5 w-full rounded border border-line bg-ink px-2 py-1 text-xs outline-none focus:border-crow disabled:opacity-50"
            title="host"
          >
            {hostOptions.length === 0 ? <option value="">no connected hosts</option> : null}
            {hostOptions.map((name) => (
              <option key={name} value={name}>
                {name}
              </option>
            ))}
          </select>
          <input
            value={cwd}
            onChange={(e) => setCwd(e.target.value)}
            placeholder="working directory (~)"
            className="mb-1.5 w-full rounded border border-line bg-ink px-2 py-1 font-mono text-xs outline-none focus:border-crow"
          />
          <div className="mb-1.5 flex gap-1.5">
            <select
              value={approvalMode}
              onChange={(e) => setApprovalMode(e.target.value === "auto" ? "auto" : "ask")}
              className="rounded border border-line bg-ink px-2 py-1 text-xs outline-none focus:border-crow"
              title="approval mode"
            >
              <option value="ask">ask approvals</option>
              <option value="auto">auto approvals</option>
            </select>
            <input
              value={autoApproveTools}
              onChange={(e) => setAutoApproveTools(e.target.value)}
              placeholder="auto-approve tools (csv)"
              className="min-w-0 flex-1 rounded border border-line bg-ink px-2 py-1 font-mono text-xs outline-none focus:border-crow"
            />
          </div>
          <button
            onClick={() => void handleCreate()}
            disabled={!canCreate}
            className="w-full rounded bg-crow px-2 py-1 text-xs font-semibold text-ink hover:bg-crow-dim disabled:opacity-50"
          >
            + new session
          </button>
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto p-2">
          {connectedHosts.length === 0 ? (
            <p className="px-1 py-2 text-xs text-fg-dim">No hosts connected.</p>
          ) : (
            <div className="space-y-3">
              {connectedHosts.map((host) => {
                const hostSessions = sessionsByHost.get(host.host.name) ?? [];
                return (
                  <div key={host.host.name}>
                    <div className="mb-1 flex items-center gap-2 px-1 text-xs font-semibold text-fg-dim">
                      <span
                        className={`h-2 w-2 rounded-full ${
                          host.state === "connected" ? "bg-crow" : "bg-fg-dim/40"
                        }`}
                      />
                      {host.host.name}
                    </div>
                    {hostSessions.length === 0 ? (
                      <p className="px-1 py-1 text-xs text-fg-dim">No sessions on this host yet.</p>
                    ) : (
                      <ul className="space-y-0.5">
                        {hostSessions.map((session) => (
                          <li key={makeSessionKey(session.hostName, session.info.id)}>
                            <SessionRow
                              session={session}
                              active={
                                makeSessionKey(session.hostName, session.info.id) ===
                                state.activeSessionKey
                              }
                              onSelect={handleSelect}
                            />
                          </li>
                        ))}
                      </ul>
                    )}
                  </div>
                );
              })}
            </div>
          )}
        </div>
      </aside>

      <main className="flex min-w-0 flex-1 flex-col">
        {active ? (
          <ChatView session={active} dispatch={dispatch} />
        ) : (
          <div className="flex flex-1 items-center justify-center text-sm text-fg-dim">
            Select a session, or create a new one.
          </div>
        )}
      </main>

      {currentApproval ? <ApprovalModal approval={currentApproval} dispatch={dispatch} /> : null}
    </div>
  );
}
