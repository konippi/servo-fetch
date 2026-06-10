import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { onWindows, useFakeBinary } from "../helpers/fake-binary.js";

describe.skipIf(onWindows)("fetch", () => {
  const fake = useFakeBinary();

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

  it("passes `--` before the URL so it can never be read as a subcommand", async () => {
    const argsFile = join(fake.dir, "args.log");
    process.env.SERVO_FETCH_ARGS_FILE = argsFile;
    try {
      const { fetch } = await import("../../src/index.js");
      await fetch("serve");
      expect(readFileSync(argsFile, "utf8")).toContain("-- serve");
    } finally {
      delete process.env.SERVO_FETCH_ARGS_FILE;
    }
  });
});
