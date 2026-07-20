import { z } from "zod";

/**
 * Crow daemon methods (client → daemon). P1 set: sessions + host info.
 * P4 adds sub-agents (agent.spawn) and teams (team.list / team.run).
 * Later phases add workflow.* / cron.* (P6), memory.* (P7).
 */
export const METHODS = {
  SESSION_CREATE: "session.create",
  SESSION_SEND: "session.send",
  SESSION_CANCEL: "session.cancel",
  SESSION_LIST: "session.list",
  SESSION_ATTACH: "session.attach",
  HOST_INFO: "host.info",
  AGENT_SPAWN: "agent.spawn",
  TEAM_LIST: "team.list",
  TEAM_RUN: "team.run",
  WORKFLOW_LIST: "workflow.list",
  WORKFLOW_RUN: "workflow.run",
  CRON_ADD: "cron.add",
  CRON_LIST: "cron.list",
  CRON_REMOVE: "cron.remove",
  MEMORY_QUERY: "memory.query",
  MEMORY_WRITE: "memory.write",
  MEMORY_EPISODES: "memory.episodes",
  MEMORY_FACTS: "memory.facts",
} as const;

/** Model reference as "provider/modelId" (e.g. "anthropic/claude-sonnet-4-5"). */
export const modelRefSchema = z
  .string()
  .regex(/^[^/]+\/.+$/, "model ref must be 'provider/modelId'");
export type ModelRef = z.infer<typeof modelRefSchema>;

/**
 * Tool-call approval mode: "auto" executes everything immediately (default,
 * pre-P2 behavior); "ask" pauses each tool call until an attached client
 * answers via `approval.respond` (or the approval times out).
 */
export const approvalModeSchema = z.enum(["auto", "ask"]);
export type ApprovalMode = z.infer<typeof approvalModeSchema>;

// --- session.create ---
export const sessionCreateParamsSchema = z.object({
  cwd: z.string().min(1),
  model: modelRefSchema.optional(),
  systemPrompt: z.string().optional(),
  skillDirs: z.array(z.string()).optional(),
  approvalMode: approvalModeSchema.optional(),
  /** Tool names that never ask, even in "ask" mode. Defaults to []. */
  autoApproveTools: z.array(z.string()).optional(),
});
export type SessionCreateParams = z.infer<typeof sessionCreateParamsSchema>;

export const sessionCreateResultSchema = z.object({
  sessionId: z.string(),
});
export type SessionCreateResult = z.infer<typeof sessionCreateResultSchema>;

// --- session.send ---
export const sessionSendParamsSchema = z.object({
  sessionId: z.string(),
  text: z.string().min(1),
});
export type SessionSendParams = z.infer<typeof sessionSendParamsSchema>;

// --- session.cancel ---
export const sessionCancelParamsSchema = z.object({
  sessionId: z.string(),
});
export type SessionCancelParams = z.infer<typeof sessionCancelParamsSchema>;

// --- session.list ---
export const sessionInfoSchema = z.object({
  id: z.string(),
  cwd: z.string(),
  model: modelRefSchema.nullable(),
  state: z.enum(["idle", "busy"]),
  createdAt: z.string(),
  approvalMode: approvalModeSchema,
});
export type SessionInfo = z.infer<typeof sessionInfoSchema>;

export const sessionListResultSchema = z.object({
  sessions: z.array(sessionInfoSchema),
});
export type SessionListResult = z.infer<typeof sessionListResultSchema>;

// --- session.attach ---
export const sessionAttachParamsSchema = z.object({
  sessionId: z.string(),
  /** Replay events after this ISO timestamp when the daemon has a buffer. */
  since: z.string().optional(),
});
export type SessionAttachParams = z.infer<typeof sessionAttachParamsSchema>;

// --- host.info ---
export const hostInfoResultSchema = z.object({
  hostname: z.string(),
  platform: z.string(),
  arch: z.string(),
  node: z.string(),
  daemonVersion: z.string(),
  protocolVersion: z.string(),
  sessions: z.number(),
  /** P5: A2A base URL when the daemon's HTTP A2A surface is enabled. */
  a2a: z.string().optional(),
});
export type HostInfoResult = z.infer<typeof hostInfoResultSchema>;

// --- agent.spawn (P4) ---

