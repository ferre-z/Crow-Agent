import {
  AgentHarness,
  AgentHarnessError,
  JsonlSessionRepo,
  type AgentHarnessEvent,
  type Skill,
} from "@earendil-works/pi-agent-core";
import { NodeExecutionEnv } from "@earendil-works/pi-agent-core/node";
import type { Models, TextContent } from "@earendil-works/pi-ai";

import { ConfinedExecutionEnv } from "./env/confined-env.ts";
import { DEFAULT_MODEL_REF, resolveModelRef } from "./models.ts";
import { buildSystemPrompt, loadCrowSkills } from "./skills.ts";
import { createCodingTools } from "./tools/index.ts";

export type CrowSessionState = "idle" | "streaming" | "error";

/**
 * Session events, deliberately mirroring the @crow/protocol `event.*` params
 * minus `sessionId` (the daemon adds that when fanning out to clients).
 */
export type CrowSessionEvent =
  | { type: "token"; text: string }
  | { type: "thinking"; text: string }
  | { type: "tool_call"; callId: string; tool: string; args: unknown }
  | { type: "tool_result"; callId: string; tool: string; output: string; isError: boolean }
  | { type: "state"; state: CrowSessionState; error?: string };

export type CrowSessionListener = (event: CrowSessionEvent) => void;

export interface CrowSessionInfo {
  id: string;
  cwd: string;
  modelRef: string;
  state: CrowSessionState;
  createdAt: string;
}

/** Tool outputs are capped before hitting the wire; full output stays in the session log. */
const TOOL_OUTPUT_MAX_CHARS = 4000;

const DEFAULT_BASE_PROMPT =
  "You are Crow, an autonomous coding agent running inside the crow daemon.";

export class CrowSession {
  readonly id: string;
  readonly cwd: string;
  readonly modelRef: string;
  readonly createdAt: string;

  private readonly harness: AgentHarness;
  private readonly listeners = new Set<CrowSessionListener>();
  private readonly unsubscribeHarness: () => void;
  private state: CrowSessionState = "idle";

  constructor(options: {
    id: string;
    cwd: string;
    harness: AgentHarness;
    modelRef: string;
    createdAt?: string;
  }) {
    this.id = options.id;
    this.cwd = options.cwd;
    this.harness = options.harness;
    this.modelRef = options.modelRef;
    this.createdAt = options.createdAt ?? new Date().toISOString();
    // One harness subscription for the session lifetime. The listener is
    // synchronous and returns void, so harness fan-out never awaits us; crow
    // listeners are sync void by contract and each call is guarded.
    this.unsubscribeHarness = this.harness.subscribe((event) => this.onHarnessEvent(event));
  }

  subscribe(listener: CrowSessionListener): () => void {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  }

  private emit(event: CrowSessionEvent): void {
    for (const listener of this.listeners) {
      try {
        listener(event);
      } catch {
        // A broken listener must not break the session event pipeline.
      }
    }
  }

  private setState(state: CrowSessionState, error?: string): void {
    this.state = state;
    this.emit({ type: "state", state, ...(error !== undefined ? { error } : {}) });
  }

  private onHarnessEvent(event: AgentHarnessEvent): void {
    switch (event.type) {
      case "agent_start":
        this.setState("streaming");
        break;
      case "agent_end": {
        const last = event.messages.filter((m) => m.role === "assistant").at(-1);
        if (last && (last.stopReason === "error" || last.stopReason === "aborted")) {
          this.setState("error", last.errorMessage ?? `agent run ${last.stopReason}`);
        } else {
          this.setState("idle");
        }
        break;
      }
      case "message_update": {
        const e = event.assistantMessageEvent;
        if (e.type === "text_delta") {
          this.emit({ type: "token", text: e.delta });
        } else if (e.type === "thinking_delta") {
          this.emit({ type: "thinking", text: e.delta });
        }
        break;
      }
      case "tool_execution_start":
        // NOTE: the harness-own "tool_call"/"tool_result" events are `on()`
        // hooks, not subscriber events; the agent-level tool_execution_*
        // pair carries the same validated args/result payloads.
        this.emit({
          type: "tool_call",
          callId: event.toolCallId,
          tool: event.toolName,
          args: event.args as unknown,
        });
        break;
      case "tool_execution_end": {
        const result = event.result as { content?: unknown } | undefined;
        const content = Array.isArray(result?.content) ? result.content : [];
        let output = content
          .filter((c): c is TextContent => {
            const block = c as { type?: unknown };
            return block.type === "text";
          })
          .map((c) => c.text)
          .join("\n");
        if (output.length > TOOL_OUTPUT_MAX_CHARS) {
          output = output.slice(0, TOOL_OUTPUT_MAX_CHARS) + "\n…[truncated]";
        }
        this.emit({
          type: "tool_result",
          callId: event.toolCallId,
          tool: event.toolName,
          output,
          isError: event.isError,
        });
        break;
      }
    }
  }

