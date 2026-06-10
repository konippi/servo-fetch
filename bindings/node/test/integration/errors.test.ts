import { describe, expect, it } from "vitest";
import { onWindows, useFakeBinary } from "../helpers/fake-binary.js";

describe.skipIf(onWindows)("error propagation", () => {
  useFakeBinary();

  it("maps a sysexits exit code to a typed error", async () => {
    const { fetch, InvalidUrlError } = await import("../../src/index.js");
    await expect(fetch("https://failurl.example")).rejects.toBeInstanceOf(InvalidUrlError);
  });

  it("wraps non-JSON output in EngineError", async () => {
    const { extract, EngineError } = await import("../../src/index.js");
    await expect(extract("https://badjson.example")).rejects.toBeInstanceOf(EngineError);
  });
});
