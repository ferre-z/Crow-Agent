import { useState } from "react";

import type { ScreenProps } from "./App.tsx";
import ApprovalModal from "./ApprovalModal.tsx";
import ChatView from "./ChatView.tsx";
import {
  selectActiveSession,
  selectCurrentApproval,
  selectSessions,
  sessionDisplayName,
  type LiveSessionState,
} from "./state.ts";

const DOT_CLASS: Record<LiveSessionState, string> = {
  idle: "bg-crow",
  streaming: "bg-crow animate-pulse",
  error: "bg-danger",
  cancelled: "bg-warn",
};

export default function MainScreen({ state, dispatch }: ScreenProps) {
  const [cwd, setCwd] = useState("~");
  const [approvalMode, setApprovalMode] = useState<"auto" | "ask">("ask");
  const [autoApproveTools, setAutoApproveTools] = useState("");

  const sessions = selectSessions(state);
  const active = selectActiveSession(state);
  const currentApproval = selectCurrentApproval(state);

  async function handleDisconnect() {
    await window.crow.hostDisconnect().catch(() => undefined);
    dispatch({ type: "disconnect.requested" });
  }

  async function handleCreate() {
    const target = cwd.trim() || "~";
    const tools = autoApproveTools
      .split(",")
      .map((t) => t.trim())
      .filter(Boolean);
    const { sessionId } = await window.crow.sessionCreate({
      cwd: target,
      approvalMode,
      ...(tools.length > 0 ? { autoApproveTools: tools } : {}),
    });
    dispatch({
      type: "session.created",
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

  function handleSelect(sessionId: string) {
    dispatch({ type: "session.selected", sessionId });
    // Attach is idempotent; the creator is auto-attached, others join live.
    void window.crow.sessionAttach(sessionId).catch(() => undefined);
  }

  return (
    <div className="flex h-full bg-ink">
      <aside className="flex w-72 shrink-0 flex-col border-r border-line bg-ink-1">
        <div className="border-b border-line p-3">
          <div className="flex items-center justify-between">
            <span className="text-sm font-semibold text-crow">crow</span>
            <button
              onClick={() => void handleDisconnect()}
              className="rounded border border-line px-2 py-0.5 text-xs text-fg-dim hover:text-danger"
            >
              disconnect
            </button>
          </div>
          {state.hostInfo ? (
            <dl className="mt-2 space-y-0.5 text-xs text-fg-dim">
              <div className="flex justify-between">
                <dt>host</dt>
                <dd className="font-mono text-fg">{state.hostInfo.hostname}</dd>
              </div>
              <div className="flex justify-between">
                <dt>platform</dt>
                <dd className="font-mono">
                  {state.hostInfo.platform}/{state.hostInfo.arch}
                </dd>
              </div>
              <div className="flex justify-between">
                <dt>daemon</dt>
                <dd className="font-mono">
                  v{state.hostInfo.daemonVersion} · proto {state.hostInfo.protocolVersion}
                </dd>
              </div>
            </dl>
          ) : null}
        </div>

        <div className="border-b border-line p-3">
          <div className="mb-1.5 text-xs font-semibold uppercase tracking-wide text-fg-dim">
            new session
          </div>
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
            className="w-full rounded bg-crow px-2 py-1 text-xs font-semibold text-ink hover:bg-crow-dim"
          >
            + new session
          </button>
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto p-2">
          {sessions.length === 0 ? (
            <p className="px-1 py-2 text-xs text-fg-dim">No sessions on this host yet.</p>
          ) : (
            <ul className="space-y-0.5">
              {sessions.map((session) => (
                <li key={session.info.id}>
                  <button
                    onClick={() => handleSelect(session.info.id)}
                    className={`flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-xs hover:bg-ink-2 ${
                      active?.info.id === session.info.id ? "bg-ink-2 text-crow" : ""
                    }`}
                  >
                    <span
                      className={`h-2 w-2 shrink-0 rounded-full ${DOT_CLASS[session.live]}`}
                      title={session.live}
                    />
                    <span className="min-w-0 flex-1 truncate">{sessionDisplayName(session)}</span>
                    <span className="shrink-0 text-[10px] text-fg-dim">{session.live}</span>
                  </button>
                </li>
              ))}
            </ul>
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
