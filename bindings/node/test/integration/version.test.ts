import { describe, expect, it } from "vitest";
import { onWindows, useFakeBinary } from "../helpers/fake-binary.js";

describe.skipIf(onWindows)("version", () => {
  useFakeBinary();

  it("strips the binary name", async () => {
    const { version } = await import("../../src/index.js");
    expect(await version()).toBe("9.9.9");
  });

  it("shutdown stops the process and the next call respawns", async () => {
    const { version, shutdown } = await import("../../src/index.js");
    expect(await version()).toBe("9.9.9");
    shutdown();
    expect(await version()).toBe("9.9.9");
  });
});
