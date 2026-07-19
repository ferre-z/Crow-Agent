import { Type, type Static } from "@earendil-works/pi-ai";
import type { AgentTool, ExecutionEnv } from "@earendil-works/pi-agent-core";

const parameters = Type.Object({
  path: Type.String({ description: "File path, absolute or relative to the working directory" }),
  content: Type.String({ description: "Full content to write; overwrites any existing file" }),
});
type WriteArgs = Static<typeof parameters>;

export function createWriteTool(env: ExecutionEnv): AgentTool {
  return {
    name: "write",
    label: "Write file",
    description: "Create or overwrite a file with the given content.",
    parameters,
    async execute(_toolCallId, rawArgs, signal) {
      const args = rawArgs as WriteArgs;
      const result = await env.writeFile(args.path, args.content, signal);
      if (!result.ok) throw result.error;
      return {
        content: [{ type: "text" as const, text: `wrote ${args.path}` }],
        details: { path: args.path, bytes: Buffer.byteLength(args.content) },
      };
    },
  };
}
