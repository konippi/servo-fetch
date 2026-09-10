import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { afterAll, beforeAll } from "vitest";

const PAGE =
  "<!doctype html><html>" +
  '<head><meta charset="utf-8"><title>servo-fetch e2e</title></head>' +
  "<body><h1>Hermetic fixture</h1><p>Served by the suite-scoped local test server.</p></body>" +
  "</html>";

export interface LocalServer {
  /** Base URL of the suite-scoped local page, available once the suite starts. */
  readonly url: string;
}

/** Serve a minimal valid page from 127.0.0.1 for the current suite. */
export function useLocalServer(): LocalServer {
  const ctx = { url: "" };
  let server: Server | undefined;
  let previousAllowPrivate: string | undefined;
  beforeAll(async () => {
    const created = createServer((_request, response) => {
      response.writeHead(200, { "content-type": "text/html; charset=utf-8" });
      response.end(PAGE);
    });
    await new Promise<void>((resolve) => created.listen(0, "127.0.0.1", resolve));
    server = created;
    const { port } = created.address() as AddressInfo;
    ctx.url = `http://127.0.0.1:${port}/`;
    // The spawned CLI inherits this and lifts the SSRF loopback block for the suite.
    previousAllowPrivate = process.env.SERVO_FETCH_ALLOW_PRIVATE;
    process.env.SERVO_FETCH_ALLOW_PRIVATE = "1";
  });
  afterAll(async () => {
    if (previousAllowPrivate === undefined) {
      delete process.env.SERVO_FETCH_ALLOW_PRIVATE;
    } else {
      process.env.SERVO_FETCH_ALLOW_PRIVATE = previousAllowPrivate;
    }
    if (!server) {
      return;
    }
    const listening = server;
    listening.closeAllConnections();
    await new Promise<void>((resolve, reject) => {
      listening.close((error) => (error ? reject(error) : resolve()));
    });
  });
  return ctx;
}
