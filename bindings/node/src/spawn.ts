import { spawn } from "node:child_process";
import { createInterface } from "node:readline";
import { binaryPath } from "./binary.js";
import { classifyError, ServoFetchError } from "./errors.js";

const GLOBAL_ARGS = ["--quiet"];
const DEFAULT_MAX_BUFFER = 128 * 1024 * 1024;

export interface RunOptions {
  signal?: AbortSignal;
  maxBuffer?: number;
}

export function runBuffer(args: string[], opts: RunOptions = {}): Promise<Buffer> {
  const maxBuffer = opts.maxBuffer ?? DEFAULT_MAX_BUFFER;
  return new Promise((resolve, reject) => {
    const child = spawn(binaryPath(), [...GLOBAL_ARGS, ...args], {
      stdio: ["ignore", "pipe", "pipe"],
      windowsHide: true,
      signal: opts.signal,
    });
    const stdout: Buffer[] = [];
    const stderr: Buffer[] = [];
    let size = 0;
    child.stdout.on("data", (chunk: Buffer) => {
      size += chunk.length;
      if (size > maxBuffer) {
        child.stdout.destroy();
        child.kill("SIGKILL");
        reject(new ServoFetchError(`output exceeded ${maxBuffer} bytes`, null, ""));
        return;
      }
      stdout.push(chunk);
    });
    child.stderr.on("data", (chunk: Buffer) => stderr.push(chunk));
    child.on("error", reject);
    child.on("close", (code) => {
      if (code === 0) resolve(Buffer.concat(stdout));
      else reject(classifyError(Buffer.concat(stderr).toString(), code));
    });
  });
}

export async function runText(args: string[], opts: RunOptions = {}): Promise<string> {
  return (await runBuffer(args, opts)).toString("utf8");
}

export async function* runStream(args: string[], opts: RunOptions = {}): AsyncGenerator<string> {
  const child = spawn(binaryPath(), [...GLOBAL_ARGS, ...args], {
    stdio: ["ignore", "pipe", "pipe"],
    windowsHide: true,
    signal: opts.signal,
  });
  const stderr: Buffer[] = [];
  child.stderr.on("data", (chunk: Buffer) => stderr.push(chunk));
  child.stdout.on("error", () => {});

  const exit = new Promise<{ code: number | null; error?: Error }>((resolve) => {
    child.on("error", (error) => resolve({ code: null, error }));
    child.on("close", (code) => resolve({ code }));
  });

  const lines = createInterface({ input: child.stdout, crlfDelay: Infinity });
  let completed = false;
  try {
    for await (const line of lines) {
      if (line.trim()) yield line;
    }
    completed = true;
  } finally {
    lines.close();
    // Consumer abandoned the iterator (e.g. `break`): terminate the still-running child.
    if (!completed && child.exitCode === null && child.signalCode === null) {
      child.stdout.destroy();
      child.kill("SIGTERM");
    }
  }

  const { code, error } = await exit;
  if (error) throw error;
  if (code !== 0) throw classifyError(Buffer.concat(stderr).toString(), code);
}
