/**
 * @crow/daemon — `crowd`, the per-host Crow daemon.
 *
 * WebSocket JSON-RPC control API (bearer-token auth at upgrade), multi-session
 * manager, session event fan-out. Scheduler, A2A, and capabilities land in
 * later phases. See docs/protocol.md for the wire spec.
 */
export { loadOrCreateDaemonConfig, type DaemonConfig } from "./config.ts";
export { CrowDaemon, DAEMON_VERSION, type CrowDaemonOptions } from "./server.ts";
