/**
 * A2A client used by the daemon to delegate sub-agent tasks to another
 * daemon. Polling only (no push callbacks). Same wire shapes as the server
 * side in a2a-server.ts.
 */

export interface A2aClientOptions {
  baseUrl: string;
  token: string;
  /** Per-poll interval in ms; defaults to 200. */
  pollIntervalMs?: number;
  /** Maximum total wait in ms; defaults to 10 minutes. */
  timeoutMs?: number;
}

interface TaskResponse {
  taskId: string;
  state: "running" | "done" | "error";
  output?: string;
  error?: string;
}

export class CrowA2aClient {
  private readonly baseUrl: string;
  private readonly token: string;
  private readonly pollIntervalMs: number;
  private readonly timeoutMs: number;

  constructor(options: A2aClientOptions) {
    this.baseUrl = options.baseUrl.replace(/\/+$/, "");
    this.token = options.token;
    this.pollIntervalMs = options.pollIntervalMs ?? 200;
    this.timeoutMs = options.timeoutMs ?? 10 * 60 * 1000;
  }

  /**
   * Delegate a task and resolve when it finishes. Throws on transport
   * failure or remote error; the caller maps these to the daemon's own
   * event.agent "error" state.
   */
  async delegate(params: {
    prompt: string;
    cwd: string;
    systemPrompt?: string;
    tools?: string[];
    model?: string;
  }): Promise<{ output: string }> {
    const created = await this.post<TaskResponse>("/a2a/tasks", params);
    const deadline = Date.now() + this.timeoutMs;
    while (Date.now() < deadline) {
      if (created.state === "done") {
        return { output: created.output ?? "" };
      }
      if (created.state === "error") {
        throw new Error(created.error ?? "remote task failed");
      }
      await new Promise((resolve) => setTimeout(resolve, this.pollIntervalMs));
      const polled = await this.get<TaskResponse>(`/a2a/tasks/${created.taskId}`);
      if (polled.state === "done") return { output: polled.output ?? "" };
      if (polled.state === "error") throw new Error(polled.error ?? "remote task failed");
    }
    throw new Error(`A2A delegation timed out (task ${created.taskId})`);
  }

  private async post<T>(path: string, body: unknown): Promise<T> {
    const res = await fetch(this.baseUrl + path, {
      method: "POST",
      headers: {
        authorization: `Bearer ${this.token}`,
        "content-type": "application/json",
      },
      body: JSON.stringify(body),
    });
    if (!res.ok) {
      const text = await res.text().catch(() => "");
      throw new Error(`A2A POST ${path} failed (${res.status}): ${text.slice(0, 200)}`);
    }
    return (await res.json()) as T;
  }

  private async get<T>(path: string): Promise<T> {
    const res = await fetch(this.baseUrl + path, {
      method: "GET",
      headers: { authorization: `Bearer ${this.token}` },
    });
    if (!res.ok) {
      throw new Error(`A2A GET ${path} failed (${res.status})`);
    }
    return (await res.json()) as T;
  }
}
