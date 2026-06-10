#!/usr/bin/env node
"use strict";

const { spawnSync } = require("node:child_process");
const { binaryPath } = require("../dist/index.cjs");

let bin;
try {
  bin = binaryPath();
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
}

const child = spawnSync(bin, process.argv.slice(2), {
  stdio: "inherit",
  shell: false,
  windowsHide: true,
});

if (child.error) {
  console.error(child.error.message);
  process.exit(1);
}

if (child.signal) {
  process.kill(process.pid, child.signal);
} else {
  process.exitCode = child.status ?? 1;
}
