import type { Dispatch } from "react";

import type { ApprovalDecision } from "../../shared/api.ts";
import type { Action, PendingApproval } from "./state.ts";

function pretty(value: unknown): string {
  try {
    return JSON.stringify(value, null, 2) ?? String(value);
  } catch {
    return String(value);
  }
}

const BUTTONS: { decision: ApprovalDecision; label: string; className: string }[] = [
  { decision: "allow", label: "allow", className: "bg-crow text-ink hover:bg-crow-dim" },
  {
    decision: "always",
    label: "always",
    className: "border border-crow text-crow hover:bg-crow/10",
  },
  {
    decision: "deny",
    label: "deny",
    className: "border border-danger text-danger hover:bg-danger/10",
  },
];

export default function ApprovalModal({
  approval,
  dispatch,
}: {
  approval: PendingApproval;
  dispatch: Dispatch<Action>;
}) {
  function respond(decision: ApprovalDecision) {
    void window.crow
      .approvalRespond({ approvalId: approval.approvalId, decision })
      .catch(() => undefined);
    dispatch({ type: "approval.responded", approvalId: approval.approvalId, decision });
  }

  return (
    <div className="fixed inset-0 z-10 flex items-center justify-center bg-black/60">
      <div className="w-[520px] max-w-full rounded-lg border border-warn/50 bg-ink-1 p-5 shadow-2xl">
        <h2 className="text-sm font-semibold text-warn">tool approval requested</h2>
        <p className="mt-1 text-xs text-fg-dim">
          The agent wants to run <span className="font-mono text-fg">{approval.tool}</span>
        </p>
        <pre className="mt-3 max-h-64 overflow-auto rounded border border-line bg-ink p-2 font-mono text-xs text-fg-dim">
          {pretty(approval.args)}
        </pre>
        <div className="mt-4 flex justify-end gap-2">
          {BUTTONS.map(({ decision, label, className }) => (
            <button
              key={decision}
              onClick={() => respond(decision)}
              className={`rounded px-4 py-1.5 text-sm font-semibold ${className}`}
            >
              {label}
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}
