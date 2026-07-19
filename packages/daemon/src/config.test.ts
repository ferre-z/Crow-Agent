import { mkdtemp, rm, stat, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";

import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { loadOrCreateDaemonConfig } from "./config.ts";

describe("loadOrCreateDaemonConfig", () => {
  let dataDir: string;

  beforeEach(async () => {
    dataDir = await mkdtemp(path.join(os.tmpdir(), "crowd-config-"));
  });

  afterEach(async () => {
    await rm(dataDir, { recursive: true, force: true });
  });

  it("creates daemon.json with a fresh token and mode 0600 on first run", async () => {
    const config = loadOrCreateDaemonConfig(dataDir);
    expect(config.version).toBe(1);
    expect(config.token).toMatch(/^[0-9a-f]{64}$/);
    expect(config.port).toBe(7749);
    expect(config.host).toBe("127.0.0.1");

    const mode = (await stat(path.join(dataDir, "daemon.json"))).mode & 0o777;
    expect(mode).toBe(0o600);
  });

  it("reloads the same config on subsequent runs", async () => {
    const first = loadOrCreateDaemonConfig(dataDir);
    const second = loadOrCreateDaemonConfig(dataDir);
    expect(second).toEqual(first);
  });

  it("throws on a corrupt config instead of rotating it", async () => {
    await writeFile(path.join(dataDir, "daemon.json"), "not json", "utf8");
    expect(() => loadOrCreateDaemonConfig(dataDir)).toThrow(/corrupt daemon config/);

    await writeFile(path.join(dataDir, "daemon.json"), JSON.stringify({ version: 2 }), "utf8");
    expect(() => loadOrCreateDaemonConfig(dataDir)).toThrow(/corrupt daemon config/);
  });
});
