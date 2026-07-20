import fs from "node:fs";
import { NodeExecutionEnv } from "@earendil-works/pi-agent-core/node";
import { randomUUID } from "node:crypto";
import os from "node:os";
import path from "node:path";

import { MemoryStore } from "@crow/memory";

import {
  CrowSessionManager,
  createCrowModels,
  CronScheduler,
  DEFAULT_BASE_PROMPT,
  DEFAULT_MODEL_REF,
  isWorkflow,
  SubAgentRunner,
  TEAM_PRESETS,
  TeamRunner,
  WorkflowRunner,
  type ApprovalDecision,
  type ApprovalRequest,
  type ApprovalVerdict,
  type CrowSession,
  type CrowSessionEvent,
  type CrowSessionInfo,
  type Models,
  type Workflow,
} from "@crow/core";
import { CrowA2aClient } from "./a2a-client.ts";
import { CrowA2aServer } from "./a2a-server.ts";
import {
  approvalRespondParamsSchema,
  encodeFrame,
  EVENTS,
  jsonRpcFrameSchema,
  makeError,
  makeNotification,
  makeResult,
  METHODS,
  methodParamsSchemas,
  NOTIFICATIONS,
  PROTOCOL_VERSION,
  RPC_ERRORS,
  type AgentSpawnParams,
  type ApprovalRespondParams,
  type CronAddParams,
  type CronJobWire,
  type CronListResult,
  type CronRemoveParams,
  type HostInfoResult,
  type JsonRpcFrame,
  type JsonRpcNotification,
  type JsonRpcRequest,
  type RequestId,
  type SessionAttachParams,
  type SessionCancelParams,
  type SessionCreateParams,
  type SessionInfo,
  type SessionSendParams,
  type TeamListResult,
  type TeamRunParams,
  type MemoryEpisodeWire,
  type MemoryEpisodesResult,
  type MemoryFactWire,
  type MemoryFactsResult,
  type MemoryHitWire,
  type MemoryQueryParams,
  type MemoryQueryResult,
  type MemoryWriteParams,
  type MemoryWriteResult,
  type WorkflowInfo,
  type WorkflowListResult,
  type WorkflowRunParams,
  type WorkflowRunResult,
} from "@crow/protocol";
import { WebSocket, WebSocketServer } from "ws";

export const DAEMON_VERSION = "0.1.0" as const;

/** Default time a tool call waits for an approval.respond before being denied. */
export const DEFAULT_APPROVAL_TIMEOUT_MS = 120_000;

/** Close policy for a runaway NDJSON accumulator (no newline seen). */
const MAX_BUFFER_BYTES = 1024 * 1024;

/** Minimal structural view of the zod param validators (zod is not a direct dep). */
interface ParamsValidator {
  safeParse(
    input: unknown,
  ):
    { success: true; data: unknown } | { success: false; error: { issues: { message: string }[] } };
}

const paramsValidators: Record<string, ParamsValidator> = methodParamsSchemas;

export interface CrowDaemonOptions {
  host: string;
  port: number;
  token: string;
  dataDir: string;
  /** Injectable for tests (e.g. the faux provider); defaults to all built-in providers. */
  models?: Models;
  defaultModelRef?: string;
  /** How long a tool call waits for approval.respond; defaults to 120 s. */
  approvalTimeoutMs?: number;
  /**
   * P5: when set, the daemon also serves an A2A HTTP surface on this port
   * and accepts agent.spawn delegations from other daemons via the
   * `host` param (and team presets with per-step `host`).
   */
  a2a?: { host?: string; port?: number; publicBaseUrl?: string };
  /** P5: injected A2A client factory — only used by tests to swap the baseUrl. */
  createA2aClient?: (baseUrl: string, token: string) => CrowA2aClient;
}

interface ConnectionState {
  ws: WebSocket;
  attachedSessionIds: Set<string>;
  buffer: string;
}

/** One tool call paused waiting for a client's approval.respond. */
interface PendingApproval {
  sessionId: string;
  resolve: (verdict: ApprovalDecision | ApprovalVerdict) => void;
  timer: ReturnType<typeof setTimeout>;
}

