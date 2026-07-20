import http from "node:http";
import { randomUUID } from "node:crypto";

import { agentSpawnParamsSchema, PROTOCOL_VERSION, type AgentSpawnParams } from "@crow/protocol";
import { resolveModelRef, type Models } from "@crow/core";

import type { SubAgentRunner } from "@crow/core";

export interface A2aServerOptions {
  host: string;
  port: number;
  token: string;
  models: Models;
  subAgents: SubAgentRunner;
  /** Public URL of this server (advertised in the agent card). */
  publicBaseUrl?: string;
  /** How long the client should poll before giving up on a task. */
  pollDeadlineMs?: number;
}

interface TaskRecord {
  taskId: string;
  state: "running" | "done" | "error";
  output?: string;
  error?: string;
  startedAt: number;
}

function jsonResponse(res: http.ServerResponse, status: number, body: unknown): void {
  res.statusCode = status;
  res.setHeader("content-type", "application/json");
  res.end(JSON.stringify(body));
}

function authorized(req: http.IncomingMessage, token: string): boolean {
  return req.headers.authorization === `Bearer ${token}`;
}

/**
 * Minimal A2A HTTP surface so one Crow daemon can delegate a sub-agent task
 * to another. Single-tenant (same token as the WS API). Polling only — no
 * push callbacks; keep it boring.
 */
export class CrowA2aServer {
  private readonly options: A2aServerOptions;
  private readonly tasks = new Map<string, TaskRecord>();
  private server: http.Server | undefined;
  private actualPort: number | undefined;

  constructor(options: A2aServerOptions) {
    this.options = options;
  }

  start(): Promise<{ port: number }> {
    return new Promise((resolve, reject) => {
      const server = http.createServer((req, res) => {
        void this.handle(req, res);
      });
      server.on("error", reject);
      server.on("listening", () => {
        this.server = server;
        const address = server.address();
        const port =
          typeof address === "object" && address !== null ? address.port : this.options.port;
        this.actualPort = port;
        resolve({ port });
      });
      server.listen(this.options.port, this.options.host);
    });
  }

  /** Public base URL of this server once it's listening (port 0 resolves to the real port). */
  baseUrl(): string {
    if (this.options.publicBaseUrl) return this.options.publicBaseUrl.replace(/\/+$/, "");
    const port = this.actualPort ?? this.options.port;
    return `http://${this.options.host}:${port}`;
  }

  async stop(): Promise<void> {
    const server = this.server;
    this.server = undefined;
    if (!server) return;
    await new Promise<void>((resolve) => server.close(() => resolve()));
  }

  private async handle(req: http.IncomingMessage, res: http.ServerResponse): Promise<void> {
    if (!authorized(req, this.options.token)) {
      res.statusCode = 401;
      res.setHeader("www-authenticate", "Bearer");
      res.end("unauthorized");
      return;
    }
    const url = new URL(req.url ?? "/", `http://${req.headers.host ?? "localhost"}`);
    const path = url.pathname.replace(/\/+$/, "") || "/";

    if (req.method === "GET" && path === "/.well-known/agent.json") {
      const body = await this.agentCard();
      jsonResponse(res, 200, body);
      return;
    }

    if (req.method === "POST" && path === "/a2a/tasks") {
      await this.createTask(req, res);
      return;
    }

    const taskMatch = path.match(/^\/a2a\/tasks\/([^/]+)$/);
    if (req.method === "GET" && taskMatch) {
      this.getTask(res, taskMatch[1]!);
      return;
    }

    if (req.method === "GET" && path === "/healthz") {
      jsonResponse(res, 200, { ok: true });
      return;
    }

    res.statusCode = 404;
    res.end("not found");
  }

  private async agentCard(): Promise<unknown> {
    const available = (await this.options.models.getAvailable?.()) ?? [];
    const models: string[] = [];
    for (const m of available) {
      models.push(`${m.provider}/${m.id}`);
    }
    return {
      name: "crow",
      version: PROTOCOL_VERSION,
      capabilities: { agentSpawn: true, teamRun: true, a2a: true },
      models,
      endpoint: `${this.baseUrl()}/a2a/tasks`,
    };
  }

  private async createTask(req: http.IncomingMessage, res: http.ServerResponse): Promise<void> {
    let body = "";
    for await (const chunk of req) {
      body += chunk;
      if (body.length > 1_000_000) {
        res.statusCode = 413;
        res.end("payload too large");
        return;
      }
    }
    let raw: unknown;
    try {
      raw = JSON.parse(body || "{}");
    } catch {
      jsonResponse(res, 400, { error: "invalid JSON" });
      return;
    }
    const parsed = agentSpawnParamsSchema.safeParse(raw);
    if (!parsed.success) {
      jsonResponse(res, 400, {
        error: "invalid params",
        issues: parsed.error.issues.map((i) => i.message),
      });
      return;
    }
    const params = parsed.data as AgentSpawnParams;
    const taskId = `task_${randomUUID()}`;
    const record: TaskRecord = { taskId, state: "running", startedAt: Date.now() };
    this.tasks.set(taskId, record);

    // Resolve the model against this daemon's available models; if missing,
    // the spawn below will fail with a clear error.
    if (params.model) {
      const resolved = resolveModelRef(this.options.models, params.model);
      if (!resolved) {
        record.state = "error";
        record.error = `unknown model: ${params.model}`;
        jsonResponse(res, 201, { taskId, state: record.state, error: record.error });
        return;
      }
    }

    // Run in the background; client polls GET /a2a/tasks/:taskId.
    void this.options.subAgents
      .spawn({
        prompt: params.prompt,
        cwd: params.cwd,
        ...(params.systemPrompt !== undefined ? { systemPrompt: params.systemPrompt } : {}),
        ...(params.tools !== undefined ? { tools: params.tools } : {}),
        ...(params.model !== undefined ? { model: params.model } : {}),
      })
      .then(({ done }) => done)
      .then(
        ({ output }) => {
          record.state = "done";
          record.output = output;
        },
        (error: unknown) => {
          record.state = "error";
          record.error = error instanceof Error ? error.message : String(error);
        },
      );

    jsonResponse(res, 201, { taskId, state: record.state });
  }

  private getTask(res: http.ServerResponse, taskId: string): void {
    const record = this.tasks.get(taskId);
    if (!record) {
      jsonResponse(res, 404, { error: "unknown task" });
      return;
    }
    jsonResponse(res, 200, {
      taskId: record.taskId,
      state: record.state,
      ...(record.output !== undefined ? { output: record.output } : {}),
      ...(record.error !== undefined ? { error: record.error } : {}),
    });
  }
}
