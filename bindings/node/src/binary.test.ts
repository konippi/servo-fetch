import { afterEach, describe, expect, it, vi } from "vitest";

const ORIGINAL_PLATFORM = process.platform;
const ORIGINAL_ARCH = process.arch;
const ORIGINAL_OVERRIDE = process.env.SERVO_FETCH_BINARY_PATH;

function setPlatform(platform: NodeJS.Platform): void {
  Object.defineProperty(process, "platform", { value: platform, configurable: true });
}

function setArch(arch: NodeJS.Architecture): void {
  Object.defineProperty(process, "arch", { value: arch, configurable: true });
}

afterEach(() => {
  setPlatform(ORIGINAL_PLATFORM);
  setArch(ORIGINAL_ARCH);
  if (ORIGINAL_OVERRIDE === undefined) delete process.env.SERVO_FETCH_BINARY_PATH;
  else process.env.SERVO_FETCH_BINARY_PATH = ORIGINAL_OVERRIDE;
  // binaryPath() memoizes in module scope; reset so each test re-resolves.
  vi.resetModules();
});

describe("binaryPath", () => {
  it("returns the SERVO_FETCH_BINARY_PATH override verbatim", async () => {
    process.env.SERVO_FETCH_BINARY_PATH = process.execPath;
    const { binaryPath } = await import("./binary.js");
    expect(binaryPath()).toBe(process.execPath);
  });

  it("throws when SERVO_FETCH_BINARY_PATH points to a missing file", async () => {
    process.env.SERVO_FETCH_BINARY_PATH = "/no/such/servo-fetch";
    const { binaryPath } = await import("./binary.js");
    expect(() => binaryPath()).toThrow(/non-existent file/);
  });

  it("throws a descriptive error on unsupported platforms", async () => {
    delete process.env.SERVO_FETCH_BINARY_PATH;
    setPlatform("sunos");
    const { binaryPath } = await import("./binary.js");
    expect(() => binaryPath()).toThrow(/unsupported platform "sunos/);
  });

  it("resolves the win32-x64-msvc platform package on Windows", async () => {
    delete process.env.SERVO_FETCH_BINARY_PATH;
    setPlatform("win32");
    setArch("x64");
    const { binaryPath } = await import("./binary.js");
    // The package is not installed in dev/CI, so resolution fails referencing it.
    expect(() => binaryPath()).toThrow(/win32-x64-msvc/);
  });
});
