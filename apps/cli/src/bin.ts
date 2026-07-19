#!/usr/bin/env node
import process from "node:process";

import { CrowClient, CrowClientError } from "@crow/client";
import { METHODS, RPC_ERRORS } from "@crow/protocol";

import {
  loadHostsFile,
  removeHost,
  saveHostsFile,
  type KnownHost,
  upsertHost,
} from "./hosts-file.ts";
import { renderEvent } from "./render.ts";

const USAGE = `Usage: crow <command> [options]

Commands:
  hosts                    list saved hosts
  hosts add <name>         --url <ws://host:port> --token <token>
  hosts remove <name>
  info                     show daemon host info
  sessions                 list daemon sessions
  send <sessionId> <text>  send a prompt (--wait to block until idle)
  prompt <text>            one-shot: create session, send, stream
  cancel <sessionId>       cancel a session
  attach <sessionId>       attach and stream events

Global options:
  --host <name>            use saved host
  --url <ws://...>         ad-hoc daemon URL
  --token <token>          ad-hoc token (requires --url)
  --json                   machine-readable output
  --help                   show this help
`;

type ExitCode = 0 | 1 | 2;

interface ParsedArgs {
  command: string | undefined;
  subcommand: string | undefined;
  positional: string[];
  hostName: string | undefined;
  url: string | undefined;
  token: string | undefined;
  json: boolean;
  help: boolean;
}

function parseArgs(argv: string[]): ParsedArgs {
  const args = argv.slice(2);
  const positional: string[] = [];
  let command: string | undefined;
  let subcommand: string | undefined;
  let hostName: string | undefined;
  let url: string | undefined;
  let token: string | undefined;
  let json = false;
  let help = false;

  for (let i = 0; i < args.length; i++) {
    const arg = args[i];
    if (arg === undefined) continue;
    const next = () => {
      const v = args[++i];
      if (v === undefined) throw new Error(`missing value for ${arg}`);
      return v;
    };
    if (arg === "--help") {
      help = true;
    } else if (arg === "--host" || arg === "-h") {
      hostName = next();
    } else if (arg === "--url") {
      url = next();
    } else if (arg === "--token") {
      token = next();
    } else if (arg === "--json") {
      json = true;
    } else if (arg.startsWith("-")) {
      throw new Error(`unknown flag: ${arg}`);
    } else {
      positional.push(arg);
    }
  }

  if (positional.length > 0) {
    command = positional.shift();
  }
  if (command === "hosts" && positional.length > 0) {
    subcommand = positional.shift();
  }

  return {
    command,
    subcommand,
    positional,
    hostName,
    url,
    token,
    json,
    help,
  };
}

async function resolveHost(
  args: ParsedArgs,
): Promise<{ name: string; url: string; token: string }> {
  if (args.url && args.token) {
    return { name: "(ad-hoc)", url: args.url, token: args.token };
  }
  if (args.url && !args.token) {
    throw new Error("--token is required when --url is used");
  }
  if (!args.hostName) {
    throw new Error("select a host with --host or provide --url and --token");
  }
  const file = await loadHostsFile();
  const host = file.hosts.find((h) => h.name === args.hostName);
  if (!host) {
    throw new Error(`unknown host: ${args.hostName}`);
  }
  return host;
}

function withClient<T>(fn: (client: CrowClient, args: ParsedArgs) => Promise<T>) {
  return async (args: ParsedArgs): Promise<T> => {
    const host = await resolveHost(args);
    const client = new CrowClient({ url: host.url, token: host.token });
    await client.connect();
    try {
      return await fn(client, args);
    } finally {
      await client.close();
    }
  };
}

function printJson(value: unknown): void {
  console.log(JSON.stringify(value, null, 2));
}

async function listHosts(args: ParsedArgs): Promise<ExitCode> {
  const file = await loadHostsFile();
  if (args.json) {
    printJson(file.hosts.map((h) => ({ name: h.name, url: h.url })));
    return 0;
  }
  for (const host of file.hosts) {
    console.log(`${host.name}\t${host.url}`);
  }
  return 0;
}

async function addHost(args: ParsedArgs): Promise<ExitCode> {
  const name = args.positional[0];
  if (!name) throw new Error("name is required");
  if (!args.url) throw new Error("--url is required");
  if (!args.token) throw new Error("--token is required");

  const host: KnownHost = { name, url: args.url, token: args.token };
  const file = await loadHostsFile();
  file.hosts = upsertHost(file.hosts, host);
  await saveHostsFile(file);
  console.log(`added host: ${name}`);
  return 0;
}

async function doRemoveHost(args: ParsedArgs): Promise<ExitCode> {
  const name = args.positional[0];
  if (!name) throw new Error("name is required");
  const file = await loadHostsFile();
  file.hosts = removeHost(file.hosts, name);
  await saveHostsFile(file);
  console.log(`removed host: ${name}`);
  return 0;
}

const showInfo = withClient(async (client) => {
  const info = await client.hostInfo();
  console.log(info.hostname);
  console.log(`  platform: ${info.platform} ${info.arch}`);
  console.log(`  node:     ${info.node}`);
  console.log(`  daemon:   ${info.daemonVersion}`);
  console.log(`  protocol: ${info.protocolVersion}`);
  console.log(`  sessions: ${info.sessions}`);
  return 0;
});

