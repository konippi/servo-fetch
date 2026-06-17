import { type ChildProcessWithoutNullStreams, spawn } from "node:child_process";
import { createInterface, type Interface } from "node:readline";
import { binaryPath } from "./binary.js";
import { type RpcErrorBody, rpcError, ServoFetchError } from "./errors.js";

interface Response {
  id?: number;
  method?: string;
  result?: unknown;
  error?: RpcErrorBody;
  params?: { token: number; value: unknown };
}

/** Single-consumer queue fed by `$/progress` notifications. */
class ProgressStream<T> {
  private readonly queued: T[] = [];
  private waiting?: { resolve: (r: IteratorResult<T>) => void; reject: (e: Error) => void };
  private done = false;
  private failure?: Error;

  push(value: T): void {
    if (this.done) return;
    if (this.waiting) {
      this.waiting.resolve({ value, done: false });
      this.waiting = undefined;
    } else {
      this.queued.push(value);
    }
  }

  close(failure?: Error): void {
    this.done = true;
    this.failure = failure;
    if (this.waiting) {
      if (failure) this.waiting.reject(failure);
      else this.waiting.resolve({ value: undefined, done: true });
      this.waiting = undefined;
    }
  }

  async *[Symbol.asyncIterator](): AsyncGenerator<T> {
    while (true) {
      if (this.queued.length > 0) {
        yield this.queued.shift() as T;
        continue;
      }
      if (this.done) {
        if (this.failure) throw this.failure;
        return;
      }
      const next = await new Promise<IteratorResult<T>>((resolve, reject) => {
        this.waiting = { resolve, reject };
      });
      if (next.done) {
        if (this.failure) throw this.failure;
        return;
      }
      yield next.value;
    }
  }
}

type Pipe = { ref(): void; unref(): void };

// Child stdio are sockets at runtime; we only need their ref/unref.
const asPipe = (stream: unknown): Pipe => stream as Pipe;

class RpcClient {
  private readonly child: ChildProcessWithoutNullStreams;
  private readonly stdout: Pipe;
  private readonly lines: Interface;
  private readonly pending = new Map<
    number,
    { resolve: (v: unknown) => void; reject: (e: Error) => void }
  >();
  private readonly streams = new Map<number, ProgressStream<unknown>>();
  private nextId = 1;
  private active = 0;
  private stderrTail = "";
  private closed?: ServoFetchError;
  private readonly onExit = () => this.child.kill();

  constructor() {
    this.child = spawn(binaryPath(), ["--quiet", "rpc"], { windowsHide: true });
    this.stdout = asPipe(this.child.stdout);
    asPipe(this.child.stdin).unref();
    asPipe(this.child.stderr).unref();
    this.stdout.unref();
    this.child.unref();

    this.lines = createInterface({ input: this.child.stdout, crlfDelay: Number.POSITIVE_INFINITY });
    this.lines.on("line", (line) => this.dispatch(line));
    this.child.stderr.on("data", (c: Buffer) => {
      this.stderrTail = `${this.stderrTail}${c}`.slice(-4096);
    });
    // Swallow pipe errors; real failures surface via child error/exit.
    for (const pipe of [this.child.stdin, this.child.stdout, this.child.stderr])
      pipe.on("error", () => {});
    this.child.on("error", (e) => this.fail(e.message));
    this.child.on("exit", (code, signal) => {
      const tail = this.stderrTail.trim();
      this.fail(
        `servo-fetch rpc exited (code ${code}, signal ${signal})${tail ? `: ${tail}` : ""}`,
      );
    });
    process.once("exit", this.onExit);
  }

  private retain(): void {
    if (this.active++ === 0) {
      this.child.ref();
      this.stdout.ref();
    }
  }

  private release(): void {
    if (--this.active === 0) {
      this.child.unref();
      this.stdout.unref();
    }
  }

