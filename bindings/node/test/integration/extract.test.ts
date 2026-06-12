import { describe, expect, it } from "vitest";
import { onWindows, useFakeBinary } from "../helpers/fake-binary.js";

describe.skipIf(onWindows)("extract", () => {
  useFakeBinary();

  it("returns the structured article", async () => {
    const { extract } = await import("../../src/index.js");
    expect(await extract("https://e.com")).toEqual({
      title: "T",
      content: "<p>c</p>",
      textContent: "c",
      byline: "me",
      excerpt: undefined,
      lang: undefined,
      url: undefined,
    });
  });

  it("extractSchema returns the extracted payload", async () => {
    const { extractSchema } = await import("../../src/index.js");
    const data = await extractSchema("https://e.com", {
      baseSelector: ".x",
      fields: [{ name: "title", selector: "h2", type: "text" }],
    });
    expect(data).toEqual([{ title: "x" }]);
  });
});
