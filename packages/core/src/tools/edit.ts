import { Type, type Static } from "@earendil-works/pi-ai";
import type { AgentTool, ExecutionEnv } from "@earendil-works/pi-agent-core";

const parameters = Type.Object({
  path: Type.String({ description: "File path, absolute or relative to the working directory" }),
  oldText: Type.String({ description: "Exact text to replace; must occur exactly once" }),
  newText: Type.String({ description: "Replacement text" }),
});
type EditArgs = Static<typeof parameters>;

export function createEditTool(env: ExecutionEnv): AgentTool {
  return {
    name: "edit",
    label: "Edit file",
    description:
      "Replace exact text in a file. Fails when oldText is absent or occurs more than once.",
    parameters,
    async execute(_toolCallId, rawArgs, signal) {
      const args = rawArgs as EditArgs;
      if (args.oldText.length === 0) {
        throw new Error("oldText must not be empty");
      }
      const read = await env.readTextFile(args.path, signal);
      if (!read.ok) throw read.error;
      const occurrences = read.value.split(args.oldText).length - 1;
      if (occurrences === 0) {
        throw new Error(`no match for oldText in ${args.path}`);
      }
      if (occurrences > 1) {
        throw new Error(`ambiguous edit: oldText occurs ${occurrences} times in ${args.path}`);
      }
      const updated = read.value.replace(args.oldText, args.newText);
      const written = await env.writeFile(args.path, updated, signal);
      if (!written.ok) throw written.error;
      return {
        content: [{ type: "text" as const, text: `edited ${args.path}` }],
        details: { path: args.path },
      };
    },
  };
}
