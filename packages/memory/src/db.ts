import { randomUUID } from "node:crypto";
import fs from "node:fs";
import path from "node:path";

import Database from "better-sqlite3";

/**
 * Crow memory: persistent SQLite store for session transcripts and explicit
 * facts. FTS5 powers the text search behind `memory.query`.
 *
 * Two tables:
 *   - episodes: one row per session's summary (text + optional session id,
 *     host, tags). Written by the daemon when a session settles.
 *   - facts: short user/operator-curated notes, also FTS-indexed.
 *
 * Embedding-based retrieval is out of scope for P7; ranking is BM25 via FTS5.
 */

export interface MemoryStoreOptions {
  dbPath: string;
  /** Maximum characters of episode/fact text used as a system-prompt excerpt. */
  maxExcerptChars?: number;
}

export interface Episode {
  id: string;
  sessionId?: string;
  host?: string;
  text: string;
  tags: string[];
  createdAt: string;
}

export interface Fact {
  id: string;
  text: string;
  tags: string[];
  createdAt: string;
}

export type MemoryKind = "episode" | "fact";

export interface MemoryHit {
  id: string;
  kind: MemoryKind;
  text: string;
  score: number;
  tags: string[];
  createdAt: string;
  sessionId?: string;
  host?: string;
}

interface EpisodeRow {
  id: string;
  sessionId: string | null;
  host: string | null;
  text: string;
  tags: string;
  createdAt: string;
}

interface FactRow {
  id: string;
  text: string;
  tags: string;
  createdAt: string;
}

function rowToEpisode(row: EpisodeRow): Episode {
  return {
    id: row.id,
    ...(row.sessionId ? { sessionId: row.sessionId } : {}),
    ...(row.host ? { host: row.host } : {}),
    text: row.text,
    tags: row.tags ? (JSON.parse(row.tags) as string[]) : [],
    createdAt: row.createdAt,
  };
}

function rowToFact(row: FactRow): Fact {
  return {
    id: row.id,
    text: row.text,
    tags: row.tags ? (JSON.parse(row.tags) as string[]) : [],
    createdAt: row.createdAt,
  };
}

export class MemoryStore {
  private readonly db: Database.Database;
  private readonly maxExcerptChars: number;

  constructor(options: MemoryStoreOptions) {
    fs.mkdirSync(path.dirname(options.dbPath), { recursive: true });
    this.db = new Database(options.dbPath);
    this.db.pragma("journal_mode = WAL");
    this.db.exec(`
      CREATE TABLE IF NOT EXISTS episodes (
        id TEXT PRIMARY KEY,
        sessionId TEXT,
        host TEXT,
        text TEXT NOT NULL,
        tags TEXT NOT NULL DEFAULT '[]',
        createdAt TEXT NOT NULL
      );
      CREATE VIRTUAL TABLE IF NOT EXISTS episodes_fts USING fts5(
        text, tags, content='episodes', content_rowid='rowid'
      );
      CREATE TABLE IF NOT EXISTS facts (
        id TEXT PRIMARY KEY,
        text TEXT NOT NULL,
        tags TEXT NOT NULL DEFAULT '[]',
        createdAt TEXT NOT NULL
      );
      CREATE VIRTUAL TABLE IF NOT EXISTS facts_fts USING fts5(
        text, tags, content='facts', content_rowid='rowid'
      );
    `);
    this.maxExcerptChars = options.maxExcerptChars ?? 500;
  }

  close(): void {
    this.db.close();
  }

  addEpisode(input: { sessionId?: string; host?: string; text: string; tags?: string[] }): Episode {
    const episode: Episode = {
      id: `ep_${randomUUID()}`,
      ...(input.sessionId ? { sessionId: input.sessionId } : {}),
      ...(input.host ? { host: input.host } : {}),
      text: input.text,
      tags: input.tags ?? [],
      createdAt: new Date().toISOString(),
    };
    const tx = this.db.transaction((e: Episode) => {
      this.db
        .prepare(
          `INSERT INTO episodes (id, sessionId, host, text, tags, createdAt)
           VALUES (@id, @sessionId, @host, @text, @tags, @createdAt)`,
        )
        .run({
          id: e.id,
          sessionId: e.sessionId ?? null,
          host: e.host ?? null,
          text: e.text,
          tags: JSON.stringify(e.tags),
          createdAt: e.createdAt,
        });
      this.db
        .prepare(
          "INSERT INTO episodes_fts (rowid, text, tags) VALUES ((SELECT rowid FROM episodes WHERE id = ?), ?, ?)",
        )
        .run(e.id, e.text, JSON.stringify(e.tags));
    });
    tx(episode);
    return episode;
  }

