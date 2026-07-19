/**
 * @crow/memory — custom memory for Crow agents.
 *
 * SQLite-backed: episodic session log, facts store, FTS5 search.
 * Embedding index lives behind an interface and is off by default.
 * Filled in during P7.
 */
export const MEMORY_VERSION = "0.1.0" as const;
