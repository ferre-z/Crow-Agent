import { randomUUID } from "node:crypto";
import fs from "node:fs";
import path from "node:path";

import Database from "better-sqlite3";

/**
 * P6 recurrence strings. Keep the grammar small — anything else throws at
 * parse time. Full cron parsing is a future enhancement.
 *
 *   @every <N>sec|min|hour
 *   @hourly
 *   @daily HH:MM          (24h, UTC)
 */
export type Recurrence =
  | { kind: "every"; intervalMs: number }
  | { kind: "hourly" }
  | { kind: "daily"; hour: number; minute: number };

export interface CronJob {
  id: string;
  name: string;
  workflowName: string;
  recurrence: string;
  inputs: Record<string, unknown>;
  createdAt: string;
  lastRunAt?: string;
  nextRunAt: string;
  enabled: boolean;
}

export type FireCallback = (job: CronJob) => void | Promise<void>;

const DEFAULT_TICK_MS = 1000;

export function parseRecurrence(spec: string): Recurrence {
  const trimmed = spec.trim();
  if (trimmed === "@hourly") return { kind: "hourly" };
  const every = trimmed.match(
    /^@every\s+(\d+)\s*(s|sec|secs|second|seconds|m|min|mins|minute|minutes|h|hr|hrs|hour|hours)?$/i,
  );
  if (every) {
    const n = Number(every[1]);
    const unit = (every[2] ?? "s").toLowerCase();
    let ms: number;
    if (unit === "s" || unit.startsWith("sec")) ms = n * 1000;
    else if (unit === "m" || unit.startsWith("min")) ms = n * 60_000;
    else ms = n * 3_600_000;
    if (ms <= 0 || !Number.isFinite(ms)) throw new Error(`invalid @every interval: ${spec}`);
    return { kind: "every", intervalMs: ms };
  }
  const daily = trimmed.match(/^@daily\s+(\d{1,2}):(\d{2})$/);
  if (daily) {
    const hour = Number(daily[1]);
    const minute = Number(daily[2]);
    if (hour < 0 || hour > 23 || minute < 0 || minute > 59) {
      throw new Error(`invalid @daily time (expected HH:MM 0..23): ${spec}`);
    }
    return { kind: "daily", hour, minute };
  }
  throw new Error(`unsupported recurrence: ${spec} (expected @every, @hourly, @daily HH:MM)`);
}

export function nextRunAt(recurrence: Recurrence, from: Date = new Date()): Date {
  if (recurrence.kind === "every") return new Date(from.getTime() + recurrence.intervalMs);
  if (recurrence.kind === "hourly") {
    const next = new Date(from);
    next.setMinutes(0, 0, 0);
    next.setHours(next.getHours() + 1);
    return next;
  }
  const next = new Date(from);
  next.setHours(recurrence.hour, recurrence.minute, 0, 0);
  if (next.getTime() <= from.getTime()) next.setDate(next.getDate() + 1);
  return next;
}

interface JobRow {
  id: string;
  name: string;
  workflowName: string;
  recurrence: string;
  inputsJson: string;
  createdAt: string;
  lastRunAt: string | null;
  nextRunAt: string;
  enabled: number;
}

function rowToJob(row: JobRow): CronJob {
  return {
    id: row.id,
    name: row.name,
    workflowName: row.workflowName,
    recurrence: row.recurrence,
    inputs: JSON.parse(row.inputsJson) as Record<string, unknown>,
    createdAt: row.createdAt,
    ...(row.lastRunAt ? { lastRunAt: row.lastRunAt } : {}),
    nextRunAt: row.nextRunAt,
    enabled: row.enabled === 1,
  };
}

/**
 * Persistent cron-like scheduler backed by a SQLite file. The daemon owns
 * one instance; the file lives at `<dataDir>/scheduler.db`.
 */
export class CronScheduler {
  private readonly db: Database.Database;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private fireCb: FireCallback | undefined;
  private running = false;
  private loopRunning = false;
  private readonly tickIntervalMs: number;