/** Map the richer crow session state onto the wire's coarse idle/busy. */
function toWireSessionInfo(info: CrowSessionInfo): SessionInfo {
  return {
    id: info.id,
    cwd: info.cwd,
    model: info.modelRef,
    state: info.state === "streaming" ? "busy" : "idle",
    createdAt: info.createdAt,
    approvalMode: info.approvalMode,
  };
}

export class CrowDaemon {
  private readonly options: CrowDaemonOptions;
  private readonly manager: CrowSessionManager;
  private readonly subAgents: SubAgentRunner;
  private readonly teams: TeamRunner;
  private readonly workflows: Map<string, Workflow>;
  private readonly scheduler: CronScheduler;
  private readonly memory: MemoryStore;
  private readonly connections = new Set<ConnectionState>();
  private readonly sessionSubscriptions = new Map<string, () => void>();
  private readonly pendingApprovals = new Map<string, PendingApproval>();
  private wss: WebSocketServer | undefined;
  private a2aServer: CrowA2aServer | undefined;
  private a2aBaseUrl: string | undefined;

  constructor(options: CrowDaemonOptions) {
    this.options = options;
    const models = options.models ?? createCrowModels();
    const defaultModelRef = options.defaultModelRef ?? DEFAULT_MODEL_REF;
    this.manager = new CrowSessionManager({
      sessionsRoot: path.join(options.dataDir, "sessions"),
      models,
      defaultModelRef,
    });
    this.subAgents = new SubAgentRunner({
      sessionsRoot: path.join(options.dataDir, "subagent-sessions"),
      models,
      defaultModelRef,
    });
    this.teams = new TeamRunner(this.subAgents, { delegate: this.delegateA2a.bind(this) });
    this.workflows = new Map(loadBuiltinWorkflows(options.dataDir).map((w) => [w.name, w]));
    this.scheduler = new CronScheduler(path.join(options.dataDir, "scheduler.db"));
    this.scheduler.setFireCallback((job) => void this.fireCronJob(job));
    this.scheduler.start();
    this.memory = new MemoryStore({ dbPath: path.join(options.dataDir, "memory.db") });
  }

  private workflowsForRun(name: string): Workflow {
    const workflow = this.workflows.get(name);
    if (!workflow) throw new Error(`unknown workflow: ${name}`);
    return workflow;
  }

  private buildWorkflowRunner(): WorkflowRunner {
    return new WorkflowRunner({
      prompt: async (params) => {
        const { done } = await this.subAgents.spawn(params);
        return await done;
      },
      shell: async (command, options) => this.runWorkflowShell(command, options),
      a2a: this.delegateA2a.bind(this),
    });
  }

  private async runWorkflowShell(
    command: string,
    options: { cwd: string; timeoutSeconds?: number },
  ): Promise<{ stdout: string; stderr: string; exitCode: number }> {
    const env = new NodeExecutionEnv({ cwd: options.cwd });
    const result = await env.exec(command, {
      ...(options.timeoutSeconds !== undefined ? { timeout: options.timeoutSeconds } : {}),
    });
    if (!result.ok) throw new Error(result.error.message);
    return result.value;
  }

  private async fireCronJob(job: {
    id: string;
    name: string;
    workflowName: string;
    inputs: Record<string, unknown>;
  }): Promise<void> {
    let workflow: Workflow;
    try {
      workflow = this.workflowsForRun(job.workflowName);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      this.broadcastAll(
        makeNotification(EVENTS.WORKFLOW, {
          runId: "cron:" + job.id,
          state: "error",
          error: message,
        }),
      );
      return;
    }
    const workflowRunId = `wrun_${job.id}_${Date.now()}`;
    this.broadcastAll(
      makeNotification(EVENTS.CRON_FIRED, {
        jobId: job.id,
        jobName: job.name,
        workflowRunId,
      }),
    );
    void this.executeWorkflow(workflow, workflowRunId, job.inputs).catch(() => {
      // Errors already broadcast as event.workflow.
    });
  }

  private async executeWorkflow(
    workflow: Workflow,
    runId: string,
    inputs: Record<string, unknown>,
  ): Promise<void> {
    const runner = this.buildWorkflowRunner();
    // Render the workflow steps against inputs (simple string substitution).
    const prepared: Workflow = JSON.parse(JSON.stringify(workflow)) as Workflow;
    renderWorkflowInputs(prepared, inputs);
    await runner.run(prepared, (event) => {
      const params: Record<string, unknown> = { runId, ...event };
      this.broadcastAll(makeNotification(EVENTS.WORKFLOW, params));
    });
  }

