import { randomUUID } from "node:crypto";

import type { AgentHarness } from "@earendil-works/pi-agent-core";
import type { Models, TextContent } from "@earendil-works/pi-ai";

import { DEFAULT_MODEL_REF } from "./models.ts";
import { buildSessionHarness } from "./session.ts";

export const DEFAULT_SUBAGENT_PROMPT =
  "You are a Crow sub-agent. Complete the task and reply with the result.";

export interface SubAgentRunnerOptions {
  sessionsRoot: string;
  models: Models;
  defaultModelRef?: string;
}

export interface SpawnSubAgentOptions {
  prompt: string;
  cwd: string;
  systemPrompt?: string;
  /** Whitelist of default coding tool names (read/write/edit/bash); absent = full set. */
  tools?: string[];
  model?: string;
}

export interface SubAgentHandle {
  agentId: string;
  /**
   * Resolves with the final assistant message's text on success; rejects with
   * Error(message) when the run ends in an error/aborted state.
   */
  done: Promise<{ output: string }>;
}

/**
 * Spawns one-shot agent runs, each with a fresh harness and its own session
 * log under `sessionsRoot`. Unlike interactive CrowSessions there is no
 * approval gate: a sub-agent runs to completion unattended.
 */
export class SubAgentRunner {
  private readonly sessionsRoot: string;
  private readonly models: Models;
  private readonly defaultModelRef: string;
  private readonly active = new Set<AgentHarness>();

  constructor(options: SubAgentRunnerOptions) {
    this.sessionsRoot = options.sessionsRoot;
    this.models = options.models;
    this.defaultModelRef = options.defaultModelRef ?? DEFAULT_MODEL_REF;
  }

  async spawn(options: SpawnSubAgentOptions): Promise<SubAgentHandle> {
    const agentId = `agent_${randomUUID()}`;
    const { harness } = await buildSessionHarness({
      cwd: options.cwd,
      models: this.models,
      modelRef: options.model ?? this.defaultModelRef,
      sessionsRoot: this.sessionsRoot,
      baseSystemPrompt: options.systemPrompt ?? DEFAULT_SUBAGENT_PROMPT,
      ...(options.tools !== undefined ? { tools: options.tools } : {}),
    });
    this.active.add(harness);
    const done = this.runToCompletion(harness, options.prompt).finally(() => {
      this.active.delete(harness);
    });
    return { agentId, done };
  }

  private async runToCompletion(
    harness: AgentHarness,
    prompt: string,
  ): Promise<{ output: string }> {
    try {
      const message = await harness.prompt(prompt);
      if (message.stopReason === "error" || message.stopReason === "aborted") {
        throw new Error(message.errorMessage ?? `agent run ${message.stopReason}`);
      }
      const output = message.content
        .filter((block): block is TextContent => block.type === "text")
        .map((block) => block.text)
        .join("\n");
      return { output };
    } finally {
      await harness.env.cleanup();
    }
  }

  /** Abort every in-flight sub-agent; their `done` promises reject as aborted. */
  async shutdown(): Promise<void> {
    const running = [...this.active];
    this.active.clear();
    await Promise.all(running.map((harness) => harness.abort()));
  }
}