  /** Run one prompt to completion; resolves when the agent is idle again. */
  async prompt(text: string): Promise<void> {
    try {
      await this.harness.prompt(text);
    } catch (error) {
      if (error instanceof AgentHarnessError && error.code === "busy") {
        throw new Error(`session ${this.id} is busy`, { cause: error });
      }
      throw error;
    }
  }

  async cancel(): Promise<void> {
    await this.harness.abort();
    await this.harness.waitForIdle();
  }

  getInfo(): CrowSessionInfo {
    return {
      id: this.id,
      cwd: this.cwd,
      modelRef: this.modelRef,
      state: this.state,
      createdAt: this.createdAt,
    };
  }

  async close(): Promise<void> {
    this.unsubscribeHarness();
    this.listeners.clear();
    await this.harness.env.cleanup();
  }
}

export interface CrowSessionManagerOptions {
  sessionsRoot: string;
  models: Models;
  defaultModelRef?: string;
}

export interface CreateSessionOptions {
  cwd: string;
  modelRef?: string;
  systemPrompt?: string;
  skillDirs?: string[];
}

export class CrowSessionManager {
  private readonly sessionsRoot: string;
  private readonly models: Models;
  private readonly defaultModelRef: string;
  private readonly sessions = new Map<string, CrowSession>();

  constructor(options: CrowSessionManagerOptions) {
    this.sessionsRoot = options.sessionsRoot;
    this.models = options.models;
    this.defaultModelRef = options.defaultModelRef ?? DEFAULT_MODEL_REF;
  }

  async create(options: CreateSessionOptions): Promise<CrowSession> {
    const modelRef = options.modelRef ?? this.defaultModelRef;
    const model = resolveModelRef(this.models, modelRef);
    const nodeEnv = new NodeExecutionEnv({ cwd: options.cwd });
    const confined = new ConfinedExecutionEnv(nodeEnv, options.cwd);
    const repo = new JsonlSessionRepo({ fs: nodeEnv, sessionsRoot: this.sessionsRoot });
    const session = await repo.create({ cwd: options.cwd });
    const tools = createCodingTools(confined);
    // Skills are daemon-trusted config and typically live outside the session
    // cwd (~/.crow/skills), so they load through the unconfined env on purpose.
    let skills: Skill[] = [];
    if (options.skillDirs?.length) {
      skills = (await loadCrowSkills(nodeEnv, options.skillDirs)).skills;
    }
    const systemPrompt = buildSystemPrompt(options.systemPrompt ?? DEFAULT_BASE_PROMPT, skills);
    const harness = new AgentHarness({
      env: confined,
      session,
      models: this.models,
      model,
      tools,
      resources: { skills },
      systemPrompt,
    });
    const metadata = await session.getMetadata();
    const crowSession = new CrowSession({
      id: metadata.id,
      cwd: options.cwd,
      harness,
      modelRef,
      createdAt: metadata.createdAt,
    });
    this.sessions.set(crowSession.id, crowSession);
    return crowSession;
  }

  get(id: string): CrowSession | undefined {
    return this.sessions.get(id);
  }

  list(): CrowSessionInfo[] {
    return [...this.sessions.values()].map((s) => s.getInfo());
  }

  /** Close and drop a session. Returns false when the id is unknown. */
  async remove(id: string): Promise<boolean> {
    const session = this.sessions.get(id);
    if (!session) return false;
    await session.close();
    return this.sessions.delete(id);
  }

  async shutdown(): Promise<void> {
    const all = [...this.sessions.values()];
    this.sessions.clear();
    await Promise.all(all.map((s) => s.close()));
  }
}
