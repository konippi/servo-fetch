import { describe, expect, it } from "vitest";
import { onWindows, useFakeBinary } from "../helpers/fake-binary.js";

describe.skipIf(onWindows)("error propagation", () => {
  useFakeBinary();

  it("maps an RPC error kind to a typed error", async () => {
    const { fetch, InvalidUrlError } = await import("../../src/index.js");
    await expect(fetch("https://failurl.example")).rejects.toBeInstanceOf(InvalidUrlError);
  });

  it("rejects an aborted request with requestCancelled", async () => {
    const { fetch, ServoFetchError } = await import("../../src/index.js");
    const controller = new AbortController();
    controller.abort();
    const err = await fetch("https://e.com", { signal: controller.signal }).catch((e) => e);
    expect(err).toBeInstanceOf(ServoFetchError);
    expect((err as { kind: string }).kind).toBe("requestCancelled");
  });
});