  private dispatch(line: string): void {
    let msg: Response;
    try {
      msg = JSON.parse(line) as Response;
    } catch {
      return;
    }
    if (msg.method === "$/progress" && msg.params) {
      this.streams.get(msg.params.token)?.push(msg.params.value);
      return;
    }
    if (typeof msg.id !== "number") return;

    const stream = this.streams.get(msg.id);
    if (stream) {
      this.streams.delete(msg.id);
      this.release();
      stream.close(msg.error ? rpcError(msg.error) : undefined);
      return;
    }
    const waiter = this.pending.get(msg.id);
    if (!waiter) return;
    this.pending.delete(msg.id);
    this.release();
    if (msg.error) waiter.reject(rpcError(msg.error));
    else waiter.resolve(msg.result);
  }

  private close(error: ServoFetchError): void {
    if (this.closed) return;
    this.closed = error;
    for (const { reject } of this.pending.values()) reject(error);
    for (const stream of this.streams.values()) stream.close(error);
    this.pending.clear();
    this.streams.clear();
    this.active = 0;
    process.removeListener("exit", this.onExit);
    if (current === this) current = undefined;
  }

  private fail(reason: string): void {
    this.close(new ServoFetchError(`servo-fetch rpc connection lost: ${reason}`));
  }

  dispose(): void {
    if (this.closed) return;
    this.close(new ServoFetchError("servo-fetch rpc client stopped", "requestCancelled"));
    this.child.kill();
  }

  private write(message: unknown): void {
    this.child.stdin.write(`${JSON.stringify(message)}\n`);
  }

  private cancel(id: number): void {
    if (!this.closed) this.write({ jsonrpc: "2.0", method: "$/cancelRequest", params: { id } });
  }

  request(method: string, params: unknown, signal?: AbortSignal): Promise<unknown> {
    if (this.closed) return Promise.reject(this.closed);
    if (signal?.aborted) return Promise.reject(abortError());
    const id = this.nextId++;
    this.retain();
    return new Promise((resolve, reject) => {
      const onAbort = () => {
        if (!this.pending.delete(id)) return;
        this.release();
        this.cancel(id);
        reject(abortError());
      };
      signal?.addEventListener("abort", onAbort, { once: true });
      this.pending.set(id, {
        resolve: (v: unknown) => {
          signal?.removeEventListener("abort", onAbort);
          resolve(v);
        },
        reject: (e: Error) => {
          signal?.removeEventListener("abort", onAbort);
          reject(e);
        },
      });
      this.write({ jsonrpc: "2.0", id, method, params });
    });
  }

  async *stream(method: string, params: unknown, signal?: AbortSignal): AsyncGenerator<unknown> {
    if (this.closed) throw this.closed;
    const id = this.nextId++;
    const out = new ProgressStream<unknown>();
    this.retain();
    this.streams.set(id, out);
    const onAbort = () => {
      if (this.streams.delete(id)) {
        this.release();
        this.cancel(id);
        out.close(abortError());
      }
    };
    signal?.addEventListener("abort", onAbort, { once: true });
    this.write({ jsonrpc: "2.0", id, method, params });
    try {
      yield* out;
    } finally {
      signal?.removeEventListener("abort", onAbort);
      // Consumer stopped early: cancel the server.
      if (this.streams.delete(id)) {
        this.release();
        this.cancel(id);
      }
    }
  }
}

function abortError(): ServoFetchError {
  return new ServoFetchError("request aborted", "requestCancelled");
}

let current: RpcClient | undefined;

function client(): RpcClient {
  if (!current) current = new RpcClient();
  return current;
}

export function request<T>(method: string, params: unknown, signal?: AbortSignal): Promise<T> {
  return client().request(method, params, signal) as Promise<T>;
}

export function stream<T>(
  method: string,
  params: unknown,
  signal?: AbortSignal,
): AsyncGenerator<T> {
  return client().stream(method, params, signal) as AsyncGenerator<T>;
}

/** Stop the persistent process; the next call respawns. */
export function shutdown(): void {
  current?.dispose();
}
