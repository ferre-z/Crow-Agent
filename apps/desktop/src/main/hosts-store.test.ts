import { mkdtemp, readFile, rm, stat, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";

import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { loadHosts, removeHost, saveHosts, upsertHost } from "./hosts-store.ts";

describe("hosts-store", () => {
  let tmp: string;
  let file: string;

  beforeEach(async () => {
    tmp = await mkdtemp(path.join(os.tmpdir(), "crow-hosts-test-"));
    file = path.join(tmp, "userData", "hosts.json");
  });

  afterEach(async () => {
    await rm(tmp, { recursive: true, force: true });
  });

  it("returns an empty list when the file does not exist", async () => {
    expect(await loadHosts(file)).toEqual([]);
  });

  it("round-trips hosts through save and load", async () => {
    const hosts = [
      { name: "local", url: "ws://127.0.0.1:7749", token: "secret-a" },
      { name: "pi", url: "ws://192.168.1.20:7749", token: "secret-b" },
    ];
    await saveHosts(file, hosts);
    expect(await loadHosts(file)).toEqual(hosts);
  });

  it("creates the parent directory on save", async () => {
    await saveHosts(file, []);
    expect(await readFile(file, "utf8")).toBe("[]\n");
  });

  it("writes the file mode 0600 (it contains tokens)", async () => {
    await saveHosts(file, [{ name: "local", url: "ws://127.0.0.1:7749", token: "t" }]);
    const info = await stat(file);
    expect(info.mode & 0o777).toBe(0o600);
  });

  it("returns an empty list for corrupt content", async () => {
    await saveHosts(file, []); // ensures the parent dir exists
    await writeFile(file, "{ not json", "utf8");
    expect(await loadHosts(file)).toEqual([]);

    await writeFile(file, JSON.stringify([{ name: "x" }]), "utf8"); // schema mismatch
    expect(await loadHosts(file)).toEqual([]);
  });

  it("upsertHost replaces an existing host with the same name", () => {
    const a = { name: "local", url: "ws://a:1", token: "t1" };
    const b = { name: "pi", url: "ws://b:1", token: "t2" };
    const a2 = { name: "local", url: "ws://a:2", token: "t3" };
    expect(upsertHost([a, b], a2)).toEqual([b, a2]);
    expect(upsertHost([], a)).toEqual([a]);
  });

  it("removeHost drops by name and ignores unknown names", () => {
    const a = { name: "local", url: "ws://a:1", token: "t1" };
    const b = { name: "pi", url: "ws://b:1", token: "t2" };
    expect(removeHost([a, b], "local")).toEqual([b]);
    expect(removeHost([a], "nope")).toEqual([a]);
  });
});
