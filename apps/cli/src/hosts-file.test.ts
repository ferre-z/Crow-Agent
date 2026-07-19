import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { loadHostsFile, removeHost, saveHostsFile, upsertHost } from "./hosts-file.js";

describe("hosts-file", () => {
  let tmpDir: string;
  let filePath: string;

  beforeEach(async () => {
    tmpDir = await fs.mkdtemp(path.join(os.tmpdir(), "crow-cli-"));
    filePath = path.join(tmpDir, "hosts.json");
  });

  afterEach(async () => {
    await fs.rm(tmpDir, { recursive: true, force: true });
  });

  it("returns empty array when file is missing", async () => {
    const file = await loadHostsFile(filePath);
    expect(file.hosts).toEqual([]);
  });

  it("round-trips hosts and sets mode 0600", async () => {
    const host = { name: "local", url: "ws://127.0.0.1:7749", token: "secret" };
    await saveHostsFile({ hosts: [host] }, filePath);
    const loaded = await loadHostsFile(filePath);
    expect(loaded.hosts).toEqual([host]);

    const stat = await fs.stat(filePath);
    expect((stat.mode & 0o777).toString(8)).toBe("600");
  });

  it("upserts and sorts by name", async () => {
    let hosts = upsertHost([], { name: "beta", url: "ws://beta", token: "t" });
    hosts = upsertHost(hosts, { name: "alpha", url: "ws://alpha", token: "t" });
    hosts = upsertHost(hosts, { name: "beta", url: "ws://beta2", token: "t2" });
    expect(hosts.map((h) => h.name)).toEqual(["alpha", "beta"]);
    expect(hosts[1]!.url).toBe("ws://beta2");
  });

  it("removes hosts", () => {
    const hosts = [
      { name: "a", url: "ws://a", token: "t" },
      { name: "b", url: "ws://b", token: "t" },
    ];
    expect(removeHost(hosts, "a").map((h) => h.name)).toEqual(["b"]);
  });

  it("throws on corrupt JSON", async () => {
    await fs.writeFile(filePath, "not json", "utf8");
    await expect(loadHostsFile(filePath)).rejects.toThrow("corrupt hosts file");
  });

  it("throws on invalid shape", async () => {
    await fs.writeFile(filePath, JSON.stringify({ hosts: [{ name: "x" }] }), "utf8");
    await expect(loadHostsFile(filePath)).rejects.toThrow("corrupt hosts file");
  });
});
