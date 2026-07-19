/**
 * @crow/core — Crow agent runtime built on the pi SDK
 * (@earendil-works/pi-ai + @earendil-works/pi-agent-core).
 *
 * Owns: confined execution env, default coding tools, model registry,
 * session factory/manager, skill loading. See packages/daemon for the wire API.
 */
export const CORE_VERSION = "0.1.0" as const;

export { ConfinedExecutionEnv } from "./env/confined-env.ts";
export {
  createBashTool,
  createCodingTools,
  createEditTool,
  createReadTool,
  createWriteTool,
} from "./tools/index.ts";
export {
  createCrowModels,
  DEFAULT_MODEL_REF,
  parseModelRef,
  resolveModelRef,
  type ParsedModelRef,
} from "./models.ts";
export { buildSystemPrompt, loadCrowSkills } from "./skills.ts";
export {
  CrowSession,
  CrowSessionManager,
  type CreateSessionOptions,
  type CrowSessionEvent,
  type CrowSessionInfo,
  type CrowSessionListener,
  type CrowSessionManagerOptions,
  type CrowSessionState,
} from "./session.ts";
// Re-exported for @crow/daemon, which intentionally has no direct pi-ai dependency.
export type { Models } from "@earendil-works/pi-ai";
export * as testing from "./testing/faux.ts";
