/**
 * Crow workflows: declarative sequences of steps executed by the daemon's
 * scheduler or on demand. Steps run in order; each step's `output` flows to
 * the next via templating. For P6 we support three step kinds:
 *
 * - `prompt` — spawn a sub-agent and pass its output to the next step.
 * - `shell` — run a shell command and pass its stdout to the next step.
 *   Disabled by default (must be enabled per workflow), since it runs
 *   unconfined against the workflow cwd.
 * - `a2a` — delegate a step to another daemon via the A2A HTTP surface.
 */

export type WorkflowStepPrompt = {
  kind: "prompt";
  name: string;
  prompt: string;
  systemPrompt?: string;
  tools?: string[];
  model?: string;
};

export type WorkflowStepShell = {
  kind: "shell";
  name: string;
  command: string;
  timeoutSeconds?: number;
};

export type WorkflowStepA2a = {
  kind: "a2a";
  name: string;
  /** A2A base URL of the target daemon. */
  host: string;
  prompt: string;
  systemPrompt?: string;
  tools?: string[];
  model?: string;
};

export type WorkflowStep = WorkflowStepPrompt | WorkflowStepShell | WorkflowStepA2a;

export interface Workflow {
  name: string;
  description: string;
  /** Workflow cwd; the daemon runs steps against it. */
  cwd: string;
  /**
   * When true, `shell` steps are allowed. Workflows loaded from the
   * scheduler default to false unless the daemon operator explicitly
   * allows shell execution.
   */
  allowShell?: boolean;
  steps: WorkflowStep[];
}

/** Minimal structural view of the shape — loaders don't need to import zod. */
export function isWorkflow(value: unknown): value is Workflow {
  if (typeof value !== "object" || value === null) return false;
  const w = value as Record<string, unknown>;
  return (
    typeof w.name === "string" &&
    typeof w.description === "string" &&
    typeof w.cwd === "string" &&
    Array.isArray(w.steps) &&
    w.steps.every(isStep)
  );
}

function isStep(value: unknown): boolean {
  if (typeof value !== "object" || value === null) return false;
  const s = value as Record<string, unknown>;
  if (typeof s.kind !== "string" || typeof s.name !== "string") return false;
  if (s.kind === "prompt") return typeof s.prompt === "string";
  if (s.kind === "shell") return typeof s.command === "string";
  if (s.kind === "a2a") return typeof s.host === "string" && typeof s.prompt === "string";
  return false;
}

export type WorkflowEvent =
  | { state: "step_started"; step: number; name: string; kind: WorkflowStep["kind"] }
  | {
      state: "step_done";
      step: number;
      name: string;
      kind: WorkflowStep["kind"];
      output: string;
    }
  | { state: "error"; step: number; name: string; error: string }
  | { state: "done"; output: string };

export type WorkflowEventListener = (event: WorkflowEvent) => void;

export type RunPrompt = (options: {
  prompt: string;
  cwd: string;
  systemPrompt?: string;
  tools?: string[];
  model?: string;
}) => Promise<{ output: string }>;

export type RunA2a = (
  baseUrl: string,
  options: {
    prompt: string;
    cwd: string;
    systemPrompt?: string;
    tools?: string[];
    model?: string;
  },
) => Promise<{ output: string }>;

export type RunShell = (
  command: string,
  options: { cwd: string; timeoutSeconds?: number },
) => Promise<{ stdout: string; stderr: string; exitCode: number }>;

export interface WorkflowRunnerOptions {
  prompt: RunPrompt;
  shell?: RunShell;
  a2a?: RunA2a;
  /**
   * Maximum characters of a previous step's output to template into the next
   * step's prompt (prevents runaway memory as chains grow).
   */
  contextWindowChars?: number;
}

/**
 * Runs a workflow's steps sequentially, threading the previous step's output
 * into the next via a `{{previous}}` placeholder in the step's text fields.
 * Step output defaults to the last assistant text content (prompt/a2a) or the
 * captured stdout+stderr (shell).
 */
export class WorkflowRunner {
  private readonly opts: WorkflowRunnerOptions;
  private readonly contextWindowChars: number;

  constructor(opts: WorkflowRunnerOptions) {
    this.opts = opts;
    this.contextWindowChars = opts.contextWindowChars ?? 20_000;
  }

  async run(workflow: Workflow, onEvent: WorkflowEventListener): Promise<{ output: string }> {
    if (workflow.steps.some((s) => s.kind === "shell") && !workflow.allowShell) {
      throw new Error(`workflow '${workflow.name}' contains shell steps but allowShell is not set`);
    }
    let previous = "";
    let finalOutput = "";
    for (const [index, step] of workflow.steps.entries()) {
      const stepIndex = index + 1;
      onEvent({ state: "step_started", step: stepIndex, name: step.name, kind: step.kind });
      try {
        let output = "";
        if (step.kind === "prompt") {
          const prompt = renderTemplate(step.prompt, previous);
          ({ output } = await this.opts.prompt({
            prompt,
            cwd: workflow.cwd,
            ...(step.systemPrompt !== undefined ? { systemPrompt: step.systemPrompt } : {}),
            ...(step.tools !== undefined ? { tools: step.tools } : {}),
            ...(step.model !== undefined ? { model: step.model } : {}),
          }));
        } else if (step.kind === "a2a") {
          if (!this.opts.a2a) throw new Error(`step '${step.name}' requires a2a support`);
          const prompt = renderTemplate(step.prompt, previous);
          ({ output } = await this.opts.a2a(step.host, {
            prompt,
            cwd: workflow.cwd,
            ...(step.systemPrompt !== undefined ? { systemPrompt: step.systemPrompt } : {}),
            ...(step.tools !== undefined ? { tools: step.tools } : {}),
            ...(step.model !== undefined ? { model: step.model } : {}),
          }));
        } else if (step.kind === "shell") {
          if (!this.opts.shell) throw new Error(`step '${step.name}' requires shell support`);
          const result = await this.opts.shell(step.command, {
            cwd: workflow.cwd,
            ...(step.timeoutSeconds !== undefined ? { timeoutSeconds: step.timeoutSeconds } : {}),
          });
          if (result.exitCode !== 0) {
            throw new Error(
              `shell exited ${result.exitCode}: ${result.stderr.trim().slice(0, 200)}`,
            );
          }
          output = result.stdout;
        }
        onEvent({ state: "step_done", step: stepIndex, name: step.name, kind: step.kind, output });
        previous = output;
        finalOutput = output;
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        onEvent({ state: "error", step: stepIndex, name: step.name, error: message });
        throw error;
      }
    }
    onEvent({ state: "done", output: finalOutput });
    return { output: finalOutput };
  }
}

function renderTemplate(text: string, previous: string): string {
  if (!text.includes("{{previous}}")) return text;
  const max = 20_000;
  const trimmed = previous.length > max ? `${previous.slice(0, max)}\n... [truncated]` : previous;
  return text.replaceAll("{{previous}}", trimmed);
}
