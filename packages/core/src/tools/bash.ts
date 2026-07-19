import { Type, type Static } from "@earendil-works/pi-ai";
import type { AgentTool, ExecutionEnv } from "@earendil-works/pi-agent-core";

const DEFAULT_TIMEOUT_SECONDS = 120;

const parameters = Type.Object({
  command: Type.String({ description: "Shell command to execute" }),
  timeoutSeconds: Type.Optional(
    Type.Number({
      description: `Kill the command after this many seconds (default ${DEFAULT_TIMEOUT_SECONDS})`,
    }),
  ),
});
type BashArgs = Static<typeof parameters>;

export function createBashTool(env: ExecutionEnv): AgentTool {
  return {
    name: "bash",
    label: "Run shell command",
    description:
      "Run a shell command and return combined stdout/stderr. A non-zero exit code is reported in the output, not as an error.",
    parameters,
    async execute(_toolCallId, rawArgs, signal) {
      const args = rawArgs as BashArgs;
      const result = await env.exec(args.command, {
        timeout: args.timeoutSeconds ?? DEFAULT_TIMEOUT_SECONDS,
        abortSignal: signal,
      });
      if (!result.ok) throw result.error;
      const { stdout, stderr, exitCode } = result.value;
      let text = stdout;
      if (stderr.length > 0) {
        text += text.length > 0 && !text.endsWith("\n") ? "\n" + stderr : stderr;
      }
      if (exitCode !== 0) {
        text += `\n(exit ${exitCode})`;
      }
      return {
        content: [{ type: "text" as const, text }],
        details: { exitCode },
      };
    },
  };
}