  addFact(input: { text: string; tags?: string[] }): Fact {
    const fact: Fact = {
      id: `fact_${randomUUID()}`,
      text: input.text,
      tags: input.tags ?? [],
      createdAt: new Date().toISOString(),
    };
    const tx = this.db.transaction((f: Fact) => {
      this.db
        .prepare(
          `INSERT INTO facts (id, text, tags, createdAt) VALUES (@id, @text, @tags, @createdAt)`,
        )
        .run({ id: f.id, text: f.text, tags: JSON.stringify(f.tags), createdAt: f.createdAt });
      this.db
        .prepare(
          "INSERT INTO facts_fts (rowid, text, tags) VALUES ((SELECT rowid FROM facts WHERE id = ?), ?, ?)",
        )
        .run(f.id, f.text, JSON.stringify(f.tags));
    });
    tx(fact);
    return fact;
  }

  query(input: { q: string; k?: number; kinds?: MemoryKind[] }): MemoryHit[] {
    const k = input.k ?? 10;
    const kinds = input.kinds ?? (["episode", "fact"] as MemoryKind[]);
    const ftsQuery = sanitizeFts(input.q);
    if (!ftsQuery) return [];

    const hits: MemoryHit[] = [];

    if (kinds.includes("episode")) {
      const rows = this.db
        .prepare(
          `SELECT e.id, e.sessionId, e.host, e.text, e.tags, e.createdAt, bm25(episodes_fts) AS score
           FROM episodes_fts
           JOIN episodes e ON e.rowid = episodes_fts.rowid
           WHERE episodes_fts MATCH ?
           ORDER BY score ASC
           LIMIT ?`,
        )
        .all(ftsQuery, k) as (EpisodeRow & { score: number })[];
      for (const r of rows) {
        const e = rowToEpisode(r);
        hits.push({
          id: e.id,
          kind: "episode",
          text: e.text,
          score: r.score,
          tags: e.tags,
          createdAt: e.createdAt,
          ...(e.sessionId ? { sessionId: e.sessionId } : {}),
          ...(e.host ? { host: e.host } : {}),
        });
      }
    }

    if (kinds.includes("fact")) {
      const rows = this.db
        .prepare(
          `SELECT f.id, f.text, f.tags, f.createdAt, bm25(facts_fts) AS score
           FROM facts_fts
           JOIN facts f ON f.rowid = facts_fts.rowid
           WHERE facts_fts MATCH ?
           ORDER BY score ASC
           LIMIT ?`,
        )
        .all(ftsQuery, k) as (FactRow & { score: number })[];
      for (const r of rows) {
        const f = rowToFact(r);
        hits.push({
          id: f.id,
          kind: "fact",
          text: f.text,
          score: r.score,
          tags: f.tags,
          createdAt: f.createdAt,
        });
      }
    }

    hits.sort((a, b) => a.score - b.score);
    return hits.slice(0, k);
  }

  contextBlock(query: string, k = 5): string {
    const hits = this.query({ q: query, k });
    if (hits.length === 0) return "";
    const lines: string[] = ["## Relevant memories"];
    for (const hit of hits) {
      const text =
        hit.text.length > this.maxExcerptChars
          ? `${hit.text.slice(0, this.maxExcerptChars)}…`
          : hit.text;
      const tag =
        hit.kind === "fact" ? "fact" : `episode${hit.sessionId ? ` (${hit.sessionId})` : ""}`;
      lines.push(`- [${tag}] ${text}`);
    }
    return lines.join("\n");
  }

  listEpisodes(): Episode[] {
    const rows = this.db
      .prepare("SELECT * FROM episodes ORDER BY createdAt DESC")
      .all() as EpisodeRow[];
    return rows.map(rowToEpisode);
  }

  listFacts(): Fact[] {
    const rows = this.db.prepare("SELECT * FROM facts ORDER BY createdAt DESC").all() as FactRow[];
    return rows.map(rowToFact);
  }
}

function sanitizeFts(q: string): string {
  const cleaned = q.replace(/[^a-zA-Z0-9_\-"\s]/g, " ").trim();
  if (!cleaned) return "";
  // Quote every token: a single bare term like `test` is valid FTS5 but
  // for safety (and to avoid edge cases with the contentless FTS5 schema)
  // we treat every term as a phrase.
  const tokens = cleaned.split(/\s+/).filter(Boolean);
  return tokens.map((t) => `"${t.replace(/"/g, '""')}"`).join(" ");
}
