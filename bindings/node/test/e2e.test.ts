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

  it("maps an invalid URL to InvalidUrlError", async () => {
    const { fetch, InvalidUrlError } = await import("../src/index.js");
    const err = await fetch("not a url").then(
      () => null,
      (e: unknown) => e,
    );
    expect(err).toBeInstanceOf(InvalidUrlError);
    expect((err as { kind: string }).kind).toBe("invalidUrl");
  });

  it("session fetches within an isolated worker and closes cleanly", async () => {
    const { Session } = await import("../src/index.js");
    const session = await Session.open();
    try {
      const markdown = await session.fetch(URL);
      expect(typeof markdown).toBe("string");
      expect(markdown.length).toBeGreaterThan(0);
    } finally {
      await session.close();
    }
  });

  it("closed session is rejected with a typed error", async () => {
    const { Session, ServoFetchError } = await import("../src/index.js");
    const session = await Session.open();
    expect(session.isClosed()).toBe(false);
    await session.close();
    await session.close();
    expect(session.isClosed()).toBe(true);
    const err = await session.fetch(URL).then(
      () => null,
      (e: unknown) => e,
    );
    expect(err).toBeInstanceOf(ServoFetchError);
    expect((err as Error).message).toContain("browser session is closed");
  });

  it("session handle from a previous server process is rejected", async () => {
    const { Session, ServoFetchError } = await import("../src/index.js");
    const { shutdown } = await import("../src/rpc-client.js");
    const stale = await Session.open();
    shutdown();
    const err = await stale.fetch(URL).then(
      () => null,
      (e: unknown) => e,
    );
    expect(err).toBeInstanceOf(ServoFetchError);
    expect((err as Error).message).toContain("previous server process");
    await stale.close();
  });
});
