import { mkdir, mkdtemp, rm } from "node:fs/promises";
import os from "node:os";
import path from "node:path";

import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { SubAgentRunner } from "./subagents.ts";
import { TEAM_PRESETS, TeamRunner, type TeamRunEvent } from "./teams.ts";
import { FAUX_MODEL_REF, fauxAssistantMessage, fauxText, makeFauxModels } from "./testing/faux.ts";

describe("TeamRunner", () => {
  let tmp: string;
  let workdir: string;
  let subAgents: SubAgentRunner | undefined;
  let runner: TeamRunner;

  beforeEach(async () => {
    tmp = await mkdtemp(path.join(os.tmpdir(), "crow-team-"));
    workdir = path.join(tmp, "work");
    await mkdir(workdir, { recursive: true });
  });

  afterEach(async () => {
    await subAgents?.shutdown();
    await rm(tmp, { recursive: true, force: true });
  });

  const setup = (models: ReturnType<typeof makeFauxModels>["models"]) => {
    subAgents = new SubAgentRunner({
      sessionsRoot: path.join(tmp, "subagent-sessions"),
      models,
      defaultModelRef: FAUX_MODEL_REF,
    });
    runner = new TeamRunner(subAgents);
  };

  it("exposes the built-in presets", () => {
    const names = TEAM_PRESETS.map((preset) => preset.name);
    expect(names).toContain("plan-implement-review");
    expect(names).toContain("solo-review");
    for (const preset of TEAM_PRESETS) {
      expect(preset.description.length).toBeGreaterThan(0);
      expect(preset.agents.length).toBeGreaterThan(0);
      for (const agent of preset.agents) {
        expect(agent.name).toBeTruthy();
        expect(agent.role).toBeTruthy();
      }
    }
  });

  it("threads each step's output into the next step's prompt", async () => {
    const { models, faux } = makeFauxModels();
    setup(models);
    const seenContexts: unknown[] = [];
    faux.setResponses([
      (context) => {
        seenContexts.push(context.messages);
        return fauxAssistantMessage([fauxText("PLAN: step one")]);
      },
      (context) => {
        seenContexts.push(context.messages);
        return fauxAssistantMessage([fauxText("IMPLEMENTED: did step one")]);
      },
      (context) => {
        seenContexts.push(context.messages);
        return fauxAssistantMessage([fauxText("VERDICT: acceptable")]);
      },
    ]);

    const events: TeamRunEvent[] = [];
    const result = await runner.run(
      "plan-implement-review",
      "add a feature",
      { cwd: workdir },
      (event) => events.push(event),
    );

    expect(result).toEqual({ output: "VERDICT: acceptable" });
    expect(events).toEqual([
      { state: "step_started", step: 1, agent: "planner" },
      { state: "step_done", step: 1, agent: "planner", output: "PLAN: step one" },
      { state: "step_started", step: 2, agent: "implementer" },
      { state: "step_done", step: 2, agent: "implementer", output: "IMPLEMENTED: did step one" },
      { state: "step_started", step: 3, agent: "reviewer" },
      { state: "step_done", step: 3, agent: "reviewer", output: "VERDICT: acceptable" },
    ]);

    // The first step sees the bare task; later steps see the work so far.
    const first = JSON.stringify(seenContexts[0]);
    expect(first).toContain("add a feature");
    expect(first).not.toContain("Work so far");
    const second = JSON.stringify(seenContexts[1]);
    expect(second).toContain("Work so far");
    expect(second).toContain("PLAN: step one");
    const third = JSON.stringify(seenContexts[2]);
    expect(third).toContain("PLAN: step one");
    expect(third).toContain("IMPLEMENTED: did step one");
  });

  it("runs solo-review as a single step", async () => {
    const { models, faux } = makeFauxModels();
    setup(models);
    faux.setResponses([fauxAssistantMessage([fauxText("VERDICT: fine")])]);

    const events: TeamRunEvent[] = [];
    const result = await runner.run("solo-review", "look at this", { cwd: workdir }, (e) =>
      events.push(e),
    );

    expect(result).toEqual({ output: "VERDICT: fine" });
    expect(events).toEqual([
      { state: "step_started", step: 1, agent: "reviewer" },
      { state: "step_done", step: 1, agent: "reviewer", output: "VERDICT: fine" },
    ]);
    expect(faux.state.callCount).toBe(1);
  });

  it("throws on an unknown team name", async () => {
    const { models } = makeFauxModels();
    setup(models);
    await expect(runner.run("no-such-team", "input", { cwd: workdir }, () => {})).rejects.toThrow(
      "unknown team: no-such-team",
    );
  });

  it("emits an error event and rethrows when a step fails", async () => {
    const { models, faux } = makeFauxModels();
    setup(models);
    faux.setResponses([
      fauxAssistantMessage([fauxText("PLAN: step one")]),
      fauxAssistantMessage("boom", { stopReason: "error", errorMessage: "impl exploded" }),
    ]);

    const events: TeamRunEvent[] = [];
    await expect(
      runner.run("plan-implement-review", "task", { cwd: workdir }, (e) => events.push(e)),
    ).rejects.toThrow("impl exploded");

    expect(events).toEqual([
      { state: "step_started", step: 1, agent: "planner" },
      { state: "step_done", step: 1, agent: "planner", output: "PLAN: step one" },
      { state: "step_started", step: 2, agent: "implementer" },
      { state: "error", step: 2, agent: "implementer", error: "impl exploded" },
    ]);
  });
});
