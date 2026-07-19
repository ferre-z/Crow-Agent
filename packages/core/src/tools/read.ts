import { Type, type Static } from "@earendil-works/pi-ai";
import type { AgentTool, ExecutionEnv } from "@earendil-works/pi-agent-core";

const parameters = Type.Object({
  path: Type.String({ description: "File path, absolute or relative to the working directory" }),
  maxLines: Type.Optional(
    Type.Number({ description: "Read at most this many lines from the start of the file" }),
  ),
});
type ReadArgs = Static<typeof parameters>;

export function createReadTool(env: ExecutionEnv): AgentTool {
  return {
    name: "read",
    label: "Read file",
    description:
      "Read a UTF-8 text file. Returns the whole file, or the first maxLines lines when given.",
    parameters,
    async execute(_toolCallId, rawArgs, signal) {
      // Args are validated against `parameters` by the agent loop before execute runs.
      const args = rawArgs as ReadArgs;
      if (args.maxLines !== undefined) {
        const result = await env.readTextLines(args.path, {
          maxLines: args.maxLines,
          abortSignal: signal,
        });
        if (!result.ok) throw result.error;
        const text = result.value.join("\n");
        return {
          content: [{ type: "text" as const, text }],
          details: { path: args.path, sizeBytes: Buffer.byteLength(text) },
        };
      }
      const result = await env.readTextFile(args.path, signal);
      if (!result.ok) throw result.error;
      return {
        content: [{ type: "text" as const, text: result.value }],
        details: { path: args.path, sizeBytes: Buffer.byteLength(result.value) },
      };
    },
  };
}