  constructor(dbPath: string, options: { tickIntervalMs?: number } = {}) {
    // better-sqlite3 creates the file but not the parent directory.
    fs.mkdirSync(path.dirname(dbPath), { recursive: true });
    this.db = new Database(dbPath);
    this.db.pragma("journal_mode = WAL");
    this.db.exec(`
      CREATE TABLE IF NOT EXISTS jobs (
        id TEXT PRIMARY KEY,
        name TEXT NOT NULL,
        workflowName TEXT NOT NULL,
        recurrence TEXT NOT NULL,
        inputsJson TEXT NOT NULL,
        createdAt TEXT NOT NULL,
        lastRunAt TEXT,
        nextRunAt TEXT NOT NULL,
        enabled INTEGER NOT NULL DEFAULT 1
      )
    `);
    this.tickIntervalMs = options.tickIntervalMs ?? DEFAULT_TICK_MS;
  }

  setFireCallback(cb: FireCallback): void {
    this.fireCb = cb;
  }

  add(input: {
    name: string;
    workflowName: string;
    recurrence: string;
    inputs?: Record<string, unknown>;
    runAt?: Date;
  }): CronJob {
    const recurrence = parseRecurrence(input.recurrence);
    const next = input.runAt ?? nextRunAt(recurrence);
    const job: CronJob = {
      id: `job_${randomUUID()}`,
      name: input.name,
      workflowName: input.workflowName,
      recurrence: input.recurrence,
      inputs: input.inputs ?? {},
      createdAt: new Date().toISOString(),
      nextRunAt: next.toISOString(),
      enabled: true,
    };
    this.db
      .prepare(
        `INSERT INTO jobs (id, name, workflowName, recurrence, inputsJson, createdAt, lastRunAt, nextRunAt, enabled)
         VALUES (@id, @name, @workflowName, @recurrence, @inputsJson, @createdAt, NULL, @nextRunAt, 1)`,
      )
      .run({
        id: job.id,
        name: job.name,
        workflowName: job.workflowName,
        recurrence: job.recurrence,
        inputsJson: JSON.stringify(job.inputs),
        createdAt: job.createdAt,
        nextRunAt: job.nextRunAt,
      });
    return job;
  }

  remove(id: string): boolean {
    const result = this.db.prepare("DELETE FROM jobs WHERE id = ?").run(id);
    return result.changes > 0;
  }

  list(): CronJob[] {
    const rows = this.db.prepare("SELECT * FROM jobs ORDER BY createdAt ASC").all() as JobRow[];
    return rows.map(rowToJob);
  }

  get(id: string): CronJob | undefined {
    const row = this.db.prepare("SELECT * FROM jobs WHERE id = ?").get(id) as JobRow | undefined;
    return row ? rowToJob(row) : undefined;
  }

  start(): void {
    if (this.running) return;
    this.running = true;
    this.scheduleNextTick();
  }

  stop(): void {
    this.running = false;
    if (this.timer) {
      clearTimeout(this.timer);
      this.timer = null;
    }
  }

  /** Run one tick immediately and return the jobs that fired. Used by tests. */
  async runOnce(now: Date = new Date()): Promise<CronJob[]> {
    return this.tick(now);
  }

  close(): void {
    this.stop();
    this.db.close();
  }

  private scheduleNextTick(): void {
    if (!this.running) return;
    this.timer = setTimeout(() => {
      void this.tick().finally(() => this.scheduleNextTick());
    }, this.tickIntervalMs);
    // Keep the process alive only while the scheduler is running.
    if (typeof this.timer.unref === "function") this.timer.unref();
  }

  private async tick(now: Date = new Date()): Promise<CronJob[]> {
    if (this.loopRunning) return [];
    this.loopRunning = true;
    try {
      const due = this.db
        .prepare("SELECT * FROM jobs WHERE enabled = 1 AND nextRunAt <= ? ORDER BY nextRunAt ASC")
        .all(now.toISOString()) as JobRow[];
      const fired: CronJob[] = [];
      for (const row of due) {
        const job = rowToJob(row);
        // Schedule the next run BEFORE firing so slow jobs don't pile up.
        const recurrence = parseRecurrence(job.recurrence);
        const nextDate = nextRunAt(recurrence, now);
        this.db
          .prepare("UPDATE jobs SET lastRunAt = ?, nextRunAt = ? WHERE id = ?")
          .run(now.toISOString(), nextDate.toISOString(), job.id);
        job.lastRunAt = now.toISOString();
        job.nextRunAt = nextDate.toISOString();
        if (this.fireCb) {
          try {
            await this.fireCb(job);
          } catch {
            // Errors are the workflow's responsibility; the scheduler keeps
            // running regardless.
          }
        }
        fired.push(job);
      }
      return fired;
    } finally {
      this.loopRunning = false;
    }
  }
}
