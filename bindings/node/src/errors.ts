import type { ErrorKind } from "./generated/index.js";

/** Base error for every failure surfaced by the servo-fetch RPC server. */
export class ServoFetchError extends Error {
  /** Stable, surface-independent error category, when known. */
  readonly kind: ErrorKind | null;
  /** JSON-RPC error code, when the failure came from the server. */
  readonly code: number | null;

  constructor(message: string, kind: ErrorKind | null = null, code: number | null = null) {
    super(message);
    this.name = new.target.name;
    this.kind = kind;
    this.code = code;
  }
}

export class InvalidUrlError extends ServoFetchError {}
export class FetchTimeoutError extends ServoFetchError {}
export class NetworkError extends ServoFetchError {}
export class EngineError extends ServoFetchError {}

const BY_KIND: Partial<Record<ErrorKind, typeof ServoFetchError>> = {
  invalidUrl: InvalidUrlError,
  timeout: FetchTimeoutError,
  addressNotAllowed: NetworkError,
  engine: EngineError,
  javascript: EngineError,
};

/** JSON-RPC `error` object as returned by the servo-fetch server. */
export interface RpcErrorBody {
  code: number;
  message: string;
  data?: { kind?: ErrorKind };
}

/** Convert a JSON-RPC error object into the matching typed error. */
export function rpcError(body: RpcErrorBody): ServoFetchError {
  const kind = body.data?.kind ?? null;
  const Ctor = (kind && BY_KIND[kind]) || ServoFetchError;
  return new Ctor(body.message || "servo-fetch request failed", kind, body.code);
}
