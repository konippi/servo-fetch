import { describe, expect, it } from "vitest";
import { onWindows, useFakeBinary } from "../helpers/fake-binary.js";

describe.skipIf(onWindows)("crawl & map", () => {
  useFakeBinary();

  it("crawl streams pages and errors, skipping stats", async () => {
    const { crawlAll } = await import("../../src/index.js");
    const results = await crawlAll("https://e.com");
    expect(results).toHaveLength(2);
    expect(results[0]).toMatchObject({ ok: true, url: "https://e.com/", linksFound: 2 });
    expect(results[1]).toMatchObject({ ok: false, error: "boom" });
  });

  it("map returns discovered URLs", async () => {
    const { map } = await import("../../src/index.js");
    expect(await map("https://e.com")).toEqual([
      { url: "https://e.com/" },
      { url: "https://e.com/a", lastmod: "2024-01-01" },
    ]);
  });
});