  /**
   * Hook used by TeamRunner when a preset step declares `host`. Default
   * implementation instantiates a CrowA2aClient; tests inject their own.
   */
  private async delegateA2a(
    baseUrl: string,
    params: {
      prompt: string;
      cwd: string;
      systemPrompt?: string;
      tools?: string[];
      model?: string;
    },
  ): Promise<{ output: string }> {
    const factory =
      this.options.createA2aClient ?? ((url, token) => new CrowA2aClient({ baseUrl: url, token }));
    return factory(baseUrl, this.options.token).delegate(params);
  }

  start(): Promise<{ port: number }> {
    return new Promise((resolve, reject) => {
      const wss = new WebSocketServer({
        host: this.options.host,
        port: this.options.port,
        verifyClient: (info, done) => {
          const expected = `Bearer ${this.options.token}`;
          // Rejected upgrades get an HTTP 401 before any WS frames flow.
          done(info.req.headers.authorization === expected, 401, "unauthorized");
        },
      });
      wss.on("error", reject);
      wss.on("listening", async () => {
        this.wss = wss;
        const address = wss.address();
        const wsPort =
          typeof address === "object" && address !== null ? address.port : this.options.port;
        if (this.options.a2a) {
          const a2aPort = this.options.a2a.port ?? wsPort + 1;
          const a2aHost = this.options.a2a.host ?? this.options.host;
          const server = new CrowA2aServer({
            host: a2aHost,
            port: a2aPort,
            token: this.options.token,
            models: this.options.models ?? createCrowModels(),
            subAgents: this.subAgents,
            ...(this.options.a2a.publicBaseUrl
              ? { publicBaseUrl: this.options.a2a.publicBaseUrl }
              : {}),
          });
          try {
            await server.start();
            this.a2aServer = server;
            this.a2aBaseUrl = server.baseUrl();
          } catch (error) {
            reject(error);
            return;
          }
        }
        resolve({ port: wsPort });
      });
      wss.on("connection", (ws) => this.onConnection(ws));
    });
  }

  async stop(): Promise<void> {
    for (const conn of this.connections) {
      conn.ws.close();
    }
    this.connections.clear();
    for (const unsubscribe of this.sessionSubscriptions.values()) {
      unsubscribe();
    }
    this.sessionSubscriptions.clear();
    // Unblock any tool calls still waiting on an approval so their sessions
    // can settle instead of hanging until the timeout fires.
    for (const [approvalId, pending] of this.pendingApprovals) {
      clearTimeout(pending.timer);
      pending.resolve({ decision: "deny", reason: "daemon shutting down" });
      this.pendingApprovals.delete(approvalId);
    }
    await this.manager.shutdown();
    // In-flight sub-agent/team runs are aborted here: their `done` promises
    // reject as "aborted", and the resulting error events are no-ops because
    // the connections above are already closed (broadcastAll skips them).
    await this.subAgents.shutdown();
    this.scheduler.close();
    this.memory.close();
    if (this.a2aServer) {
      await this.a2aServer.stop();
      this.a2aServer = undefined;
    }
    await new Promise<void>((resolve) => {
      if (!this.wss) {
        resolve();
        return;
      }
      this.wss.close(() => resolve());
    });
  }

  private onConnection(ws: WebSocket): void {
    const conn: ConnectionState = { ws, attachedSessionIds: new Set(), buffer: "" };
    this.connections.add(conn);
    ws.on("message", (data: Buffer) => this.onMessage(conn, data));
    ws.on("close", () => {
      this.connections.delete(conn);
    });
    ws.on("error", () => {
      // The "close" handler above does the cleanup; swallow transport errors.
    });
  }

  private onMessage(conn: ConnectionState, data: Buffer): void {
    conn.buffer += data.toString("utf8");
    if (conn.buffer.length > MAX_BUFFER_BYTES) {
      conn.buffer = "";
      conn.ws.close(1009, "message too big");
      return;
    }
    const lines = conn.buffer.split("\n");
    conn.buffer = lines.pop() ?? "";
    for (const line of lines) {
      if (line.trim().length === 0) continue;
      this.onLine(conn, line);
    }
  }

