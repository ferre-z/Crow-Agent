import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { MemoryStore } from "./db.ts";

describe("MemoryStore", () => {
  let dir: string;
  let store: MemoryStore;

  beforeEach(() => {
    dir = mkdtempSync(path.join(tmpdir(), "crow-mem-"));
    store = new MemoryStore({ dbPath: path.join(dir, "memory.db") });
  });

  afterEach(() => {
    store.close();
    rmSync(dir, { recursive: true, force: true });
  });

  it("stores and retrieves episodes with FTS5", () => {
    store.addEpisode({
      sessionId: "s1",
      text: "the project uses pnpm and Node 22",
      tags: ["setup"],
    });
    store.addEpisode({ sessionId: "s2", text: "fixed a flaky shell timeout test", tags: ["bug"] });

    const hits = store.query({ q: "pnpm setup" });
    expect(hits).toHaveLength(1);
    expect(hits[0]?.text).toContain("pnpm");
  });

  it("stores facts and mixes them with episodes in query results", () => {
    store.addFact({ text: "Crow is NVIDIA-first", tags: ["preference"] });
    store.addEpisode({ sessionId: "s1", text: "tested crowd on a remote host" });

    const hits = store.query({ q: "NVIDIA" });
    expect(hits.find((h) => h.kind === "fact")).toBeDefined();

    const onlyFacts = store.query({ q: "NVIDIA", kinds: ["fact"] });
    expect(onlyFacts.every((h) => h.kind === "fact")).toBe(true);
  });

  it("returns an empty list for an unsanitizable query", () => {
    store.addFact({ text: "irrelevant" });
    expect(store.query({ q: "!@#$%^&*" })).toEqual([]);
  });

  it("contextBlock returns a 'Relevant memories' block when there are hits", () => {
    store.addFact({ text: "Crow is NVIDIA-first" });
    const block = store.contextBlock("NVIDIA");
    expect(block).toMatch(/^## Relevant memories/);
    expect(block).toContain("NVIDIA");
  });

  it("lists episodes and facts in newest-first order", () => {
    store.addFact({ text: "a" });
    store.addFact({ text: "b" });
    store.addEpisode({ sessionId: "s1", text: "x" });
    const facts = store.listFacts();
    expect(facts).toHaveLength(2);
    const episodes = store.listEpisodes();
    expect(episodes).toHaveLength(1);
  });
});
