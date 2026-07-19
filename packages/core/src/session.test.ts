import { mkdir, mkdtemp, readdir, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { CrowSessionManager, type CrowSessionEvent } from "./session.ts";
import {
  FAUX_MODEL_REF,
  fauxAssistantMessage,
  fauxText,
  fauxToolCall,
  makeFauxModels,
} from "./testing/faux.ts";

describe("CrowSessionManager", () => {
  let tmp: string;
  let workdir: string;
  let sessionsRoot: string;
  let manager: CrowSessionManager | undefined;

  beforeEach(async () => {
    tmp = await mkdtemp(path.join(os.tmpdir(), "crow-session-"));
    workdir = path.join(tmp, "work");
    await mkdir(workdir, { recursive: true });
    sessionsRoot = path.join(tmp, "sessions");
  });

  afterEach(async () => {
    await manager?.shutdown();
    await rm(tmp, { recursive: true, force: true });
  });

  it("streams a scripted tool-call conversation to completion", async () => {
    await writeFile(path.join(workdir, "hello.txt"), "hello from disk");
    const { models, faux } = makeFauxModels();
    manager = new CrowSessionManager({ sessionsRoot, models, defaultModelRef: FAUX_MODEL_REF });
    const session = await manager.create({ cwd: workdir });

    const events: CrowSessionEvent[] = [];
    session.subscribe((e) => events.push(e));

    // First LLM call returns a read tool call; the loop runs the tool against
    // the confined env and makes a second call, which answers with text.
    faux.setResponses([
      fauxAssistantMessage([fauxToolCall("read", { path: "hello.txt" })], {
        stopReason: "toolUse",
      }),
      fauxAssistantMessage([fauxText("The file says: hello from disk")]),
    ]);

    await session.prompt("read the file and tell me what it says");

    const types = events.map((e) => e.type);
    expect(types[0]).toBe("state");
    expect(events[0]).toEqual({ type: "state", state: "streaming" });

    const toolCall = events.find((e) => e.type === "tool_call");
    expect(toolCall).toMatchObject({ tool: "read", args: { path: "hello.txt" } });

    const toolResult = events.find((e) => e.type === "tool_result");
    expect(toolResult).toMatchObject({ tool: "read", isError: false });
    expect(toolResult && toolResult.type === "tool_result" ? toolResult.output : "").toContain(
      "hello from disk",
    );

    const tokens = events
      .filter((e) => e.type === "token")
      .map((e) => (e.type === "token" ? e.text : ""))
      .join("");
    expect(tokens).toContain("The file says: hello from disk");

    const last = events.at(-1);
    expect(last).toEqual({ type: "state", state: "idle" });

    // Order: streaming before tool_call before tool_result before idle.
    const indexOf = (pred: (e: CrowSessionEvent) => boolean) => events.findIndex(pred);
    const iStreaming = indexOf((e) => e.type === "state" && e.state === "streaming");
    const iToolCall = indexOf((e) => e.type === "tool_call");
    const iToolResult = indexOf((e) => e.type === "tool_result");
    const iIdle = events.length - 1;
    expect(iStreaming).toBeGreaterThanOrEqual(0);
    expect(iStreaming).toBeLessThan(iToolCall);
    expect(iToolCall).toBeLessThan(iToolResult);
    expect(iToolResult).toBeLessThan(iIdle);

    expect(session.getInfo().state).toBe("idle");
    expect(faux.state.callCount).toBe(2);
    expect(manager.list()).toHaveLength(1);
    expect(manager.get(session.id)).toBe(session);

    // The pi session log landed under sessionsRoot.
    const files = await readdir(sessionsRoot, { recursive: true });
    expect(files.some((f) => f.endsWith(".jsonl"))).toBe(true);
  });

  it("surfaces a provider error as a state error event", async () => {
    const { models, faux } = makeFauxModels();
    manager = new CrowSessionManager({ sessionsRoot, models, defaultModelRef: FAUX_MODEL_REF });
    const session = await manager.create({ cwd: workdir });

    const events: CrowSessionEvent[] = [];
    session.subscribe((e) => events.push(e));

    faux.setResponses([
      fauxAssistantMessage("boom", { stopReason: "error", errorMessage: "kaboom" }),
    ]);
    await session.prompt("hi");

    const last = events.at(-1);
    expect(last).toEqual({ type: "state", state: "error", error: "kaboom" });
    expect(session.getInfo().state).toBe("error");
  });

  it("rejects a second prompt while streaming with a busy error", async () => {
    const { models, faux } = makeFauxModels({ tokensPerSecond: 20 });
    manager = new CrowSessionManager({ sessionsRoot, models, defaultModelRef: FAUX_MODEL_REF });
    const session = await manager.create({ cwd: workdir });

    faux.setResponses([
      fauxAssistantMessage([fauxText("slow answer ".repeat(40))]),
      fauxAssistantMessage([fauxText("never reached")]),
    ]);

    const first = session.prompt("go");
    await vi.waitFor(() => {
      if (session.getInfo().state !== "streaming") throw new Error("not streaming yet");
    });
    await expect(session.prompt("again")).rejects.toThrow(/busy/);
    await session.cancel();
    await first;
  });

  it("cancels a streaming run and ends in an error state", async () => {
    const { models, faux } = makeFauxModels({ tokensPerSecond: 20 });
    manager = new CrowSessionManager({ sessionsRoot, models, defaultModelRef: FAUX_MODEL_REF });
    const session = await manager.create({ cwd: workdir });

    const events: CrowSessionEvent[] = [];
    session.subscribe((e) => events.push(e));

    faux.setResponses([fauxAssistantMessage([fauxText("word ".repeat(200))])]);

    const run = session.prompt("tell me a story");
    await vi.waitFor(() => {
      if (!events.some((e) => e.type === "token")) throw new Error("no tokens yet");
    });
    await session.cancel();
    await run;

    const last = events.at(-1);
    expect(last).toMatchObject({ type: "state", state: "error" });
    expect(session.getInfo().state).toBe("error");
  });
});