  private onLine(conn: ConnectionState, line: string): void {
    let raw: unknown;
    try {
      raw = JSON.parse(line);
    } catch {
      this.sendFrame(conn, makeError("unknown", RPC_ERRORS.PARSE_ERROR, "invalid JSON"));
      return;
    }
    const parsed = jsonRpcFrameSchema.safeParse(raw);
    if (!parsed.success) {
      this.sendFrame(
        conn,
        makeError(extractId(raw), RPC_ERRORS.INVALID_REQUEST, "invalid JSON-RPC frame"),
      );
      return;
    }
    const frame = parsed.data;
    if (!("method" in frame)) return; // responses from clients: ignore
    if (!("id" in frame)) {
      this.onNotification(conn, frame);
      return;
    }
    void this.dispatch(conn, frame);
  }

  /** Client notifications never get a response frame; malformed ones are dropped. */
  private onNotification(conn: ConnectionState, notification: JsonRpcNotification): void {
    if (notification.method !== NOTIFICATIONS.APPROVAL_RESPOND) return;
    const parsed = approvalRespondParamsSchema.safeParse(notification.params ?? {});
    if (!parsed.success) return;
    this.onApprovalRespond(conn, parsed.data);
  }

  private onApprovalRespond(conn: ConnectionState, params: ApprovalRespondParams): void {
    const pending = this.pendingApprovals.get(params.approvalId);
    if (!pending) return; // unknown or already resolved/expired: ignore
    if (!conn.attachedSessionIds.has(pending.sessionId)) return; // not your session
    clearTimeout(pending.timer);
    this.pendingApprovals.delete(params.approvalId);
    pending.resolve(params.decision);
  }

  /**
   * ApprovalGate `ask` callback for a session in "ask" mode: fan an
   * `event.approval_request` out to every attached connection and wait for
   * the matching `approval.respond`. Denies when nobody is attached and on
   * timeout; callers see the verdict (with the deny reason) via the gate.
   */
  private askForApproval(
    sessionId: string,
    request: ApprovalRequest,
  ): Promise<ApprovalDecision | ApprovalVerdict> {
    const targets = [...this.connections].filter(
      (conn) => conn.attachedSessionIds.has(sessionId) && conn.ws.readyState === WebSocket.OPEN,
    );
    if (targets.length === 0) {
      return Promise.resolve({ decision: "deny", reason: "no client attached to approve" });
    }
    const approvalId = `appr_${randomUUID()}`;
    const frame = encodeFrame(
      makeNotification(EVENTS.APPROVAL_REQUEST, {
        sessionId,
        approvalId,
        callId: request.callId,
        tool: request.tool,
        args: request.args,
      }),
    );
    return new Promise((resolve) => {
      const timer = setTimeout(() => {
        this.pendingApprovals.delete(approvalId);
        resolve({ decision: "deny", reason: "approval timed out" });
      }, this.options.approvalTimeoutMs ?? DEFAULT_APPROVAL_TIMEOUT_MS);
      // The daemon must be stoppable (and tests exitable) with approvals pending.
      timer.unref();
      this.pendingApprovals.set(approvalId, { sessionId, resolve, timer });
      for (const conn of targets) {
        conn.ws.send(frame);
      }
    });
  }

  private async dispatch(conn: ConnectionState, request: JsonRpcRequest): Promise<void> {
    const { id, method } = request;
    try {
      const validator = paramsValidators[method];
      if (!validator) {
        this.sendFrame(
          conn,
          makeError(id, RPC_ERRORS.METHOD_NOT_FOUND, `unknown method: ${method}`),
        );
        return;
      }
      const params = validator.safeParse(request.params ?? {});
      if (!params.success) {
        const detail = params.error.issues.map((i) => i.message).join("; ");
        this.sendFrame(conn, makeError(id, RPC_ERRORS.INVALID_PARAMS, `invalid params: ${detail}`));
        return;
      }
      const result = await this.handle(conn, method, params.data);
      this.sendFrame(conn, makeResult(id, result ?? {}));
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      const code = codeForMethodError(message);
      this.sendFrame(conn, makeError(id, code, message));
    }
  }

