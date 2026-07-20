import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";

import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { DEFAULT_SUBAGENT_PROMPT, SubAgentRunner } from "./subagents.ts";
import {
  FAUX_MODEL_REF,
  fauxAssistantMessage,
  fauxText,
  fauxToolCall,
  makeFauxModels,
} from "./testing/faux.ts";

describe("SubAgentRunner", () => {
  let tmp: string;
  let workdir: string;
  let sessionsRoot: string;
  let runner: SubAgentRunner | undefined;

  beforeEach(async () => {
    tmp = await mkdtemp(path.join(os.tmpdir(), "crow-subagent-"));
    workdir = path.join(tmp, "work");
    await mkdir(workdir, { recursive: true });
    sessionsRoot = path.join(tmp, "subagent-sessions");
  });

  afterEach(async () => {
    await runner?.shutdown();
    await rm(tmp, { recursive: true, force: true });
  });

  const makeRunner = (models: ReturnType<typeof makeFauxModels>["models"]) =>
    new SubAgentRunner({ sessionsRoot, models, defaultModelRef: FAUX_MODEL_REF });

  it("runs a scripted prompt and resolves with the final text output", async () => {
    const { models, faux } = makeFauxModels();
    runner = makeRunner(models);
    faux.setResponses([fauxAssistantMessage([fauxText("sub-agent result")])]);

    const { agentId, done } = await runner.spawn({ prompt: "do the thing", cwd: workdir });
    expect(agentId).toMatch(/^agent_/);
    await expect(done).resolves.toEqual({ output: "sub-agent result" });
    expect(faux.state.callCount).toBe(1);
  });

  it("executes whitelisted tools against the shared cwd", async () => {
    await writeFile(path.join(workdir, "hello.txt"), "hello from disk");
    const { models, faux } = makeFauxModels();
    runner = makeRunner(models);
    faux.setResponses([
      fauxAssistantMessage([fauxToolCall("read", { path: "hello.txt" })], {
        stopReason: "toolUse",
      }),
      fauxAssistantMessage([fauxText("The file says: hello from disk")]),
    ]);

    const { done } = await runner.spawn({
      prompt: "read the file",
      cwd: workdir,
      tools: ["read"],
    });
    await expect(done).resolves.toEqual({ output: "The file says: hello from disk" });
    expect(faux.state.callCount).toBe(2);
  });

  it("scopes the tool set to the whitelist (asserted via the tools the provider sees)", async () => {
    // Tool scoping is asserted by capturing `context.tools` in a faux response
    // factory: the provider receives exactly the tools the harness exposes, so
    // a missing whitelist entry can never be called by the model.
    const { models, faux } = makeFauxModels();
    runner = makeRunner(models);
    const seenToolNames: string[][] = [];
    const capture = (context: { tools?: { name: string }[] }) => {
      seenToolNames.push((context.tools ?? []).map((tool) => tool.name));
      return fauxAssistantMessage([fauxText("ok")]);
    };
    faux.setResponses([capture, capture]);

    const scoped = await runner.spawn({ prompt: "a", cwd: workdir, tools: ["read"] });
    await scoped.done;
    const full = await runner.spawn({ prompt: "b", cwd: workdir });
    await full.done;

    expect(seenToolNames[0]).toEqual(["read"]);
    expect(seenToolNames[1]).toEqual(["read", "write", "edit", "bash"]);
  });

  it("uses the default system prompt unless one is given", async () => {
    const { models, faux } = makeFauxModels();
    runner = makeRunner(models);
    const seenPrompts: (string | undefined)[] = [];
    const capture = (context: { systemPrompt?: string }) => {
      seenPrompts.push(context.systemPrompt);
      return fauxAssistantMessage([fauxText("ok")]);
    };
    faux.setResponses([capture, capture]);

    const plain = await runner.spawn({ prompt: "a", cwd: workdir });
    await plain.done;
    const custom = await runner.spawn({
      prompt: "b",
      cwd: workdir,
      systemPrompt: "You are a very specific bot.",
    });
    await custom.done;

    expect(seenPrompts[0]).toContain(DEFAULT_SUBAGENT_PROMPT);
    expect(seenPrompts[1]).toContain("You are a very specific bot.");
  });

  it("rejects done with the error message when the run fails", async () => {
    const { models, faux } = makeFauxModels();
    runner = makeRunner(models);
    faux.setResponses([
      fauxAssistantMessage("boom", { stopReason: "error", errorMessage: "kaboom" }),
    ]);

    const { done } = await runner.spawn({ prompt: "go", cwd: workdir });
    await expect(done).rejects.toThrow("kaboom");
  });
});
