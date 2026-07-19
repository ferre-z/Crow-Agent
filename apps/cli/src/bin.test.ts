import { spawn } from "node:child_process";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { CrowDaemon } from "@crow/daemon";
import { testing } from "@crow/core";

const CLI = "src/bin.ts";

function runCli(
  args: string[],
  env?: Record<string, string>,
): Promise<{ code: number; stdout: string; stderr: string }> {
  return new Promise((resolve, reject) => {
    const child = spawn("node", [CLI, ...args], {
      cwd: process.cwd(),
      env: { ...process.env, ...env },
    });
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (d) => (stdout += d.toString()));
    child.stderr.on("data", (d) => (stderr += d.toString()));
    child.on("error", reject);
    child.on("close", (code) => resolve({ code: code ?? 1, stdout, stderr }));
  });
}

describe("crow CLI e2e", () => {
  let tmpDir: string;
  let daemon: CrowDaemon;
  let port: number;
  let hostsFile: string;

  beforeEach(async () => {
    tmpDir = await fs.mkdtemp(path.join(os.tmpdir(), "crow-cli-e2e-"));
    hostsFile = path.join(tmpDir, "hosts.json");
    const { models, faux } = testing.makeFauxModels();
    faux.setResponses([testing.fauxAssistantMessage([testing.fauxText("Hello from faux")])]);
    daemon = new CrowDaemon({
      host: "127.0.0.1",
      port: 0,
      token: "test-token",
      dataDir: tmpDir,
      models,
      defaultModelRef: "faux/faux-1",
    });
    const addr = await daemon.start();
    port = addr.port;
  });

  afterEach(async () => {
    await daemon.stop();
    await fs.rm(tmpDir, { recursive: true, force: true });
  });

  it("info with ad-hoc url/token prints host info", async () => {
    const { code, stdout } = await runCli([
      "info",
      "--url",
      `ws://127.0.0.1:${port}`,
      "--token",
      "test-token",
    ]);
    expect(code).toBe(0);
    expect(stdout).toContain("platform:");
    expect(stdout).toContain("sessions:");
  });

  it("bad token exits 1 with clean message", async () => {
    const { code, stderr } = await runCli([
      "info",
      "--url",
      `ws://127.0.0.1:${port}`,
      "--token",
      "wrong",
    ]);
    expect(code).toBe(1);
    expect(stderr).toContain("auth failed");
  });

  it("prompt streams tokens and exits 0", async () => {
    const { code, stdout, stderr } = await runCli(
      ["prompt", "hello", "--url", `ws://127.0.0.1:${port}`, "--token", "test-token"],
      { CROW_HOSTS_FILE: hostsFile },
    );
    expect(code).toBe(0);
    expect(stderr).toMatch(/session: [a-z0-9-]+/);
    expect(stdout).toContain("Hello from faux");
  });

  it("hosts add/list/remove round-trip", async () => {
    const env = { CROW_HOSTS_FILE: hostsFile };
    const add = await runCli(["hosts", "add", "local", "--url", "ws://x", "--token", "t"], env);
    expect(add.code).toBe(0);

    const list = await runCli(["hosts"], env);
    expect(list.stdout).toContain("local\tws://x");

    const rm = await runCli(["hosts", "remove", "local"], env);
    expect(rm.code).toBe(0);
    const list2 = await runCli(["hosts"], env);
    expect(list2.stdout).not.toContain("local");
  });

  it("unknown command exits 2 with usage", async () => {
    const { code, stderr } = await runCli(["nope"]);
    expect(code).toBe(2);
    expect(stderr).toContain("unknown command");
    expect(stderr).toContain("Usage:");
  });
});
