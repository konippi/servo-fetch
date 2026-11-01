#!/usr/bin/env node
import { spawn } from "node:child_process";
import { constants } from "node:os";

import { binaryPath } from "./binary.js";

const FORWARDED: readonly NodeJS.Signals[] = ["SIGINT", "SIGTERM", "SIGHUP"];

function signalExitCode(signal: NodeJS.Signals): number {
  return 128 + constants.signals[signal];
}

function main(): void {
  let bin: string;
  try {
    bin = binaryPath();
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
    return;
  }

  const child = spawn(bin, process.argv.slice(2), { stdio: "inherit", windowsHide: true });

  const forward = (signal: NodeJS.Signals): void => {
    // Windows kill() is TerminateProcess, and Ctrl+C already reaches the child through the console.
    if (process.platform !== "win32") child.kill(signal);
  };
  for (const signal of FORWARDED) process.on(signal, forward);

  child.on("error", (error: NodeJS.ErrnoException) => {
    console.error(`servo-fetch: ${error.message}`);
    process.exitCode = error.code === "ENOENT" ? 127 : 126;
  });

  child.on("exit", (code, signal) => {
    for (const forwarded of FORWARDED) process.off(forwarded, forward);

    if (signal === null) {
      process.exitCode = code ?? 1;
      return;
    }
    process.exitCode = signalExitCode(signal);
    if (signal !== "SIGUSR1") process.kill(process.pid, signal);
  });
}

main();
