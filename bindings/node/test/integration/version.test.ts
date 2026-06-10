import { describe, expect, it } from "vitest";
import { onWindows, useFakeBinary } from "../helpers/fake-binary.js";

describe.skipIf(onWindows)("version", () => {
  useFakeBinary();

  it("strips the binary name", async () => {
    const { version } = await import("../../src/index.js");
    expect(await version()).toBe("9.9.9");
  });
});
