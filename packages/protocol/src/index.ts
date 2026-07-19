/**
 * @crow/protocol — shared wire types for Crow.
 *
 * JSON-RPC 2.0 over newline-delimited WebSocket frames between the desktop
 * hub and per-host daemons, plus A2A (daemon-to-daemon) types in later
 * phases. See docs/protocol.md for the human-readable spec.
 */
export const PROTOCOL_VERSION = "0.1.0" as const;

export * from "./jsonrpc.ts";
export * from "./methods.ts";
export * from "./events.ts";
