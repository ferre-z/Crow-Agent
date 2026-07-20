import type { SubAgentRunner } from "./subagents.ts";

export interface TeamAgentSpec {
  name: string;
  role: string;
  systemPrompt?: string;
  /** Whitelist of default coding tool names; absent = full set. */
  tools?: string[];
  model?: string;
  /**
   * P5: when set, this step is delegated to the given A2A base URL instead
   * of running locally. The runner invokes its `delegate` hook.
   */
  host?: string;
}

/**
 * Hook for cross-host step delegation (P5). When the runner sees a step
 * with `host` set, it calls this instead of spawning a local sub-agent.
 * `baseUrl` is the A2A endpoint; the hook POSTs the task, waits for
 * completion, and returns the final output text.
 */
export type TeamStepDelegate = (
  baseUrl: string,
  params: {
    prompt: string;
    cwd: string;
    systemPrompt?: string;
    tools?: string[];
    model?: string;
  },
) => Promise<{ output: string }>;

export interface TeamPreset {
  name: string;
  description: string;
  agents: TeamAgentSpec[];
}

const READ_ONLY = ["read"];

/**
 * Declarative team presets, addressable by name over the wire (`team.run`).
 * Keep these short: the role/systemPrompt text is the whole definition.
 */
export const TEAM_PRESETS: TeamPreset[] = [
  {
    name: "plan-implement-review",
    description:
      "A planner drafts a plan, an implementer executes it, a reviewer returns a verdict.",
    agents: [
      {
        name: "planner",
        role: "Plan the work",
        systemPrompt:
          "You are the planner on a Crow team. Analyze the task and produce a concrete, " +
          "step-by-step implementation plan. Do not modify any files.",
        tools: READ_ONLY,
      },
      {
        name: "implementer",
        role: "Implement the plan",
        systemPrompt:
          "You are the implementer on a Crow team. Follow the plan from the work so far " +
          "and make the changes in the repository. Reply with a summary of what you changed.",
      },
      {
        name: "reviewer",
        role: "Review the implementation",
        systemPrompt:
          "You are the reviewer on a Crow team. Review the plan and the implementation " +
          "from the work so far. Do not modify any files. Reply with a verdict: what is " +
          "correct, what is missing, and whether the work is acceptable.",
        tools: READ_ONLY,
      },
    ],
  },
  {
    name: "solo-review",
    description: "A single read-only reviewer returns a verdict on the input.",
    agents: [
      {
        name: "reviewer",
        role: "Review the input",
        systemPrompt:
          "You are a Crow reviewer. Review the given material carefully. Do not modify " +
          "any files. Reply with a verdict: findings first, then an overall assessment.",
        tools: READ_ONLY,
      },
    ],
  },
];

export type TeamRunEvent =
  | { state: "step_started"; step: number; agent: string }
  | { state: "step_done"; step: number; agent: string; output: string }
  | { state: "error"; step: number; agent: string; error: string };

export type TeamRunEventListener = (event: TeamRunEvent) => void;

export interface RunTeamOptions {
  cwd: string;
  model?: string;
}

/**
 * Runs a team preset as a strict sequence of sub-agents sharing one cwd, so
 * earlier agents' file edits are visible to later ones. Each step's prompt
 * carries the task plus the formatted outputs of all previous steps.
 */
export class TeamRunner {
  private readonly subAgents: SubAgentRunner;
  private readonly delegate?: TeamStepDelegate;

  constructor(subAgents: SubAgentRunner, options: { delegate?: TeamStepDelegate } = {}) {
    this.subAgents = subAgents;
    this.delegate = options.delegate;
  }

  getPreset(name: string): TeamPreset | undefined {
    return TEAM_PRESETS.find((preset) => preset.name === name);
  }

  async run(
    presetName: string,
    input: string,
    options: RunTeamOptions,
    onEvent: TeamRunEventListener,
  ): Promise<{ output: string }> {
    const preset = this.getPreset(presetName);
    if (!preset) {
      throw new Error(`unknown team: ${presetName}`);
    }
    const previous: { name: string; output: string }[] = [];
    for (const [index, agent] of preset.agents.entries()) {
      const step = index + 1; // wire steps are 1-based
      onEvent({ state: "step_started", step, agent: agent.name });
      let prompt = `${agent.systemPrompt ?? agent.role}\n\nTask:\n${input}`;
      if (previous.length > 0) {
        prompt +=
          "\n\nWork so far:\n" +
          previous.map((entry) => `## ${entry.name}\n${entry.output}`).join("\n\n");
      }
      try {
        const model = agent.model ?? options.model;
        const stepBaseParams = {
          prompt,
          cwd: options.cwd,
          ...(agent.systemPrompt !== undefined ? { systemPrompt: agent.systemPrompt } : {}),
          ...(agent.tools !== undefined ? { tools: agent.tools } : {}),
          ...(model !== undefined ? { model } : {}),
        };
        let output: string;
        if (agent.host) {
          if (!this.delegate) {
            throw new Error(
              `step '${agent.name}' requires A2A delegation but no delegate is configured`,
            );
          }
          ({ output } = await this.delegate(agent.host, stepBaseParams));
        } else {
          const { done } = await this.subAgents.spawn(stepBaseParams);
          ({ output } = await done);
        }
        previous.push({ name: agent.name, output });
        onEvent({ state: "step_done", step, agent: agent.name, output });
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        onEvent({ state: "error", step, agent: agent.name, error: message });
        throw error;
      }
    }
    return { output: previous.at(-1)?.output ?? "" };
  }
}
