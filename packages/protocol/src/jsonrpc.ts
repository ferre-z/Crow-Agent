import { z } from "zod";

/** JSON-RPC 2.0 request id: string or number (no null — we never use parse-error responses). */
export const requestIdSchema = z.union([z.string(), z.number()]);
export type RequestId = z.infer<typeof requestIdSchema>;

export const jsonRpcRequestSchema = z.object({
  jsonrpc: z.literal("2.0"),
  id: requestIdSchema,
  method: z.string(),
  params: z.unknown().optional(),
});
export type JsonRpcRequest = z.infer<typeof jsonRpcRequestSchema>;

export const jsonRpcNotificationSchema = z.object({
  jsonrpc: z.literal("2.0"),
  method: z.string(),
  params: z.unknown().optional(),
});
export type JsonRpcNotification = z.infer<typeof jsonRpcNotificationSchema>;

export const jsonRpcErrorSchema = z.object({
  code: z.number(),
  message: z.string(),
  data: z.unknown().optional(),
});
export type JsonRpcError = z.infer<typeof jsonRpcErrorSchema>;

export const jsonRpcResponseSchema = z.object({
  jsonrpc: z.literal("2.0"),
  id: requestIdSchema,
  result: z.unknown().optional(),
  error: jsonRpcErrorSchema.optional(),
});
export type JsonRpcResponse = z.infer<typeof jsonRpcResponseSchema>;

/** Any inbound frame on the wire: request, notification, or response. */
export const jsonRpcFrameSchema = z.union([
  jsonRpcRequestSchema,
  jsonRpcNotificationSchema,
  jsonRpcResponseSchema,
]);
export type JsonRpcFrame = z.infer<typeof jsonRpcFrameSchema>;

/** Standard JSON-RPC error codes plus Crow's own (-32xxx range). */
export const RPC_ERRORS = {
  PARSE_ERROR: -32700,
  INVALID_REQUEST: -32600,
  METHOD_NOT_FOUND: -32601,
  INVALID_PARAMS: -32602,
  INTERNAL_ERROR: -32603,
  UNAUTHORIZED: -32001,
  SESSION_NOT_FOUND: -32002,
  SESSION_BUSY: -32003,
} as const;

export function makeRequest(id: RequestId, method: string, params?: unknown): JsonRpcRequest {
  return { jsonrpc: "2.0", id, method, params };
}

export function makeNotification(method: string, params?: unknown): JsonRpcNotification {
  return { jsonrpc: "2.0", method, params };
}

export function makeResult(id: RequestId, result: unknown): JsonRpcResponse {
  return { jsonrpc: "2.0", id, result };
}

export function makeError(
  id: RequestId,
  code: number,
  message: string,
  data?: unknown,
): JsonRpcResponse {
  return { jsonrpc: "2.0", id, error: { code, message, ...(data !== undefined ? { data } : {}) } };
}

/**
 * Encode a frame for the wire: single-line JSON terminated by \n
 * (newline-delimited JSON over WebSocket text frames).
 */
export function encodeFrame(frame: JsonRpcFrame): string {
  return JSON.stringify(frame) + "\n";
}

/** Decode one NDJSON line into a validated frame. Throws zod errors on malformed input. */
export function decodeFrame(line: string): JsonRpcFrame {
  return jsonRpcFrameSchema.parse(JSON.parse(line));
}
