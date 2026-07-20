#!/usr/bin/env node
import os from "node:os";
import path from "node:path";

import { DEFAULT_MODEL_REF } from "@crow/core";

import { loadOrCreateDaemonConfig } from "./config.ts";
import { CrowDaemon } from "./server.ts";

interface CliArgs {
  port?: number;
  host?: string;
  dataDir?: string;
  model?: string;
  token?: string;
  a2aPort?: number;
  a2aHost?: string;
  publicBaseUrl?: string;
  skillDir: string[];
}

const USAGE = `crowd — the Crow per-host daemon

Usage: crowd [options]

Options:
  --port N              listen port (default: from daemon.json, initially 7749)
  --host ADDR           listen address (default: from daemon.json, initially 127.0.0.1)
  --data-dir PATH       state directory (default: ~/.crow)
  --model REF           default model ref "provider/modelId" (default: ${DEFAULT_MODEL_REF})
  --token TOKEN         override the auth token from daemon.json (dev/test use)
  --a2a-port N          also serve the A2A HTTP surface on this port (P5)
  --a2a-host ADDR       bind the A2A HTTP surface to this address (default: same as --host)
  --public-base-url URL advertise this A2A endpoint in host.info (e.g. when behind a proxy)
  --skill-dir PATH      extra directory scanned for skills on every session (repeatable)
  --help                show this help
`;

function parseArgs(argv: string[]): CliArgs {
  const args: CliArgs = { skillDir: [] };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === undefined) break;
    const value = (): string => {
      const v = argv[++i];
      if (v === undefined) throw new Error(`missing value for ${arg}`);
      return v;
    };
    switch (arg) {
      case "--port": {
        const n = Number(value());
        if (!Number.isInteger(n) || n <= 0 || n > 65535) {
          throw new Error(`invalid --port: must be an integer in 1..65535`);
        }
        args.port = n;
        break;
      }
      case "--host":
        args.host = value();
        break;
      case "--data-dir":
        args.dataDir = value();
        break;
      case "--model":
        args.model = value();
        break;
      case "--token":
        args.token = value();
        break;
      case "--a2a-port": {
        const n = Number(value());
        if (!Number.isInteger(n) || n <= 0 || n > 65535) {
          throw new Error(`invalid --a2a-port: must be an integer in 1..65535`);
        }
        args.a2aPort = n;
        break;
      }
      case "--a2a-host":
        args.a2aHost = value();
        break;
      case "--public-base-url":
        args.publicBaseUrl = value();
        break;
      case "--skill-dir":
        args.skillDir.push(value());
        break;
      case "--help":
      case "-h":
        process.stdout.write(USAGE);
        process.exit(0);
        break;
      default:
        throw new Error(`unknown argument: ${arg}`);
    }
  }
  return args;
}

async function main(): Promise<void> {
  const args = parseArgs(process.argv.slice(2));
  const dataDir = args.dataDir ?? path.join(os.homedir(), ".crow");
  const config = loadOrCreateDaemonConfig(dataDir);
  const host = args.host ?? config.host;
  const daemon = new CrowDaemon({
    host,
    port: args.port ?? config.port,
    token: args.token ?? config.token,
    dataDir,
    defaultModelRef: args.model ?? DEFAULT_MODEL_REF,
    ...(args.a2aPort !== undefined || args.a2aHost !== undefined || args.publicBaseUrl
      ? {
          a2a: {
            ...(args.a2aPort !== undefined ? { port: args.a2aPort } : {}),
            ...(args.a2aHost !== undefined ? { host: args.a2aHost } : {}),
            ...(args.publicBaseUrl !== undefined ? { publicBaseUrl: args.publicBaseUrl } : {}),
          },
        }
      : {}),
    ...(args.skillDir.length > 0 ? { defaultSkillDirs: args.skillDir } : {}),
  });
  const { port } = await daemon.start();
  // Never log the token.
  console.log(`crowd listening on ${host}:${port} (data dir: ${dataDir})`);

  let stopping = false;
  const shutdown = () => {
    if (stopping) return;
    stopping = true;
    daemon
      .stop()
      .then(() => process.exit(0))
      .catch((error: unknown) => {
        console.error(`crowd shutdown failed: ${error instanceof Error ? error.message : error}`);
        process.exit(1);
      });
  };
  process.on("SIGINT", shutdown);
  process.on("SIGTERM", shutdown);
}

main().catch((error: unknown) => {
  console.error(`crowd: ${error instanceof Error ? error.message : String(error)}`);
  process.exit(1);
});
