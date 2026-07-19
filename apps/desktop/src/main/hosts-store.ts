import { mkdir, readFile, rename, writeFile } from "node:fs/promises";
import path from "node:path";

import { hostsFileSchema, type KnownHost } from "../shared/hosts.ts";

/**
 * Persistence for the known-hosts list (`userData/hosts.json`).
 *
 * Tokens live in this file in cleartext, so it is written mode 0600. That is
 * acceptable for P2 local development; P8 moves tokens into the OS keychain
 * (keytar) and leaves only names/URLs here.
 *
 * The module is deliberately free of electron imports so it can be unit
 * tested with plain node + a tmp dir.
 */

/** Load the hosts file. A missing or corrupt file yields an empty list. */
export async function loadHosts(filePath: string): Promise<KnownHost[]> {
  let raw: string;
  try {
    raw = await readFile(filePath, "utf8");
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") return [];
    throw error;
  }
  try {
    return hostsFileSchema.parse(JSON.parse(raw));
  } catch {
    // Corrupt or schema-mismatched content: start empty rather than wedge the app.
    return [];
  }
}

/** Save the hosts file atomically (tmp + rename), mode 0600. */
export async function saveHosts(filePath: string, hosts: KnownHost[]): Promise<void> {
  await mkdir(path.dirname(filePath), { recursive: true });
  const tmpPath = `${filePath}.tmp`;
  await writeFile(tmpPath, JSON.stringify(hosts, null, 2) + "\n", {
    encoding: "utf8",
    mode: 0o600,
  });
  await rename(tmpPath, filePath);
}

/** Insert or replace a host by name. Pure. */
export function upsertHost(hosts: KnownHost[], host: KnownHost): KnownHost[] {
  return [...hosts.filter((h) => h.name !== host.name), host];
}

/** Remove a host by name (no-op when absent). Pure. */
export function removeHost(hosts: KnownHost[], name: string): KnownHost[] {
  return hosts.filter((h) => h.name !== name);
}
