import { useEffect, useMemo, useState, type Dispatch } from "react";

import type { TeamInfo } from "../../shared/api.ts";
import { selectConnectedHosts, type Action, type AppState } from "./state.ts";

export default function TeamRunModal({
  state,
  dispatch,
  onClose,
}: {
  state: AppState;
  dispatch: Dispatch<Action>;
  onClose: () => void;
}) {
  const connectedHosts = selectConnectedHosts(state).filter((h) => h.state === "connected");
  const hostOptions = useMemo(() => connectedHosts.map((h) => h.host.name), [connectedHosts]);

  const [hostName, setHostName] = useState(hostOptions[0] ?? "");
  const [teams, setTeams] = useState<TeamInfo[]>([]);
  const [teamsError, setTeamsError] = useState<string | undefined>();
  const [loadingTeams, setLoadingTeams] = useState(false);
  const [team, setTeam] = useState("");
  const [input, setInput] = useState("");
  const [cwd, setCwd] = useState("~");
  const [running, setRunning] = useState(false);
  const [runError, setRunError] = useState<string | undefined>();

  // Keep the selected host valid if the fleet changes while the modal is open.
  useEffect(() => {
    if (!hostOptions.includes(hostName)) {
      setHostName(hostOptions[0] ?? "");
    }
  }, [hostOptions, hostName]);

  // Load the team presets whenever the host changes.
  useEffect(() => {
    if (!hostName) {
      setTeams([]);
      setTeam("");
      return;
    }
    let cancelled = false;
    setLoadingTeams(true);
    setTeamsError(undefined);
    window.crow
      .teamList(hostName)
      .then((result) => {
        if (cancelled) return;
        setTeams(result.teams);
        setTeam((current) =>
          result.teams.some((t) => t.name === current) ? current : (result.teams[0]?.name ?? ""),
        );
      })
      .catch((error: unknown) => {
        if (cancelled) return;
        setTeams([]);
        setTeam("");
        setTeamsError(error instanceof Error ? error.message : String(error));
      })
      .finally(() => {
        if (!cancelled) setLoadingTeams(false);
      });
    return () => {
      cancelled = true;
    };
  }, [hostName]);

  const canRun = hostName !== "" && team !== "" && input.trim() !== "" && !running;

  async function handleRun() {
    if (!canRun) return;
    setRunning(true);
    setRunError(undefined);
    try {
      const { runId } = await window.crow.teamRun({
        hostName,
        team,
        input: input.trim(),
        cwd: cwd.trim() || "~",
      });
      dispatch({ type: "team.started", hostName, runId, team, input: input.trim() });
      onClose();
    } catch (error) {
      setRunError(error instanceof Error ? error.message : String(error));
      setRunning(false);
    }
  }

  const selectedTeam = teams.find((t) => t.name === team);

  return (
    <div
      className="fixed inset-0 z-10 flex items-center justify-center bg-ink/80"
      onClick={onClose}
    >
      <div
        className="w-[28rem] rounded border border-line bg-ink-1 p-4 shadow-xl"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="mb-3 flex items-center justify-between">
          <span className="text-sm font-semibold text-crow">run team</span>
          <button onClick={onClose} className="text-xs text-fg-dim hover:text-crow">
            esc
          </button>
        </div>

        <label className="mb-1 block text-xs text-fg-dim">host</label>
        <select
          value={hostName}
          onChange={(e) => setHostName(e.target.value)}
          disabled={hostOptions.length === 0}
          className="mb-2.5 w-full rounded border border-line bg-ink px-2 py-1 text-xs outline-none focus:border-crow disabled:opacity-50"
        >
          {hostOptions.length === 0 ? <option value="">no connected hosts</option> : null}
          {hostOptions.map((name) => (
            <option key={name} value={name}>
              {name}
            </option>
          ))}
        </select>

        <label className="mb-1 block text-xs text-fg-dim">team</label>
        <select
          value={team}
          onChange={(e) => setTeam(e.target.value)}
          disabled={loadingTeams || teams.length === 0}
          className="mb-1 w-full rounded border border-line bg-ink px-2 py-1 text-xs outline-none focus:border-crow disabled:opacity-50"
        >
          {teams.length === 0 ? (
            <option value="">{loadingTeams ? "loading teams…" : "no teams on this host"}</option>
          ) : null}
          {teams.map((t) => (
            <option key={t.name} value={t.name}>
              {t.name}
            </option>
          ))}
        </select>
        {selectedTeam ? (
          <p className="mb-2.5 text-[11px] text-fg-dim">
            {selectedTeam.description}
            {selectedTeam.agents.length > 0
              ? ` — ${selectedTeam.agents.map((a) => `${a.name} (${a.role})`).join(", ")}`
              : ""}
          </p>
        ) : null}
        {teamsError ? <p className="mb-2.5 text-[11px] text-danger">{teamsError}</p> : null}

        <label className="mb-1 block text-xs text-fg-dim">input</label>
        <textarea
          value={input}
          onChange={(e) => setInput(e.target.value)}
          rows={4}
          placeholder="what should the team do?"
          className="mb-2.5 w-full resize-y rounded border border-line bg-ink px-2 py-1 text-xs outline-none focus:border-crow"
        />

        <label className="mb-1 block text-xs text-fg-dim">working directory</label>
        <input
          value={cwd}
          onChange={(e) => setCwd(e.target.value)}
          placeholder="~"
          className="mb-3 w-full rounded border border-line bg-ink px-2 py-1 font-mono text-xs outline-none focus:border-crow"
        />

        {runError ? <p className="mb-2 text-xs text-danger">{runError}</p> : null}

        <button
          onClick={() => void handleRun()}
          disabled={!canRun}
          className="w-full rounded bg-crow px-2 py-1.5 text-xs font-semibold text-ink hover:bg-crow-dim disabled:opacity-50"
        >
          {running ? "starting…" : "run team"}
        </button>
      </div>
    </div>
  );
}
