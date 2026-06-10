import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { afterAll, beforeAll } from "vitest";

const FAKE_BIN = fileURLToPath(new URL("../fixtures/servo-fetch.mjs", import.meta.url));

// The fake is spawned via its `#!/usr/bin/env node` shebang, which Windows can't do.
export const onWindows: boolean = process.platform === "win32";

export interface FakeBinary {
  /** Temp dir for per-test scratch files. */
  readonly dir: string;
}

/** Point the binding at the fake servo-fetch fixture for the current suite. */
export function useFakeBinary(): FakeBinary {
  const ctx = { dir: "" };
  beforeAll(() => {
    ctx.dir = mkdtempSync(join(tmpdir(), "servo-fetch-fake-"));
    process.env.SERVO_FETCH_BINARY_PATH = FAKE_BIN;
  });
  afterAll(() => {
    rmSync(ctx.dir, { recursive: true, force: true });
    delete process.env.SERVO_FETCH_BINARY_PATH;
  });
  return ctx;
}
