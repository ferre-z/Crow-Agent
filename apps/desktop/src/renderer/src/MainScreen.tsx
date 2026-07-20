import { useEffect, useMemo, useState } from "react";

import type { ScreenProps } from "./App.tsx";
import ApprovalModal from "./ApprovalModal.tsx";
import ChatView from "./ChatView.tsx";
import TeamRunModal from "./TeamRunModal.tsx";
import TeamRunView from "./TeamRunView.tsx";
import { connectHost } from "./connect-host.ts";
import {
  makeRunKey,
  makeSessionKey,
  selectActiveSession,
  selectActiveTeamRun,
  selectAgentRuns,
  selectConnectedHosts,
  selectCurrentApproval,
  selectSessions,
  selectTeamRuns,
  sessionDisplayName,
  type AgentRun,
  type LiveSessionState,
  type SessionEntry,
  type TeamRun,
} from "./state.ts";

const DOT_CLASS: Record<LiveSessionState, string> = {
  idle: "bg-crow",
  streaming: "bg-crow animate-pulse",
  error: "bg-danger",
  cancelled: "bg-warn",
};

const AGENT_DOT_CLASS: Record<AgentRun["state"], string> = {
  started: "bg-crow animate-pulse",
  done: "bg-crow",
  error: "bg-danger",
};

const TEAM_DOT_CLASS: Record<TeamRun["state"], string> = {
  running: "bg-crow animate-pulse",
  done: "bg-crow",
  error: "bg-danger",
};

function AgentRunRow({ run }: { run: AgentRun }) {
  const [expanded, setExpanded] = useState(false);
  const body = run.state === "error" ? run.error : run.output;
  return (
    <div>
      <button
        onClick={() => setExpanded((v) => !v)}
        className="flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-xs hover:bg-ink-2"
      >
        <span
          className={`h-2 w-2 shrink-0 rounded-full ${AGENT_DOT_CLASS[run.state]}`}
          title={run.state}
        />
        <span className="min-w-0 flex-1 truncate">{run.prompt}</span>
        <span
          className={`shrink-0 text-[10px] ${run.state === "error" ? "text-danger" : "text-fg-dim"}`}
        >
          {run.state}
        </span>
      </button>
      {expanded && body ? (
        <pre
          className={`mx-2 mb-1 max-h-40 overflow-auto rounded border border-line bg-ink px-2 py-1 text-[11px] whitespace-pre-wrap ${
            run.state === "error" ? "text-danger" : "text-fg"
          }`}
        >
          {body}
        </pre>
      ) : null}
    </div>
  );
}

function TeamRunRow({
  run,
  active,
  onSelect,
}: {
  run: TeamRun;
  active: boolean;
  onSelect: (hostName: string, runId: string) => void;
}) {
  return (
    <button
      onClick={() => onSelect(run.hostName, run.runId)}
      className={`flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-xs hover:bg-ink-2 ${
        active ? "bg-ink-2 text-crow" : ""
      }`}
    >
      <span
        className={`h-2 w-2 shrink-0 rounded-full ${TEAM_DOT_CLASS[run.state]}`}
        title={run.state}
      />
      <span className="min-w-0 flex-1 truncate">
        {run.hostName}: {run.team}
      </span>
      <span
        className={`shrink-0 text-[10px] ${run.state === "error" ? "text-danger" : "text-fg-dim"}`}
      >
        {run.state}
      </span>
    </button>
  );
}

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

