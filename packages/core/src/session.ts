import {
  AgentHarness,
  AgentHarnessError,
  JsonlSessionRepo,
  type AgentHarnessEvent,
  type Skill,
} from "@earendil-works/pi-agent-core";
import { NodeExecutionEnv } from "@earendil-works/pi-agent-core/node";
import type { Models, TextContent } from "@earendil-works/pi-ai";

import {
  ApprovalGate,
  type ApprovalAsk,
  type ApprovalCheckResult,
  type ApprovalMode,
} from "./approvals.ts";
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
  approvalMode: ApprovalMode;
}

/** Tool outputs are capped before hitting the wire; full output stays in the session log. */
const TOOL_OUTPUT_MAX_CHARS = 4000;

const DEFAULT_BASE_PROMPT =
  "You are Crow, an autonomous coding agent running inside the crow daemon.";

export { DEFAULT_BASE_PROMPT };

export class CrowSession {
  readonly id: string;
  readonly cwd: string;
  readonly modelRef: string;
  readonly createdAt: string;

  private readonly harness: AgentHarness;
  private readonly listeners = new Set<CrowSessionListener>();
  private readonly unsubscribeHarness: () => void;
  private readonly unsubscribeApproval: () => void;
  private readonly approvalGate: ApprovalGate;
  private state: CrowSessionState = "idle";

  constructor(options: {
    id: string;
    cwd: string;
    harness: AgentHarness;
    modelRef: string;
    createdAt?: string;
    approvalGate?: ApprovalGate;
  }) {
    this.id = options.id;
    this.cwd = options.cwd;
    this.harness = options.harness;
    this.modelRef = options.modelRef;
    this.createdAt = options.createdAt ?? new Date().toISOString();
    this.approvalGate = options.approvalGate ?? new ApprovalGate();
    // The tool_call hook is an interceptor (subscribe() never sees it): in
    // "ask" mode the gate pauses each tool call here until the approver
    // answers. In "auto" mode check() resolves allow immediately, so this is
    // behavior-preserving. It is independent of the tool_execution_* pair
    // mapped to session events below — those still fire for blocked calls
    // (the block surfaces as an error tool_result with the deny reason).
    this.unsubscribeApproval = this.harness.on("tool_call", async (event) => {
      let result: ApprovalCheckResult;
      try {
        result = await this.approvalGate.check(event.toolCallId, event.toolName, event.input);
      } catch (error) {
        // A broken approver must not wedge the run; deny the call instead.
        const message = error instanceof Error ? error.message : String(error);
        return { block: true, reason: `approval failed: ${message}` };
      }
      return result.allow
        ? undefined
        : { block: true, reason: result.reason ?? "tool call not approved" };
    });
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
      approvalMode: this.approvalGate.mode,
    };
  }

  async close(): Promise<void> {
    this.unsubscribeHarness();
    this.unsubscribeApproval();
    this.listeners.clear();
    await this.harness.env.cleanup();
  }
}

export interface CrowSessionManagerOptions {
  sessionsRoot: string;
  models: Models;
  defaultModelRef?: string;
}

export interface BuildSessionHarnessOptions {
  cwd: string;
  models: Models;
  modelRef: string;
  sessionsRoot: string;
  /** Base system prompt; loaded skill instructions are appended to it. */
  baseSystemPrompt: string;
  /** Whitelist of default coding tool names; absent means the full set. */
  tools?: string[];
  /** Daemon-trusted skill dirs (typically outside cwd, e.g. ~/.crow/skills). */
  skillDirs?: string[];
}

export interface BuiltSessionHarness {
  harness: AgentHarness;
  sessionId: string;
  createdAt: string;
}

/**
 * Shared session construction for interactive sessions (CrowSessionManager)
 * and one-shot sub-agents (SubAgentRunner): confined env + default coding
 * tools (optionally whitelisted) + pi session log + harness.
 */
export async function buildSessionHarness(
  options: BuildSessionHarnessOptions,
): Promise<BuiltSessionHarness> {
  const model = resolveModelRef(options.models, options.modelRef);
  const nodeEnv = new NodeExecutionEnv({ cwd: options.cwd });
  const confined = new ConfinedExecutionEnv(nodeEnv, options.cwd);
  const repo = new JsonlSessionRepo({ fs: nodeEnv, sessionsRoot: options.sessionsRoot });
  const session = await repo.create({ cwd: options.cwd });
  let tools = createCodingTools(confined);
  if (options.tools) {
    const allowed = new Set(options.tools);
    tools = tools.filter((tool) => allowed.has(tool.name));
  }
  // Skills are daemon-trusted config and typically live outside the session
  // cwd (~/.crow/skills), so they load through the unconfined env on purpose.
  let skills: Skill[] = [];
  if (options.skillDirs?.length) {
    skills = (await loadCrowSkills(nodeEnv, options.skillDirs)).skills;
  }
  const systemPrompt = buildSystemPrompt(options.baseSystemPrompt, skills);
  const harness = new AgentHarness({
    env: confined,
    session,
    models: options.models,
    model,
    tools,
    resources: { skills },
    systemPrompt,
  });
  const metadata = await session.getMetadata();
  return { harness, sessionId: metadata.id, createdAt: metadata.createdAt };
}

export interface CreateSessionOptions {
  cwd: string;
  modelRef?: string;
  systemPrompt?: string;
  skillDirs?: string[];
  /** Tool-call approval mode; defaults to "auto" (everything runs). */
  approvalMode?: ApprovalMode;
  /** Tool names that never ask, even in "ask" mode. */
  autoApproveTools?: string[];
  /**
   * Approver callback for "ask" mode, injected by the daemon (fans out to
   * attached clients). Ignored in "auto" mode.
   */
  approvalAsk?: ApprovalAsk;
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
    const built = await buildSessionHarness({
      cwd: options.cwd,
      models: this.models,
      modelRef,
      sessionsRoot: this.sessionsRoot,
      baseSystemPrompt: options.systemPrompt ?? DEFAULT_BASE_PROMPT,
      ...(options.skillDirs !== undefined ? { skillDirs: options.skillDirs } : {}),
    });
    const approvalGate = new ApprovalGate({
      ...(options.approvalMode !== undefined ? { mode: options.approvalMode } : {}),
      ...(options.autoApproveTools !== undefined
        ? { autoApproveTools: options.autoApproveTools }
        : {}),
      ...(options.approvalAsk !== undefined ? { ask: options.approvalAsk } : {}),
    });
    const crowSession = new CrowSession({
      id: built.sessionId,
      cwd: options.cwd,
      harness: built.harness,
      modelRef,
      createdAt: built.createdAt,
      approvalGate,
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
