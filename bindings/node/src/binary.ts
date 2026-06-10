import { existsSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";

const require = createRequire(import.meta.url);

const PACKAGES: Record<string, string> = {
  "darwin-arm64": "@servo-fetch/darwin-arm64",
  "darwin-x64": "@servo-fetch/darwin-x64",
  "linux-x64-gnu": "@servo-fetch/linux-x64-gnu",
  "linux-arm64-gnu": "@servo-fetch/linux-arm64-gnu",
  "win32-x64-msvc": "@servo-fetch/win32-x64-msvc",
};

function isMusl(): boolean {
  if (process.platform !== "linux") return false;
  try {
    const report = process.report?.getReport() as
      | { header?: { glibcVersionRuntime?: string } }
      | undefined;
    return !report?.header?.glibcVersionRuntime;
  } catch {
    return false;
  }
}

function platformKey(): string {
  const { platform, arch } = process;
  if (platform === "linux") return `linux-${arch}-${isMusl() ? "musl" : "gnu"}`;
  if (platform === "win32") return `win32-${arch}-msvc`;
  return `${platform}-${arch}`;
}

function binName(): string {
  return process.platform === "win32" ? "servo-fetch.exe" : "servo-fetch";
}

let cached: string | undefined;

/** Absolute path to the platform-specific `servo-fetch` executable. */
export function binaryPath(): string {
  if (cached) return cached;

  const override = process.env.SERVO_FETCH_BINARY_PATH;
  if (override) {
    cached = override;
    return override;
  }

  const key = platformKey();
  const pkg = PACKAGES[key];
  if (!pkg) {
    throw new Error(
      `servo-fetch: unsupported platform "${key}". Supported platforms: ${Object.keys(PACKAGES).join(", ")}. ` +
        `Set SERVO_FETCH_BINARY_PATH to use a custom binary.`,
    );
  }

  let manifest: string;
  try {
    manifest = require.resolve(`${pkg}/package.json`);
  } catch {
    throw new Error(
      `servo-fetch: optional platform package "${pkg}" is not installed ` +
        `(it may have been skipped by --no-optional or --ignore-scripts). ` +
        `Reinstall with optional dependencies, or set SERVO_FETCH_BINARY_PATH.`,
    );
  }

  const file = join(dirname(manifest), binName());
  if (!existsSync(file)) {
    throw new Error(`servo-fetch: binary not found in "${pkg}" (expected ${file}).`);
  }

  cached = file;
  return file;
}
