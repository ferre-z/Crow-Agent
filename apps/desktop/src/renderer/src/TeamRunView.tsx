import { useState } from "react";

import type { TeamRun, TeamRunStep } from "./state.ts";

const STEP_DOT: Record<TeamRunStep["state"], string> = {
  running: "bg-crow animate-pulse",
  done: "bg-crow",
  error: "bg-danger",
};

const RUN_BADGE: Record<TeamRun["state"], { label: string; className: string }> = {
  running: { label: "running", className: "text-crow" },
  done: { label: "done", className: "text-crow" },
  error: { label: "error", className: "text-danger" },
};

function StepRow({ step }: { step: TeamRunStep }) {
  const [expanded, setExpanded] = useState(false);
  const body = step.state === "error" ? step.error : step.output;
  return (
    <div className="rounded border border-line bg-ink-1 text-xs">
      <button
        onClick={() => setExpanded((v) => !v)}
        className="flex w-full items-center gap-2 px-2 py-1.5 text-left hover:bg-ink-2"
      >
        <span className={`h-2 w-2 shrink-0 rounded-full ${STEP_DOT[step.state]}`} />
        <span className="text-fg-dim">step {step.step}</span>
        <span className="min-w-0 flex-1 truncate font-semibold">{step.agent}</span>
        <span className={`shrink-0 ${step.state === "error" ? "text-danger" : "text-fg-dim"}`}>
          {step.state}
        </span>
        {body ? <span className="shrink-0 text-fg-dim">{expanded ? "▾" : "▸"}</span> : null}
      </button>
      {expanded && body ? (
        <pre
          className={`max-h-60 overflow-auto border-t border-line px-2 py-1 whitespace-pre-wrap ${
            step.state === "error" ? "text-danger" : "text-fg"
          }`}
        >
          {body}
        </pre>
      ) : null}
    </div>
  );
}

export default function TeamRunView({ run }: { run: TeamRun }) {
  const badge = RUN_BADGE[run.state];
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex items-center gap-2 border-b border-line px-4 py-2">
        <span className="text-sm font-semibold text-crow">{run.team}</span>
        <span className="text-xs text-fg-dim">{run.hostName}</span>
        <span className={`text-xs ${badge.className}`}>{badge.label}</span>
      </div>
      <div className="min-h-0 flex-1 space-y-2 overflow-y-auto p-4">
        <div className="rounded border border-line bg-ink-1 px-2 py-1.5 text-xs">
          <div className="mb-0.5 text-fg-dim">input</div>
          <div className="whitespace-pre-wrap">{run.input}</div>
        </div>

        {run.steps.length === 0 && run.state === "running" ? (
          <p className="text-xs text-fg-dim">Waiting for the first step…</p>
        ) : null}
        {run.steps.map((step) => (
          <StepRow key={step.step} step={step} />
        ))}

        {run.state === "error" ? (
          <div className="rounded border border-danger/60 bg-ink-1 px-2 py-1.5 text-xs text-danger">
            {run.error ?? "team run failed"}
          </div>
        ) : null}

        {run.state === "done" ? (
          <div className="rounded border border-line bg-ink-1 px-2 py-1.5 text-xs">
            <div className="mb-0.5 font-semibold text-crow">final output</div>
            <pre className="max-h-96 overflow-auto whitespace-pre-wrap">
              {run.output || "(no output)"}
            </pre>
          </div>
        ) : null}
      </div>
    </div>
  );
}
