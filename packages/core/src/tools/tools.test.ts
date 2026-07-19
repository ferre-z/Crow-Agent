import { mkdtemp, realpath, rm } from "node:fs/promises";
import os from "node:os";
import path from "node:path";

import { NodeExecutionEnv } from "@earendil-works/pi-agent-core/node";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { ConfinedExecutionEnv } from "../env/confined-env.ts";
import { createCodingTools } from "./index.ts";

describe("createCodingTools", () => {
  let root: string;
  let env: ConfinedExecutionEnv;
  let tools: ReturnType<typeof createCodingTools>;

  const tool = (name: string) => {
    const found = tools.find((t) => t.name === name);
    if (!found) throw new Error(`tool ${name} not registered`);
    return found;
  };

  beforeEach(async () => {
    root = await mkdtemp(path.join(os.tmpdir(), "crow-tools-"));
    env = new ConfinedExecutionEnv(new NodeExecutionEnv({ cwd: root }), root);
    tools = createCodingTools(env);
  });

  afterEach(async () => {
    await rm(root, { recursive: true, force: true });
  });

  it("round-trips write then read, creating parent directories", async () => {
    await tool("write").execute("c1", { path: "notes/a.txt", content: "hello\nworld\n" });
    const result = await tool("read").execute("c2", { path: "notes/a.txt" });
    expect(result.content).toEqual([{ type: "text", text: "hello\nworld\n" }]);
    expect(result.details).toMatchObject({ path: "notes/a.txt" });
  });

  it("reads only maxLines lines when asked", async () => {
    await tool("write").execute("c1", { path: "b.txt", content: "one\ntwo\nthree\n" });
    const result = await tool("read").execute("c2", { path: "b.txt", maxLines: 2 });
    expect(result.content[0]).toEqual({ type: "text", text: "one\ntwo" });
  });

  it("edits a unique match exactly once", async () => {
    await tool("write").execute("c1", { path: "c.txt", content: "alpha beta gamma" });
    await tool("edit").execute("c2", { path: "c.txt", oldText: "beta", newText: "BETA" });
    const result = await tool("read").execute("c3", { path: "c.txt" });
    expect(result.content[0]).toEqual({ type: "text", text: "alpha BETA gamma" });
  });

  it("rejects edits with no match or an ambiguous match", async () => {
    await tool("write").execute("c1", { path: "d.txt", content: "x x y" });
    await expect(
      tool("edit").execute("c2", { path: "d.txt", oldText: "nope", newText: "z" }),
    ).rejects.toThrow(/no match/);
    await expect(
      tool("edit").execute("c3", { path: "d.txt", oldText: "x", newText: "z" }),
    ).rejects.toThrow(/ambiguous/);
  });

  it("confines reads to the session root", async () => {
    await expect(tool("read").execute("c1", { path: "../../etc/passwd" })).rejects.toThrow(
      /escapes confinement/,
    );
    await expect(tool("read").execute("c2", { path: "/etc/passwd" })).rejects.toThrow(
      /escapes confinement/,
    );
    await expect(
      tool("write").execute("c3", { path: "../escape.txt", content: "x" }),
    ).rejects.toThrow(/escapes confinement/);
  });

  it("confines shell cwd to the session root", async () => {
    // The bash tool never passes cwd, but ConfinedExecutionEnv guards exec cwd
    // for harness-side shell use; exercise the env directly.
    const escaped = await env.exec("pwd", { cwd: "../" });
    expect(escaped.ok).toBe(false);
    if (!escaped.ok) expect(escaped.error.code).toBe("spawn_error");

    const inside = await env.exec("pwd", {});
    expect(inside.ok).toBe(true);
    if (inside.ok) expect(inside.value.stdout.trim()).toBe(await realpath(root));
  });

  it("runs bash and reports stdout, stderr, and exit code", async () => {
    const ok = await tool("bash").execute("c1", { command: "echo out; echo err 1>&2" });
    const text = (ok.content[0] as { text: string }).text;
    expect(text).toContain("out");
    expect(text).toContain("err");
    expect(ok.details).toMatchObject({ exitCode: 0 });

    const failed = await tool("bash").execute("c2", { command: "echo partial; exit 3" });
    const failedText = (failed.content[0] as { text: string }).text;
    expect(failedText).toContain("partial");
    expect(failedText).toContain("(exit 3)");
    expect(failed.details).toMatchObject({ exitCode: 3 });
  });

  it("kills bash commands that exceed their timeout", { timeout: 10000 }, async () => {
    const started = Date.now();
    try {
      await tool("bash").execute("c1", { command: "sleep 5", timeoutSeconds: 1 });
      throw new Error("expected the bash tool to throw on timeout");
    } catch (error) {
      expect(error).toMatchObject({ code: "timeout" });
    }
    expect(Date.now() - started).toBeLessThan(6000);
  });
});