/**
 * Spawn an independent sub-agent run. Returns immediately; completion arrives
 * as an `event.agent` notification broadcast to every connected client.
 * `tools` whitelists names from the default coding set (read/write/edit/bash);
 * absent means the full set.
 */
export const agentSpawnParamsSchema = z.object({
  prompt: z.string().min(1),
  cwd: z.string().min(1),
  systemPrompt: z.string().optional(),
  tools: z.array(z.string()).optional(),
  model: modelRefSchema.optional(),
  /** P5: when set, the daemon delegates to this A2A base URL instead of running locally. */
  host: z.string().url().optional(),
});
export type AgentSpawnParams = z.infer<typeof agentSpawnParamsSchema>;

export const agentSpawnResultSchema = z.object({
  agentId: z.string(),
});
export type AgentSpawnResult = z.infer<typeof agentSpawnResultSchema>;

// --- team.list (P4) ---
export const teamAgentInfoSchema = z.object({
  name: z.string(),
  role: z.string(),
});
export type TeamAgentInfo = z.infer<typeof teamAgentInfoSchema>;

export const teamInfoSchema = z.object({
  name: z.string(),
  description: z.string(),
  agents: z.array(teamAgentInfoSchema),
});
export type TeamInfo = z.infer<typeof teamInfoSchema>;

export const teamListResultSchema = z.object({
  teams: z.array(teamInfoSchema),
});
export type TeamListResult = z.infer<typeof teamListResultSchema>;

// --- team.run (P4) ---

/**
 * Run a named team preset against `input`. Returns immediately; progress
 * arrives as `event.team` notifications broadcast to every connected client.
 */
export const teamRunParamsSchema = z.object({
  team: z.string().min(1),
  input: z.string().min(1),
  cwd: z.string().min(1),
  model: modelRefSchema.optional(),
});
export type TeamRunParams = z.infer<typeof teamRunParamsSchema>;

export const teamRunResultSchema = z.object({
  runId: z.string(),
});
export type TeamRunResult = z.infer<typeof teamRunResultSchema>;

// --- workflow.list / workflow.run (P6) ---

export const workflowInfoSchema = z.object({
  name: z.string(),
  description: z.string(),
  cwd: z.string(),
  allowShell: z.boolean().optional(),
  steps: z.array(
    z.object({
      kind: z.enum(["prompt", "shell", "a2a"]),
      name: z.string(),
    }),
  ),
});
export type WorkflowInfo = z.infer<typeof workflowInfoSchema>;

export const workflowListResultSchema = z.object({
  workflows: z.array(workflowInfoSchema),
});
export type WorkflowListResult = z.infer<typeof workflowListResultSchema>;

export const workflowStepInputSchema = z
  .object({
    name: z.string(),
    value: z.unknown(),
  })
  .passthrough();

export const workflowRunParamsSchema = z.object({
  workflow: z.string().min(1),
  inputs: z.record(z.unknown()).optional(),
});
export type WorkflowRunParams = z.infer<typeof workflowRunParamsSchema>;

export const workflowRunResultSchema = z.object({
  runId: z.string(),
});
export type WorkflowRunResult = z.infer<typeof workflowRunResultSchema>;

// --- cron.add / cron.list / cron.remove (P6) ---

export const cronAddParamsSchema = z.object({
  name: z.string().min(1),
  workflow: z.string().min(1),
  recurrence: z.string().min(1),
  inputs: z.record(z.unknown()).optional(),
});
export type CronAddParams = z.infer<typeof cronAddParamsSchema>;

export const cronJobSchema = z.object({
  id: z.string(),
  name: z.string(),
  workflowName: z.string(),
  recurrence: z.string(),
  inputs: z.record(z.unknown()),
  createdAt: z.string(),
  lastRunAt: z.string().optional(),
  nextRunAt: z.string(),
  enabled: z.boolean(),
});
export type CronJobWire = z.infer<typeof cronJobSchema>;

export const cronAddResultSchema = cronJobSchema;
export type CronAddResult = CronJobWire;

export const cronListResultSchema = z.object({ jobs: z.array(cronJobSchema) });
export type CronListResult = z.infer<typeof cronListResultSchema>;

export const cronRemoveParamsSchema = z.object({
  jobId: z.string().min(1),
});
export type CronRemoveParams = z.infer<typeof cronRemoveParamsSchema>;

// --- memory.query / memory.write / memory.episodes / memory.facts (P7) ---

