import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";

export interface KnownHost {
  name: string;
  url: string;
  token: string;
}

export interface HostsFile {
  hosts: KnownHost[];
}

function isKnownHost(value: unknown): value is KnownHost {
  return (
    typeof value === "object" &&
    value !== null &&
    "name" in value &&
    typeof (value as Record<string, unknown>).name === "string" &&
    "url" in value &&
    typeof (value as Record<string, unknown>).url === "string" &&
    "token" in value &&
    typeof (value as Record<string, unknown>).token === "string"
  );
}

function validateHostsFile(value: unknown): HostsFile {
  if (
    typeof value !== "object" ||
    value === null ||
    !("hosts" in value) ||
    !Array.isArray((value as Record<string, unknown>).hosts)
  ) {
    throw new Error("invalid shape");
  }
  const hosts = (value as { hosts: unknown[] }).hosts;
  if (!hosts.every(isKnownHost)) {
    throw new Error("invalid host entry");
  }
  return { hosts };
}

function defaultPath(): string {
  if (process.env.CROW_HOSTS_FILE) return process.env.CROW_HOSTS_FILE;
  return path.join(os.homedir(), ".crow", "hosts.json");
}

export async function loadHostsFile(filePath = defaultPath()): Promise<HostsFile> {
  let raw: string;
  try {
    raw = await fs.readFile(filePath, "utf8");
  } catch (error) {
    if (error instanceof Error && "code" in error && error.code === "ENOENT") {
      return { hosts: [] };
    }
    throw error;
  }

  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    throw new Error(`corrupt hosts file: ${filePath}`);
  }

  try {
    return validateHostsFile(parsed);
  } catch {
    throw new Error(`corrupt hosts file: ${filePath}`);
  }
}

export async function saveHostsFile(file: HostsFile, filePath = defaultPath()): Promise<void> {
  await fs.mkdir(path.dirname(filePath), { recursive: true });
  const tmp = `${filePath}.tmp`;
  await fs.writeFile(tmp, JSON.stringify(file, null, 2) + "\n", {
    encoding: "utf8",
    mode: 0o600,
  });
  await fs.rename(tmp, filePath);
  await fs.chmod(filePath, 0o600);
}

export function upsertHost(hosts: KnownHost[], host: KnownHost): KnownHost[] {
  const next = hosts.filter((h) => h.name !== host.name);
  next.push(host);
  return next.sort((a, b) => a.name.localeCompare(b.name));
}

export function removeHost(hosts: KnownHost[], name: string): KnownHost[] {
  return hosts.filter((h) => h.name !== name);
}
