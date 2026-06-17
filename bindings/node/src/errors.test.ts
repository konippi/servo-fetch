import { describe, expect, it } from "vitest";
import {
  EngineError,
  FetchTimeoutError,
  InvalidUrlError,
  NetworkError,
  rpcError,
  ServoFetchError,
} from "./errors.js";

describe("rpcError", () => {
  it("maps error kinds to typed errors", () => {
    expect(rpcError({ code: -32602, message: "x", data: { kind: "invalidUrl" } })).toBeInstanceOf(
      InvalidUrlError,
    );
    expect(rpcError({ code: -32000, message: "x", data: { kind: "timeout" } })).toBeInstanceOf(
      FetchTimeoutError,
    );
    expect(
      rpcError({ code: -32602, message: "x", data: { kind: "addressNotAllowed" } }),
    ).toBeInstanceOf(NetworkError);
    expect(rpcError({ code: -32000, message: "x", data: { kind: "engine" } })).toBeInstanceOf(
      EngineError,
    );
    expect(rpcError({ code: -32000, message: "x", data: { kind: "javascript" } })).toBeInstanceOf(
      EngineError,
    );
  });

  it("falls back to the base error for unmapped kinds", () => {
    const err = rpcError({ code: -32603, message: "boom", data: { kind: "internal" } });
    expect(err.constructor).toBe(ServoFetchError);
    expect(err.message).toBe("boom");
    expect(err.kind).toBe("internal");
    expect(err.code).toBe(-32603);
  });
});
