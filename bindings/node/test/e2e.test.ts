import { describe, expect, it } from "vitest";

const e2e = process.env.SERVO_FETCH_E2E === "1";
const URL = process.env.SERVO_FETCH_TEST_URL ?? "https://example.com";

describe.runIf(e2e)("real binary E2E", () => {
  it("reports a semver version", async () => {
    const { version } = await import("../src/index.js");
    expect(await version()).toMatch(/^\d+\.\d+\.\d+/);
  });

  it("fetches a real URL and renders non-empty markdown", async () => {
    const { fetch } = await import("../src/index.js");
    const markdown = await fetch(URL);
    expect(typeof markdown).toBe("string");
    expect(markdown.length).toBeGreaterThan(0);
  });

  it("extract output still matches the Article type", async () => {
    const { extract } = await import("../src/index.js");
    const article = await extract(URL);
    expect(typeof article.title).toBe("string");
    expect(typeof article.content).toBe("string");
    expect(typeof article.textContent).toBe("string");
  });

  it("crawl output still matches the CrawlResult type", async () => {
    const { crawlAll } = await import("../src/index.js");
    const [first] = await crawlAll(URL, { limit: 1 });
    expect(first?.ok).toBe(true);
    if (first?.ok) {
      expect(typeof first.url).toBe("string");
      expect(typeof first.depth).toBe("number");
      expect(typeof first.fetchedAt).toBe("string");
      expect(typeof first.content).toBe("string");
      expect(typeof first.linksFound).toBe("number");
    }
  });

  it("maps the real binary's sysexits 64 to InvalidUrlError", async () => {
    const { fetch, InvalidUrlError } = await import("../src/index.js");
    const err = await fetch("not a url").then(
      () => null,
      (e: unknown) => e,
    );
    expect(err).toBeInstanceOf(InvalidUrlError);
    expect((err as { exitCode: number }).exitCode).toBe(64);
  });
});