export default function MainScreen({
  state,
  dispatch,
  onManageHosts,
}: ScreenProps & { onManageHosts?: () => void }) {
  const allHosts = selectConnectedHosts(state);
  const connectedHosts = allHosts.filter((h) => h.state === "connected");
  const disconnectedHosts = allHosts.filter((h) => h.state !== "connected");
  const sessions = selectSessions(state);
  const active = selectActiveSession(state);
  const activeTeamRun = selectActiveTeamRun(state);
  const teamRuns = selectTeamRuns(state);
  const agentRuns = selectAgentRuns(state);
  const currentApproval = selectCurrentApproval(state);

  const [cwd, setCwd] = useState("~");
  const [approvalMode, setApprovalMode] = useState<"auto" | "ask">("ask");
  const [autoApproveTools, setAutoApproveTools] = useState("");
  const [showTeamModal, setShowTeamModal] = useState(false);
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

  function handleSelectTeamRun(hostName: string, runId: string) {
    dispatch({ type: "team.selected", hostName, runId });
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
            <div className="flex items-center gap-1.5">
              <button
                onClick={() => setShowTeamModal(true)}
                disabled={connectedHosts.length === 0}
                className="rounded border border-line px-2 py-0.5 text-xs text-fg-dim hover:text-crow disabled:opacity-50"
              >
                run team
              </button>
              <button
                onClick={onManageHosts}
                className="rounded border border-line px-2 py-0.5 text-xs text-fg-dim hover:text-crow"
              >
                hosts ({connectedHosts.length}/{allHosts.length})
              </button>
            </div>
          </div>
        </div>

        {disconnectedHosts.length > 0 ? (
          <div className="border-b border-line p-2">
            {disconnectedHosts.map((host) => (
              <div key={host.host.name} className="mb-1 rounded border border-line px-2 py-1.5">
                <div className="flex items-center gap-2">
                  <span
                    className={`h-2 w-2 shrink-0 rounded-full ${host.error ? "bg-danger" : "bg-fg-dim/40"}`}
                  />
                  <span className="min-w-0 flex-1 truncate text-xs">{host.host.name}</span>
                  <button
                    onClick={() => void connectHost(host.host, dispatch)}
                    disabled={host.connecting === true}
                    className="rounded bg-crow px-2 py-0.5 text-xs font-semibold text-ink hover:bg-crow-dim disabled:opacity-50"
                  >
                    {host.connecting ? "…" : "connect"}
                  </button>
                </div>
                {host.error ? (
                  <div className="mt-1 text-[11px] text-danger">{host.error}</div>
                ) : null}
              </div>
            ))}
          </div>
        ) : null}

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
                                state.activeView?.kind === "session" &&
                                makeSessionKey(session.hostName, session.info.id) ===
                                  state.activeView.key
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

        {teamRuns.length > 0 || agentRuns.length > 0 ? (
          <div className="max-h-64 shrink-0 overflow-y-auto border-t border-line p-2">
            <div className="mb-1 px-1 text-xs font-semibold uppercase tracking-wide text-fg-dim">
              runs
            </div>
            <ul className="space-y-0.5">
              {teamRuns.map((run) => (
                <li key={makeRunKey(run.hostName, run.runId)}>
                  <TeamRunRow
                    run={run}
                    active={
                      state.activeView?.kind === "team" &&
                      state.activeView.runId === makeRunKey(run.hostName, run.runId)
                    }
                    onSelect={handleSelectTeamRun}
                  />
                </li>
              ))}
              {agentRuns.map((run) => (
                <li key={makeRunKey(run.hostName, run.agentId)}>
                  <AgentRunRow run={run} />
                </li>
              ))}
            </ul>
          </div>
        ) : null}
      </aside>

      <main className="flex min-w-0 flex-1 flex-col">
        {activeTeamRun ? (
          <TeamRunView run={activeTeamRun} />
        ) : active ? (
          <ChatView session={active} dispatch={dispatch} />
        ) : (
          <div className="flex flex-1 items-center justify-center text-sm text-fg-dim">
            Select a session or a run, or create a new one.
          </div>
        )}
      </main>

      {showTeamModal ? (
        <TeamRunModal state={state} dispatch={dispatch} onClose={() => setShowTeamModal(false)} />
      ) : null}

      {currentApproval ? <ApprovalModal approval={currentApproval} dispatch={dispatch} /> : null}
    </div>
  );
}
