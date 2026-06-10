import { describe, expect, it } from "vitest";
import { onWindows, useFakeBinary } from "../helpers/fake-binary.js";

describe.skipIf(onWindows)("screenshot & evaluate", () => {
  useFakeBinary();

  it("screenshot returns a Buffer", async () => {
    const { screenshot } = await import("../../src/index.js");
    const png = await screenshot("https://e.com");
    expect(png.toString()).toBe("PNGDATA");
  });

  it("evaluate returns the js result", async () => {
    const { evaluate } = await import("../../src/index.js");
    expect(await evaluate("https://e.com", "document.title")).toBe("JS_RESULT");
  });
});