  private async handle(conn: ConnectionState, method: string, params: unknown): Promise<unknown> {
    switch (method) {
      case METHODS.SESSION_CREATE: {
        const p = params as SessionCreateParams;
        // The gate's ask callback only fires once the session is prompting,
        // so closing over `sessionId` before it is assigned below is safe.
        let sessionId = "";
        const memoryBlock = this.memory.contextBlock(`session cwd ${p.cwd}`);
        const composedSystemPrompt = p.systemPrompt
          ? p.systemPrompt
          : memoryBlock
            ? `${DEFAULT_BASE_PROMPT}\n\n${memoryBlock}`
            : undefined;
        const session = await this.manager.create({
          cwd: p.cwd,
          modelRef: p.model,
          ...(composedSystemPrompt !== undefined ? { systemPrompt: composedSystemPrompt } : {}),
          skillDirs: p.skillDirs,
          approvalMode: p.approvalMode,
          autoApproveTools: p.autoApproveTools,
          approvalAsk: (req) => this.askForApproval(sessionId, req),
        });
        sessionId = session.id;
        this.ensureSessionSubscription(session);
        // The creator is implicitly attached to its own session's events.
        conn.attachedSessionIds.add(session.id);
        return { sessionId: session.id };
      }
      case METHODS.SESSION_SEND: {
        const p = params as SessionSendParams;
        const session = this.requireSession(p.sessionId);
        // Fire-and-forget: tokens/thinking/tool events stream as
        // notifications; a rejected run surfaces as a session_state error.
        session.prompt(p.text).catch((error: unknown) => {
          this.broadcastSessionEvent(p.sessionId, {
            type: "state",
            state: "error",
            error: error instanceof Error ? error.message : String(error),
          });
        });
        return {};
      }
      case METHODS.SESSION_CANCEL: {
        const p = params as SessionCancelParams;
        await this.requireSession(p.sessionId).cancel();
        return {};
      }
      case METHODS.SESSION_LIST: {
        return { sessions: this.manager.list().map(toWireSessionInfo) };
      }
      case METHODS.SESSION_ATTACH: {
        const p = params as SessionAttachParams;
        this.requireSession(p.sessionId);
        // P1 has no replay buffer: `since` is accepted and ignored; the client
        // only sees events from this point on.
        conn.attachedSessionIds.add(p.sessionId);
        return {};
      }
      case METHODS.HOST_INFO: {
        const result: HostInfoResult = {
          hostname: os.hostname(),
          platform: process.platform,
          arch: process.arch,
          node: process.version,
          daemonVersion: DAEMON_VERSION,
          protocolVersion: PROTOCOL_VERSION,
          sessions: this.manager.list().length,
          ...(this.a2aBaseUrl ? { a2a: this.a2aBaseUrl } : {}),
        };
        return result;
      }
      case METHODS.AGENT_SPAWN: {
        const p = params as AgentSpawnParams;
        const agentId = `agent_${randomUUID()}`;
        this.broadcastAll(makeNotification(EVENTS.AGENT, { agentId, state: "started" }));
        if (p.host) {
          // P5 cross-daemon delegation: the local daemon delegates to the
          // remote A2A endpoint and surfaces the result as its own events.
          void this.delegateA2a(p.host, {
            prompt: p.prompt,
            cwd: p.cwd,
            ...(p.systemPrompt !== undefined ? { systemPrompt: p.systemPrompt } : {}),
            ...(p.tools !== undefined ? { tools: p.tools } : {}),
            ...(p.model !== undefined ? { model: p.model } : {}),
          }).then(
            ({ output }) =>
              this.broadcastAll(makeNotification(EVENTS.AGENT, { agentId, state: "done", output })),
            (error: unknown) =>
              this.broadcastAll(
                makeNotification(EVENTS.AGENT, {
                  agentId,
                  state: "error",
                  error: error instanceof Error ? error.message : String(error),
                }),
              ),
          );
          return { agentId };
        }
        const { done } = await this.subAgents.spawn({
          prompt: p.prompt,
          cwd: p.cwd,
          ...(p.systemPrompt !== undefined ? { systemPrompt: p.systemPrompt } : {}),
          ...(p.tools !== undefined ? { tools: p.tools } : {}),
          ...(p.model !== undefined ? { model: p.model } : {}),
        });
        // Sub-agent events go to every connected client, not just the spawner.
        done.then(
          ({ output }) =>
            this.broadcastAll(makeNotification(EVENTS.AGENT, { agentId, state: "done", output })),
          (error: unknown) =>
            this.broadcastAll(
              makeNotification(EVENTS.AGENT, {
                agentId,
                state: "error",
                error: error instanceof Error ? error.message : String(error),
              }),
            ),
        );
        return { agentId };
      }
      case METHODS.TEAM_LIST: {
        const result: TeamListResult = {
          teams: TEAM_PRESETS.map((preset) => ({
            name: preset.name,
            description: preset.description,
            agents: preset.agents.map((agent) => ({ name: agent.name, role: agent.role })),
          })),
        };
        return result;
      }
      case METHODS.TEAM_RUN: {
        const p = params as TeamRunParams;
        // Validate before returning a runId: an unknown team is an RPC error
        // (INVALID_PARAMS), not an event.team error.
        if (!this.teams.getPreset(p.team)) {
          throw new Error(`unknown team: ${p.team}`);
        }
        const runId = `run_${randomUUID()}`;
        const run = this.teams.run(
          p.team,
          p.input,
          { cwd: p.cwd, ...(p.model !== undefined ? { model: p.model } : {}) },
          (event) => this.broadcastAll(makeNotification(EVENTS.TEAM, { runId, ...event })),
        );
        run.then(
          ({ output }) =>
            this.broadcastAll(makeNotification(EVENTS.TEAM, { runId, state: "done", output })),
          () => {
            // The failing step already broadcast its event.team error.
          },
        );
        return { runId };
      }
      case METHODS.WORKFLOW_LIST: {
        const result: WorkflowListResult = {
          workflows: Array.from(this.workflows.values()).map(workflowInfo),
        };
        return result;
      }
      case METHODS.WORKFLOW_RUN: {
        const p = params as WorkflowRunParams;
        const workflow = this.workflowsForRun(p.workflow);
        const runId = `wrun_${randomUUID()}`;
        void this.executeWorkflow(workflow, runId, p.inputs ?? {}).catch(() => {
          // Errors already broadcast as event.workflow.
        });
        const result: WorkflowRunResult = { runId };
        return result;
      }
      case METHODS.CRON_ADD: {
        const p = params as CronAddParams;
        if (!this.workflows.has(p.workflow)) {
          throw new Error(`unknown workflow: ${p.workflow}`);
        }
        const job = this.scheduler.add({
          name: p.name,
          workflowName: p.workflow,
          recurrence: p.recurrence,
          ...(p.inputs !== undefined ? { inputs: p.inputs } : {}),
        });
        return cronJobToWire(job);
      }
      case METHODS.CRON_LIST: {
        const result: CronListResult = {
          jobs: this.scheduler.list().map(cronJobToWire),
        };
        return result;
      }
      case METHODS.CRON_REMOVE: {
        const p = params as CronRemoveParams;
        if (!this.scheduler.remove(p.jobId)) {
          throw new Error(`unknown cron job: ${p.jobId}`);
        }
        return {};
      }
      case METHODS.MEMORY_QUERY: {
        const p = params as MemoryQueryParams;
        const hits = this.memory.query({
          q: p.q,
          ...(p.k !== undefined ? { k: p.k } : {}),
          ...(p.kinds !== undefined ? { kinds: p.kinds } : {}),
        });
        const result: MemoryQueryResult = { results: hits.map(memoryHitToWire) };
        return result;
      }
      case METHODS.MEMORY_WRITE: {
        const p = params as MemoryWriteParams;
        const fact = this.memory.addFact({
          text: p.text,
          ...(p.tags !== undefined ? { tags: p.tags } : {}),
        });
        const result: MemoryWriteResult = {
          id: fact.id,
          text: fact.text,
          tags: fact.tags,
          createdAt: fact.createdAt,
        };
        return result;
      }
      case METHODS.MEMORY_EPISODES: {
        const result: MemoryEpisodesResult = {
          episodes: this.memory.listEpisodes().map(memoryEpisodeToWire),
        };
        return result;
      }
      case METHODS.MEMORY_FACTS: {
        const result: MemoryFactsResult = {
          facts: this.memory.listFacts().map(memoryFactToWire),
        };
        return result;
      }
      default:
        // Unreachable: paramsValidators gates unknown methods first.
        throw new Error(`unhandled method: ${method}`);
    }
  }

