import { randomBytes } from "node:crypto";
import fs from "node:fs";
import path from "node:path";

export interface DaemonConfig {
  version: 1;
  token: string;
  port: number;
  host: string;
  createdAt: string;
}

export const DEFAULT_DAEMON_PORT = 7749;
export const DEFAULT_DAEMON_HOST = "127.0.0.1";

function isDaemonConfig(value: unknown): value is DaemonConfig {
  if (typeof value !== "object" || value === null) return false;
  const v = value as Record<string, unknown>;
  return (
    v.version === 1 &&
    typeof v.token === "string" &&
    v.token.length > 0 &&
    typeof v.port === "number" &&
    Number.isInteger(v.port) &&
    v.port > 0 &&
    v.port <= 65535 &&
    typeof v.host === "string" &&
    v.host.length > 0 &&
    typeof v.createdAt === "string"
  );
}

/**
 * Read `${dataDir}/daemon.json`, creating it (with a fresh 256-bit token,
 * mode 0600) on first run. Throws on a corrupt file rather than silently
 * rotating it — an unexpected config change should be loud.
 *
 * The token is never logged anywhere.
 */
export function loadOrCreateDaemonConfig(dataDir: string): DaemonConfig {
  const file = path.join(dataDir, "daemon.json");
  if (fs.existsSync(file)) {
    let parsed: unknown;
    try {
      parsed = JSON.parse(fs.readFileSync(file, "utf8"));
    } catch (error) {
      throw new Error(`corrupt daemon config at ${file}: ${(error as Error).message}`);
    }
    if (!isDaemonConfig(parsed)) {
      throw new Error(`corrupt daemon config at ${file}: unexpected shape`);
    }
    return parsed;
  }
  const config: DaemonConfig = {
    version: 1,
    token: randomBytes(32).toString("hex"),
    port: DEFAULT_DAEMON_PORT,
    host: DEFAULT_DAEMON_HOST,
    createdAt: new Date().toISOString(),
  };
  fs.mkdirSync(dataDir, { recursive: true });
  fs.writeFileSync(file, JSON.stringify(config, null, 2) + "\n", { mode: 0o600 });
  // writeFile mode only applies to newly created files; chmod defensively.
  fs.chmodSync(file, 0o600);
  return config;
}
