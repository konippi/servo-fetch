import { type ChildProcess, spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { beforeAll, describe, expect, it } from "vitest";

const CLI = fileURLToPath(new URL("../dist/cli.mjs", import.meta.url));

interface Outcome {
  code: number | null;
  signal: NodeJS.Signals | null;
  stdout: string;
}

function launch(script: string, detached = false): ChildProcess {
  return spawn(process.execPath, [CLI, "-e", script], {
    env: { ...process.env, SERVO_FETCH_BINARY_PATH: process.execPath },
    stdio: ["ignore", "pipe", "inherit"],
    detached,
  });
}

function trap(signal: NodeJS.Signals): string {
  return `setInterval(() => {}, 1000);
process.on("${signal}", () => process.exit(42));
console.log("ready");`;
}

function outcome(proc: ChildProcess): Promise<Outcome> {
  let stdout = "";
  proc.stdout?.on("data", (chunk) => {
    stdout += chunk;
  });
  return new Promise((resolve) => {
    proc.on("close", (code, signal) => resolve({ code, signal, stdout }));
  });
}

function ready(proc: ChildProcess): Promise<void> {
  return new Promise((resolve) => {
    proc.stdout?.on("data", (chunk: Buffer) => {
      if (chunk.toString().includes("ready")) resolve();
    });
  });
}

describe.skipIf(process.platform === "win32")("cli", () => {
  beforeAll(() => {
    if (!existsSync(CLI)) throw new Error("dist/cli.mjs is missing, run `pnpm run build` first");
  });

  it("passes the exit code through", async () => {
    expect(await outcome(launch("process.exitCode = 3"))).toEqual({
      code: 3,
      signal: null,
      stdout: "",
    });
  });

  it("dies by the signal that killed the binary", async () => {
    expect(await outcome(launch('process.kill(process.pid, "SIGTERM")'))).toEqual({
      code: null,
      signal: "SIGTERM",
      stdout: "",
    });
  });

  it("exits with 128+n for a signal Node ignores", async () => {
    const script =
      'const f = () => {}; process.on("SIGPIPE", f).off("SIGPIPE", f); process.kill(process.pid, "SIGPIPE")';
    expect(await outcome(launch(script))).toEqual({ code: 141, signal: null, stdout: "" });
  });

  it("forwards SIGTERM to the binary", async () => {
    const proc = launch(trap("SIGTERM"));
    const result = outcome(proc);
    await ready(proc);
    proc.kill("SIGTERM");
    expect(await result).toEqual({ code: 42, signal: null, stdout: "ready\n" });
  });

  it("waits for the binary to handle Ctrl-C sent to the process group", async () => {
    const proc = launch(trap("SIGINT"), true);
    const result = outcome(proc);
    await ready(proc);
    process.kill(-(proc.pid as number), "SIGINT");
    expect(await result).toEqual({ code: 42, signal: null, stdout: "ready\n" });
  });
});
