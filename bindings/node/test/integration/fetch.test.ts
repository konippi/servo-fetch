import { describe, expect, it } from "vitest";
import { onWindows, useFakeBinary } from "../helpers/fake-binary.js";

describe.skipIf(onWindows)("fetch", () => {
  useFakeBinary();

  it("returns markdown", async () => {
    const { fetch } = await import("../../src/index.js");
    expect(await fetch("https://e.com")).toBe("# Title\n\nbody\n");
  });

  it("batchFetch preserves order and captures per-URL errors", async () => {
    const { batchFetch } = await import("../../src/index.js");
    const results = await batchFetch(["https://e.com", "https://failurl.example"], {
      concurrency: 2,
    });
    expect(results).toHaveLength(2);
    expect(results[0]).toEqual({ url: "https://e.com", ok: true, markdown: "# Title\n\nbody\n" });
    expect(results[1]).toMatchObject({ url: "https://failurl.example", ok: false });
    expect((results[1] as { error: string }).error).toContain("invalid URL");
  });
});