const listSessions = withClient(async (client) => {
  const result = await client.call<{ sessions: unknown[] }>(METHODS.SESSION_LIST, {});
  console.log(JSON.stringify(result, null, 2));
  return 0;
});

async function streamUntilIdle(
  client: CrowClient,
  sessionId: string,
  options: { json?: boolean; wait?: boolean } = {},
): Promise<ExitCode> {
  const events: { method: string; params: unknown }[] = [];
  let state: string | undefined;
  let error: string | undefined;

  const unsubscribe = client.onEvent((method, params) => {
    events.push({ method, params });
    if (typeof params === "object" && params !== null && "sessionId" in params) {
      const p = params as { sessionId: string; state?: string; error?: string };
      if (p.sessionId === sessionId) {
        if (method === "event.session_state") {
          state = p.state;
          error = p.error;
        }
      }
    }
  });

  const cleanup = () => {
    unsubscribe();
  };

  if (options.wait) {
    await new Promise<void>((resolve) => {
      const check = setInterval(() => {
        if (state === "idle" || state === "error") {
          clearInterval(check);
          resolve();
        }
      }, 50);
    });
  }

  cleanup();

  if (options.json) {
    printJson(events);
    return 0;
  }

  for (const { method, params } of events) {
    const rendered = renderEvent(method, params);
    if (rendered !== null) process.stdout.write(rendered);
  }
  process.stdout.write("\n");

  if (state === "error" && error && !error.match(/abort/i)) {
    return 1;
  }
  return 0;
}

const sendPrompt = withClient(async (client, args: ParsedArgs): Promise<ExitCode> => {
  const sessionId = args.positional[0];
  const text = args.positional[1];
  if (!sessionId || !text) throw new Error("usage: crow send <sessionId> <text>");

  await client.call(METHODS.SESSION_ATTACH, { sessionId });
  await client.call(METHODS.SESSION_SEND, { sessionId, text });

  if (args.json) {
    return streamUntilIdle(client, sessionId, { json: true, wait: true });
  }
  return 0;
});

const oneShotPrompt = withClient(async (client, args: ParsedArgs): Promise<ExitCode> => {
  const text = args.positional[0];
  if (!text) throw new Error("usage: crow prompt <text>");
  const cwd = process.cwd();

  const createParams: { cwd: string; model?: string } = { cwd };
  const { sessionId } = await client.call<{ sessionId: string }>(
    METHODS.SESSION_CREATE,
    createParams,
  );
  console.error(`session: ${sessionId}`);

  await client.call(METHODS.SESSION_SEND, { sessionId, text });
  return streamUntilIdle(client, sessionId, { json: args.json, wait: true });
});

const cancelSession = withClient(async (client, args: ParsedArgs): Promise<ExitCode> => {
  const sessionId = args.positional[0];
  if (!sessionId) throw new Error("usage: crow cancel <sessionId>");
  await client.call(METHODS.SESSION_CANCEL, { sessionId });
  return 0;
});

const attachSession = withClient(async (client, args: ParsedArgs): Promise<ExitCode> => {
  const sessionId = args.positional[0];
  if (!sessionId) throw new Error("usage: crow attach <sessionId>");

  await client.call(METHODS.SESSION_ATTACH, { sessionId });
  const unsubscribe = client.onEvent((method, params) => {
    const rendered = renderEvent(method, params);
    if (rendered !== null) process.stdout.write(rendered);
  });

  process.on("SIGINT", async () => {
    unsubscribe();
    await client.close();
    process.exit(0);
  });

  await new Promise(() => {
    // stay attached until SIGINT
  });
  return 0;
});

function isUsageError(error: unknown): error is Error {
  return error instanceof Error && !error.message.includes("JSON-RPC");
}

async function main(): Promise<number> {
  const args = parseArgs(process.argv);

  if (args.help || (!args.command && process.argv.length <= 2)) {
    console.log(USAGE);
    return 0;
  }

  try {
    switch (args.command) {
      case "hosts":
        if (!args.subcommand) return await listHosts(args);
        if (args.subcommand === "add") return await addHost(args);
        if (args.subcommand === "remove") return await doRemoveHost(args);
        throw new Error(`unknown hosts subcommand: ${args.subcommand}`);
      case "info":
        return await showInfo(args);
      case "sessions":
        return await listSessions(args);
      case "send":
        return await sendPrompt(args);
      case "prompt":
        return await oneShotPrompt(args);
      case "cancel":
        return await cancelSession(args);
      case "attach":
        return await attachSession(args);
      default:
        throw new Error(`unknown command: ${args.command}`);
    }
  } catch (error) {
    if (error instanceof CrowClientError) {
      const code = error.code;
      const message = error.message;
      if (code === RPC_ERRORS.UNAUTHORIZED || code === 401 || message.includes("HTTP 401")) {
        console.error("crow: auth failed (check token)");
      } else if (code === -1 || message.includes("cannot reach")) {
        console.error("crow: cannot reach daemon");
      } else {
        console.error(`crow: ${message}`);
      }
      return 1;
    }
    if (isUsageError(error)) {
      console.error(`crow: ${error.message}`);
      console.error("\n" + USAGE);
      return 2;
    }
    console.error(`crow: ${error instanceof Error ? error.message : String(error)}`);
    return 1;
  }
}

void main().then((code) => process.exit(code));
