import { EVENTS } from "@crow/protocol";

function dim(s: string): string {
  return process.stdout.isTTY ? `\x1b[2m${s}\x1b[0m` : s;
}

function bold(s: string): string {
  return process.stdout.isTTY ? `\x1b[1m${s}\x1b[0m` : s;
}

function red(s: string): string {
  return process.stdout.isTTY ? `\x1b[31m${s}\x1b[0m` : s;
}

function summarize(value: unknown, max = 80): string {
  const json = JSON.stringify(value);
  if (json.length <= max) return json;
  return `${json.slice(0, max - 3)}...`;
}

function firstLines(text: string, lines = 2): string {
  const head = text.split("\n").slice(0, lines).join("\n");
  const hasMore = text.split("\n").length > lines;
  return hasMore ? `${head}\n...` : head;
}

export function renderEvent(method: string, params: unknown): string | null {
  switch (method) {
    case EVENTS.TOKEN: {
      const p = params as { text: string };
      return p.text;
    }
    case EVENTS.THINKING: {
      const p = params as { text: string };
      return dim(`[thinking ${p.text}]`);
    }
    case EVENTS.TOOL_CALL: {
      const p = params as { tool: string; args: unknown };
      return `${bold("→")} ${p.tool}(${summarize(p.args)})`;
    }
    case EVENTS.TOOL_RESULT: {
      const p = params as { tool: string; output: string; isError: boolean };
      const prefix = p.isError ? red(`${bold("←")} ${p.tool} [error]`) : `${bold("←")} ${p.tool}`;
      return `${prefix}\n${dim(firstLines(p.output))}`;
    }
    case EVENTS.SESSION_STATE: {
      const p = params as { state: string; error?: string };
      if (p.state === "error" && p.error) {
        return red(`[session error: ${p.error}]`);
      }
      return dim(`[state ${p.state}]`);
    }
    case EVENTS.APPROVAL_REQUEST: {
      const p = params as { approvalId: string; tool: string };
      return dim(`[approval requested] ${p.approvalId} (${p.tool}) — use desktop app to respond`);
    }
    default:
      return null;
  }
}
