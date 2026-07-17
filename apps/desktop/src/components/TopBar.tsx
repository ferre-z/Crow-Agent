import { useCrowStore } from "../store";
import HeartbeatOrb from "./HeartbeatOrb";

export default function TopBar() {
  const agentState = useCrowStore((s) => s.agentState);
  const usage = useCrowStore((s) => s.usage);
  const activeSessionId = useCrowStore((s) => s.activeSessionId);

  const totalTokens = usage.input_tokens + usage.output_tokens;
  const formatted = totalTokens > 1000
    ? `${(totalTokens / 1000).toFixed(1)}k`
    : `${totalTokens}`;

  return (
    <div className="clay flex h-14 items-center justify-between px-5">
      <div className="flex items-center gap-3">
        <span className="font-display text-lg font-semibold text-mist">
          ◐ Crow
        </span>
        <HeartbeatOrb state={agentState} />
        <span className="text-xs text-fog capitalize">
          {agentState.replace("_", " ")}
        </span>
      </div>

      <div className="flex items-center gap-4">
        {activeSessionId && (
          <span className="font-mono text-xs text-fog">
            ⧉ {formatted} tokens
          </span>
        )}
      </div>
    </div>
  );
}