  private requireSession(sessionId: string): CrowSession {
    const session = this.manager.get(sessionId);
    if (!session) {
      throw new Error(`session not found: ${sessionId}`);
    }
    return session;
  }

  /** One crow-listener per session, fanning events out to every attached connection. */
  private ensureSessionSubscription(session: CrowSession): void {
    if (this.sessionSubscriptions.has(session.id)) return;
    let lastText = "";
    const unsubscribe = session.subscribe((event) => {
      this.broadcastSessionEvent(session.id, event);
      if (event.type === "token") lastText += event.text;
      if (event.type === "state" && (event.state === "idle" || event.state === "error")) {
        // Persist the final assistant text as an episode for future sessions.
        const text = lastText || session.getInfo().modelRef;
        try {
          this.memory.addEpisode({
            sessionId: session.id,
            host: os.hostname(),
            text,
            tags: [session.getInfo().approvalMode],
          });
        } catch {
          // Memory write failures must not break session flow.
        }
      }
    });
    this.sessionSubscriptions.set(session.id, unsubscribe);
  }

  private broadcastSessionEvent(sessionId: string, event: CrowSessionEvent): void {
    const notification = mapSessionEvent(sessionId, event);
    const frame = encodeFrame(notification);
    for (const conn of this.connections) {
      if (!conn.attachedSessionIds.has(sessionId)) continue;
      if (conn.ws.readyState === WebSocket.OPEN) {
        conn.ws.send(frame);
      }
    }
  }

