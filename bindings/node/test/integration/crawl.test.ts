import { readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { onWindows, useFakeBinary } from "../helpers/fake-binary.js";

describe.skipIf(onWindows)("crawl & map", () => {
  useFakeBinary();
  const cancelLog = join(tmpdir(), `servo-fetch-cancel-${process.pid}.log`);
  beforeAll(() => {
    process.env.SERVO_FETCH_CANCEL_FILE = cancelLog;
  });
  afterAll(() => {
    delete process.env.SERVO_FETCH_CANCEL_FILE;
  });

  it("crawl streams pages and errors, skipping stats", async () => {
    const { crawlAll } = await import("../../src/index.js");
    const results = await crawlAll("https://e.com");
    expect(results).toHaveLength(2);
    expect(results[0]).toMatchObject({ ok: true, url: "https://e.com/", linksFound: 2 });
    expect(results[1]).toMatchObject({ ok: false, error: "boom" });
  });

  it("cancels the server when the consumer stops early", async () => {
    const { crawl } = await import("../../src/index.js");
    for await (const _page of crawl("https://slow.example")) break;
    await new Promise((resolve) => setTimeout(resolve, 120));
    expect(readFileSync(cancelLog, "utf8").trim().length).toBeGreaterThan(0);
  });

  it("map returns discovered URLs", async () => {
    const { map } = await import("../../src/index.js");
    expect(await map("https://e.com")).toEqual([
      { url: "https://e.com/" },
      { url: "https://e.com/a", lastmod: "2024-01-01" },
    ]);
  });
});
