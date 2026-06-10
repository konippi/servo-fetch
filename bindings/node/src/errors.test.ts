import { describe, expect, it } from "vitest";
import {
  CookieError,
  classifyError,
  FetchTimeoutError,
  InvalidUrlError,
  IoError,
  NetworkError,
  SchemaError,
  ServoFetchError,
} from "./errors.js";

describe("classifyError", () => {
  it("maps sysexits exit codes to typed errors", () => {
    expect(classifyError("error: invalid URL 'x'", 64)).toBeInstanceOf(InvalidUrlError);
    expect(classifyError("error: bad schema", 65)).toBeInstanceOf(SchemaError);
    expect(classifyError("error: cookie file not found", 66)).toBeInstanceOf(CookieError);
    expect(classifyError("error: blocked address", 69)).toBeInstanceOf(NetworkError);
    expect(classifyError("error: disk write failed", 74)).toBeInstanceOf(IoError);
    expect(classifyError("error: navigation timed out", 75)).toBeInstanceOf(FetchTimeoutError);
  });

  it("falls back to the base error for unmapped codes", () => {
    const err = classifyError("error: something exploded", 1);
    expect(err).toBeInstanceOf(ServoFetchError);
    expect(err.constructor).toBe(ServoFetchError);
  });

  it("uses the last error: line as the message", () => {
    const err = classifyError("warming up\nerror: boom\n", 70);
    expect(err.message).toBe("boom");
    expect(err.exitCode).toBe(70);
  });
});
