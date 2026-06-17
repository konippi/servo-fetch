#!/usr/bin/env node
import { appendFileSync } from "node:fs";
import { createInterface } from "node:readline";

const send = (obj) => process.stdout.write(`${JSON.stringify(obj)}\n`);
const progress = (token, value) =>
  send({ jsonrpc: "2.0", method: "$/progress", params: { token, value } });
const timers = new Map();

const PAGE = {
  type: "page",
  url: "https://e.com/",
  depth: 0,
  fetchedAt: "2024-01-01T00:00:00.000Z",
  title: "Home",
  content: "# Home",
  linksFound: 2,
};
const ERROR = {
  type: "error",
  url: "https://e.com/bad",
  depth: 1,
  fetchedAt: "2024-01-01T00:00:01.000Z",
  error: "boom",
};
const STATS = { type: "stats", crawled: 2, errors: 1, elapsedMs: 5 };

function finishCrawl(id) {
  progress(id, ERROR);
  progress(id, STATS);
  send({ jsonrpc: "2.0", id, result: { crawled: 2, errors: 1, elapsedMs: 5 } });
}

createInterface({ input: process.stdin }).on("line", (line) => {
  if (!line.trim()) return;
  const { id, method, params = {} } = JSON.parse(line);

  if (method === "$/cancelRequest") {
    const timer = timers.get(params.id);
    if (timer) clearTimeout(timer);
    timers.delete(params.id);
    if (process.env.SERVO_FETCH_CANCEL_FILE)
      appendFileSync(process.env.SERVO_FETCH_CANCEL_FILE, `${params.id}\n`);
    return;
  }

  if (typeof params.url === "string" && params.url.includes("failurl")) {
    send({
      jsonrpc: "2.0",
      id,
      error: { code: -32602, message: "invalid URL 'x'", data: { kind: "invalidUrl" } },
    });
    return;
  }

  switch (method) {
    case "initialize":
      send({
        jsonrpc: "2.0",
        id,
        result: {
          protocolVersion: 1,
          serverInfo: { name: "servo-fetch", version: "9.9.9" },
          capabilities: {},
        },
      });
      break;
    case "fetch":
      send({ jsonrpc: "2.0", id, result: "# Title\n\nbody\n" });
      break;
    case "extract":
      send({
        jsonrpc: "2.0",
        id,
        result: { title: "T", content: "<p>c</p>", textContent: "c", byline: "me" },
      });
      break;
    case "evaluate":
      send({ jsonrpc: "2.0", id, result: { url: params.url, result: "JS_RESULT", console: [] } });
      break;
    case "extractSchema":
      send({ jsonrpc: "2.0", id, result: { url: params.url, extracted: [{ title: "x" }] } });
      break;
    case "screenshot":
      send({ jsonrpc: "2.0", id, result: Buffer.from("PNGDATA").toString("base64") });
      break;
    case "map":
      send({
        jsonrpc: "2.0",
        id,
        result: [{ url: "https://e.com/" }, { url: "https://e.com/a", lastmod: "2024-01-01" }],
      });
      break;
    case "crawl":
      progress(id, PAGE);
      // A "slow" seed defers the rest so a consumer can break and trigger cancellation.
      if (params.url?.includes("slow"))
        timers.set(
          id,
          setTimeout(() => finishCrawl(id), 50),
        );
      else finishCrawl(id);
      break;
    default:
      send({
        jsonrpc: "2.0",
        id,
        error: {
          code: -32601,
          message: `method not found: ${method}`,
          data: { kind: "methodNotFound" },
        },
      });
  }
});
