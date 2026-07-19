import type { AgentTool, ExecutionEnv } from "@earendil-works/pi-agent-core";

import { createBashTool } from "./bash.ts";
import { createEditTool } from "./edit.ts";
import { createReadTool } from "./read.ts";
import { createWriteTool } from "./write.ts";

export { createBashTool } from "./bash.ts";
export { createEditTool } from "./edit.ts";
export { createReadTool } from "./read.ts";
export { createWriteTool } from "./write.ts";

/**
 * Crow's default coding tool set. Pass a confined env so every path the model
 * touches stays inside the session root.
 */
export function createCodingTools(env: ExecutionEnv): AgentTool[] {
  return [createReadTool(env), createWriteTool(env), createEditTool(env), createBashTool(env)];
}