export const memoryQueryParamsSchema = z.object({
  q: z.string().min(1),
  k: z.number().int().min(1).max(50).optional(),
  kinds: z.array(z.enum(["episode", "fact"])).optional(),
});
export type MemoryQueryParams = z.infer<typeof memoryQueryParamsSchema>;

export const memoryHitSchema = z.object({
  id: z.string(),
  kind: z.enum(["episode", "fact"]),
  text: z.string(),
  score: z.number(),
  tags: z.array(z.string()),
  createdAt: z.string(),
  sessionId: z.string().optional(),
  host: z.string().optional(),
});
export type MemoryHitWire = z.infer<typeof memoryHitSchema>;

export const memoryQueryResultSchema = z.object({ results: z.array(memoryHitSchema) });
export type MemoryQueryResult = z.infer<typeof memoryQueryResultSchema>;

export const memoryWriteParamsSchema = z.object({
  text: z.string().min(1),
  tags: z.array(z.string()).optional(),
});
export type MemoryWriteParams = z.infer<typeof memoryWriteParamsSchema>;

export const memoryWriteResultSchema = z.object({
  id: z.string(),
  text: z.string(),
  tags: z.array(z.string()),
  createdAt: z.string(),
});
export type MemoryWriteResult = z.infer<typeof memoryWriteResultSchema>;

export const memoryEpisodeSchema = z.object({
  id: z.string(),
  sessionId: z.string().optional(),
  host: z.string().optional(),
  text: z.string(),
  tags: z.array(z.string()),
  createdAt: z.string(),
});
export type MemoryEpisodeWire = z.infer<typeof memoryEpisodeSchema>;

export const memoryEpisodesResultSchema = z.object({ episodes: z.array(memoryEpisodeSchema) });
export type MemoryEpisodesResult = z.infer<typeof memoryEpisodesResultSchema>;

export const memoryFactSchema = z.object({
  id: z.string(),
  text: z.string(),
  tags: z.array(z.string()),
  createdAt: z.string(),
});
export type MemoryFactWire = z.infer<typeof memoryFactSchema>;

export const memoryFactsResultSchema = z.object({ facts: z.array(memoryFactSchema) });
export type MemoryFactsResult = z.infer<typeof memoryFactsResultSchema>;

/** Params validator per method, for dispatch. */
export const methodParamsSchemas = {
  [METHODS.SESSION_CREATE]: sessionCreateParamsSchema,
  [METHODS.SESSION_SEND]: sessionSendParamsSchema,
  [METHODS.SESSION_CANCEL]: sessionCancelParamsSchema,
  [METHODS.SESSION_LIST]: z.object({}).strict(),
  [METHODS.SESSION_ATTACH]: sessionAttachParamsSchema,
  [METHODS.HOST_INFO]: z.object({}).strict(),
  [METHODS.AGENT_SPAWN]: agentSpawnParamsSchema,
  [METHODS.TEAM_LIST]: z.object({}).strict(),
  [METHODS.TEAM_RUN]: teamRunParamsSchema,
  [METHODS.WORKFLOW_LIST]: z.object({}).strict(),
  [METHODS.WORKFLOW_RUN]: workflowRunParamsSchema,
  [METHODS.CRON_ADD]: cronAddParamsSchema,
  [METHODS.CRON_LIST]: z.object({}).strict(),
  [METHODS.CRON_REMOVE]: cronRemoveParamsSchema,
  [METHODS.MEMORY_QUERY]: memoryQueryParamsSchema,
  [METHODS.MEMORY_WRITE]: memoryWriteParamsSchema,
  [METHODS.MEMORY_EPISODES]: z.object({}).strict(),
  [METHODS.MEMORY_FACTS]: z.object({}).strict(),
} as const;

// --- approval.respond (client → daemon notification, no id, no response) ---

/**
 * Client notifications (client → daemon, no `id`, never answered). Kept out of
 * METHODS/methodParamsSchemas, which drive request dispatch.
 */
export const NOTIFICATIONS = {
  APPROVAL_RESPOND: "approval.respond",
} as const;

export const approvalDecisionSchema = z.enum(["allow", "deny", "always"]);
export type ApprovalDecision = z.infer<typeof approvalDecisionSchema>;

export const approvalRespondParamsSchema = z.object({
  approvalId: z.string().min(1),
  decision: approvalDecisionSchema,
});
export type ApprovalRespondParams = z.infer<typeof approvalRespondParamsSchema>;
