/**
 * Error type for every failure a CrowClient can surface: transport errors
 * (unreachable host, rejected upgrade, dropped socket), JSON-RPC error
 * responses (with `code` set to the wire code), local param validation, and
 * per-call timeouts. Callers match on `code` (see RPC_ERRORS in
 * @crow/protocol) or on the message for transport failures.
 */
export class CrowClientError extends Error {
  /** JSON-RPC error code when the daemon rejected the call; undefined for transport/local failures. */
  readonly code: number | undefined;
  override readonly cause: unknown;

  constructor(message: string, options: { code?: number; cause?: unknown } = {}) {
    super(message);
    this.name = "CrowClientError";
    this.code = options.code;
    this.cause = options.cause;
  }
}
