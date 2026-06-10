export class ServoFetchError extends Error {
  readonly exitCode: number | null;
  readonly stderr: string;

  constructor(message: string, exitCode: number | null, stderr: string) {
    super(message);
    this.name = new.target.name;
    this.exitCode = exitCode;
    this.stderr = stderr;
  }
}

export class InvalidUrlError extends ServoFetchError {}
export class FetchTimeoutError extends ServoFetchError {}
export class NetworkError extends ServoFetchError {}
export class EngineError extends ServoFetchError {}
export class SchemaError extends ServoFetchError {}
export class CookieError extends ServoFetchError {}
export class IoError extends ServoFetchError {}
export class JsonParseError extends ServoFetchError {}

function lastErrorLine(stderr: string): string {
  const lines = stderr
    .split("\n")
    .map((l) => l.trim())
    .filter(Boolean);
  for (let i = lines.length - 1; i >= 0; i--) {
    const line = lines[i];
    if (line?.startsWith("error:")) return line.slice("error:".length).trim();
  }
  return lines.at(-1) ?? "";
}

type ServoFetchErrorCtor = new (
  message: string,
  exitCode: number | null,
  stderr: string,
) => ServoFetchError;

// Maps the CLI's sysexits.h exit codes to error types.
const BY_EXIT_CODE: ReadonlyMap<number, ServoFetchErrorCtor> = new Map([
  [64, InvalidUrlError], // EX_USAGE
  [65, SchemaError], // EX_DATAERR
  [66, CookieError], // EX_NOINPUT
  [69, NetworkError], // EX_UNAVAILABLE
  [70, EngineError], // EX_SOFTWARE
  [74, IoError], // EX_IOERR
  [75, FetchTimeoutError], // EX_TEMPFAIL
]);

export function classifyError(stderr: string, exitCode: number | null): ServoFetchError {
  const message = lastErrorLine(stderr);
  const Ctor = (exitCode !== null && BY_EXIT_CODE.get(exitCode)) || ServoFetchError;
  return new Ctor(message || `servo-fetch exited with code ${exitCode}`, exitCode, stderr);
}