  /**
   * Fan a notification out to every connected client. Used by the P4
   * agent/team events, which are global rather than session-scoped.
   */
  private broadcastAll(notification: JsonRpcNotification): void {
    const frame = encodeFrame(notification);
    for (const conn of this.connections) {
      if (conn.ws.readyState === WebSocket.OPEN) {
        conn.ws.send(frame);
      }
    }
  }

  private sendFrame(conn: ConnectionState, frame: JsonRpcFrame): void {
    if (conn.ws.readyState === WebSocket.OPEN) {
      conn.ws.send(encodeFrame(frame));
    }
  }
}

function mapSessionEvent(sessionId: string, event: CrowSessionEvent) {
  switch (event.type) {
    case "token":
      return makeNotification(EVENTS.TOKEN, { sessionId, text: event.text });
    case "thinking":
      return makeNotification(EVENTS.THINKING, { sessionId, text: event.text });
    case "tool_call":
      return makeNotification(EVENTS.TOOL_CALL, {
        sessionId,
        callId: event.callId,
        tool: event.tool,
        args: event.args,
      });
    case "tool_result":
      return makeNotification(EVENTS.TOOL_RESULT, {
        sessionId,
        callId: event.callId,
        tool: event.tool,
        output: event.output,
        isError: event.isError,
      });
    case "state":
      return makeNotification(EVENTS.SESSION_STATE, {
        sessionId,
        state: event.state,
        ...(event.error !== undefined ? { error: event.error } : {}),
      });
  }
}

/** Best-effort id recovery for malformed frames that still carry a usable id. */
function extractId(raw: unknown): RequestId {
  if (typeof raw === "object" && raw !== null) {
    const id = (raw as { id?: unknown }).id;
    if (typeof id === "string" || typeof id === "number") return id;
  }
  return "unknown";
}

function codeForMethodError(message: string): number {
  if (
    message.startsWith("unknown team:") ||
    message.startsWith("unknown workflow:") ||
    message.startsWith("unknown cron job:")
  ) {
    return RPC_ERRORS.INVALID_PARAMS;
  }
  if (message.includes("not found")) return RPC_ERRORS.SESSION_NOT_FOUND;
  if (message.includes("busy")) return RPC_ERRORS.SESSION_BUSY;
  return RPC_ERRORS.INTERNAL_ERROR;
}

