import { describe, expect, it } from "vitest";
import { onWindows, useFakeBinary } from "../helpers/fake-binary.js";

describe.skipIf(onWindows)("session", () => {
  useFakeBinary();

  it("fetch returns every requested format", async () => {
    const { Session } = await import("../../src/index.js");
    const session = await Session.open();
    try {
      expect(await session.fetch("https://e.com", { formats: ["html", "article"] })).toEqual({
        url: "https://e.com",
        title: "T",
        html: "<p>c</p>",
        article: { title: "T", content: "<p>c</p>", textContent: "c" },
      });
    } finally {
      await session.close();
    }
  });
});
