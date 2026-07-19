import { z } from "zod";

/**
 * Crow daemon methods (client → daemon). P1 set: sessions + host info.
 * Later phases add agent.spawn / team.run (P4), workflow.* / cron.* (P6),
 * memory.* (P7).
 */
export const METHODS = {
  SESSION_CREATE: "session.create",
  SESSION_SEND: "session.send",
  SESSION_CANCEL: "session.cancel",
  SESSION_LIST: "session.list",
  SESSION_ATTACH: "session.attach",
  HOST_INFO: "host.info",
} as const;

/** Model reference as "provider/modelId" (e.g. "anthropic/claude-sonnet-4-5"). */
export const modelRefSchema = z
  .string()
  .regex(/^[^/]+\/.+$/, "model ref must be 'provider/modelId'");
export type ModelRef = z.infer<typeof modelRefSchema>;

// --- session.create ---
export const sessionCreateParamsSchema = z.object({
  cwd: z.string().min(1),
  model: modelRefSchema.optional(),
  systemPrompt: z.string().optional(),
  skillDirs: z.array(z.string()).optional(),
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
});
export type HostInfoResult = z.infer<typeof hostInfoResultSchema>;

/** Params validator per method, for dispatch. */
export const methodParamsSchemas = {
  [METHODS.SESSION_CREATE]: sessionCreateParamsSchema,
  [METHODS.SESSION_SEND]: sessionSendParamsSchema,
  [METHODS.SESSION_CANCEL]: sessionCancelParamsSchema,
  [METHODS.SESSION_LIST]: z.object({}).strict(),
  [METHODS.SESSION_ATTACH]: sessionAttachParamsSchema,
  [METHODS.HOST_INFO]: z.object({}).strict(),
} as const;