function cronJobToWire(job: {
  id: string;
  name: string;
  workflowName: string;
  recurrence: string;
  inputs: Record<string, unknown>;
  createdAt: string;
  lastRunAt?: string;
  nextRunAt: string;
  enabled: boolean;
}): CronJobWire {
  return {
    id: job.id,
    name: job.name,
    workflowName: job.workflowName,
    recurrence: job.recurrence,
    inputs: job.inputs,
    createdAt: job.createdAt,
    ...(job.lastRunAt ? { lastRunAt: job.lastRunAt } : {}),
    nextRunAt: job.nextRunAt,
    enabled: job.enabled,
  };
}

function memoryHitToWire(hit: {
  id: string;
  kind: "episode" | "fact";
  text: string;
  score: number;
  tags: string[];
  createdAt: string;
  sessionId?: string;
  host?: string;
}): MemoryHitWire {
  return {
    id: hit.id,
    kind: hit.kind,
    text: hit.text,
    score: hit.score,
    tags: hit.tags,
    createdAt: hit.createdAt,
    ...(hit.sessionId ? { sessionId: hit.sessionId } : {}),
    ...(hit.host ? { host: hit.host } : {}),
  };
}

function memoryEpisodeToWire(episode: {
  id: string;
  sessionId?: string;
  host?: string;
  text: string;
  tags: string[];
  createdAt: string;
}): MemoryEpisodeWire {
  return {
    id: episode.id,
    ...(episode.sessionId ? { sessionId: episode.sessionId } : {}),
    ...(episode.host ? { host: episode.host } : {}),
    text: episode.text,
    tags: episode.tags,
    createdAt: episode.createdAt,
  };
}

function memoryFactToWire(fact: {
  id: string;
  text: string;
  tags: string[];
  createdAt: string;
}): MemoryFactWire {
  return {
    id: fact.id,
    text: fact.text,
    tags: fact.tags,
    createdAt: fact.createdAt,
  };
}

function workflowInfo(workflow: Workflow): WorkflowInfo {
  return {
    name: workflow.name,
    description: workflow.description,
    cwd: workflow.cwd,
    ...(workflow.allowShell ? { allowShell: true } : {}),
    steps: workflow.steps.map((s) => ({ kind: s.kind, name: s.name })),
  };
}

/** Replace `{{inputs.X}}` placeholders in any string fields of the workflow. */
function renderWorkflowInputs(workflow: Workflow, inputs: Record<string, unknown>): void {
  for (const step of workflow.steps) {
    if (step.kind === "prompt" || step.kind === "a2a") {
      step.prompt = substitute(step.prompt, inputs);
      if (step.systemPrompt) step.systemPrompt = substitute(step.systemPrompt, inputs);
    } else if (step.kind === "shell") {
      step.command = substitute(step.command, inputs);
    }
  }
}

function substitute(text: string, inputs: Record<string, unknown>): string {
  return text.replace(/\{\{inputs\.([a-zA-Z0-9_]+)\}\}/g, (_match, key: string) => {
    const value = inputs[key];
    return value === undefined || value === null ? "" : String(value);
  });
}

/** Built-in workflows plus any user JSON files under `<dataDir>/workflows/`. */
function loadBuiltinWorkflows(dataDir: string): Workflow[] {
  const out: Workflow[] = [
    {
      name: "self-check",
      description: "Spawn a sub-agent that summarizes the repo and shells out `ls -la` against it.",
      cwd: process.cwd(),
      allowShell: true,
      steps: [
        {
          kind: "prompt",
          name: "summarize",
          prompt:
            "List the files in the current directory and produce a one-sentence summary of the project.",
        },
        {
          kind: "shell",
          name: "list",
          command: "ls -la",
          timeoutSeconds: 30,
        },
      ],
    },
  ];
  // User workflows loaded from <dataDir>/workflows/*.json (best-effort, never throw).
  try {
    const dir = path.join(dataDir, "workflows");
    if (!fs.existsSync(dir)) return out;
    for (const entry of fs.readdirSync(dir)) {
      if (!entry.endsWith(".json")) continue;
      try {
        const raw = JSON.parse(fs.readFileSync(path.join(dir, entry), "utf8")) as unknown;
        if (isWorkflow(raw)) out.push(raw);
      } catch {
        // Skip malformed files — the operator can fix them; the daemon never crashes.
      }
    }
  } catch {
    // Best effort.
  }
  return out;
}
