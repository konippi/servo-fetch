import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { JsonParseError } from "./errors.js";
import type { Article, CrawlEvent, MappedUrl, SchemaExtractResult } from "./generated/index.js";
import { type Schema, schemaToJson } from "./schema.js";
import { runBuffer, runStream, runText } from "./spawn.js";
import type { BatchResult, CrawlOptions, CrawlResult, FetchOptions, MapOptions } from "./types.js";

export { binaryPath } from "./binary.js";
export {
  CookieError,
  EngineError,
  FetchTimeoutError,
  InvalidUrlError,
  IoError,
  JsonParseError,
  NetworkError,
  SchemaError,
  ServoFetchError,
} from "./errors.js";
export type { Article, MappedUrl, Visibility } from "./generated/index.js";
export type { Field, Schema } from "./schema.js";
export type { BatchResult, CrawlOptions, CrawlResult, FetchOptions, MapOptions } from "./types.js";

function asArray(value: string | string[] | undefined): string[] {
  if (value === undefined) return [];
  return Array.isArray(value) ? value : [value];
}

function headerArgs(headers: Record<string, string> | undefined): string[] {
  if (headers == null) return [];
  return Object.entries(headers).flatMap(([name, value]) => ["-H", `${name}: ${value}`]);
}

function parseJson<T>(raw: string): T {
  try {
    return JSON.parse(raw) as T;
  } catch {
    throw new JsonParseError("could not parse servo-fetch output as JSON", 0, raw.slice(0, 200));
  }
}

function commonArgs(o: FetchOptions): string[] {
  const args: string[] = [];
  if (o.timeout != null) args.push("-t", String(o.timeout));
  if (o.settle != null) args.push("--settle", String(o.settle));
  if (o.userAgent != null) args.push("--user-agent", o.userAgent);
  if (o.cookiesFile != null) args.push("--cookies", o.cookiesFile);
  if (o.visibility != null) args.push("--visibility", o.visibility);
  if (o.allowPrivateAddresses) args.push("--allow-private-addresses");
  args.push(...headerArgs(o.headers));
  return args;
}

/** Fetch a URL and return readable Markdown. */
export async function fetch(url: string, options: FetchOptions = {}): Promise<string> {
  const args = ["--format", "markdown", ...commonArgs(options)];
  if (options.selector != null) args.push("--selector", options.selector);
  args.push("--", url);
  return runText(args, { signal: options.signal });
}

/** Fetch a URL and return the rendered HTML (post-JS execution). */
export async function fetchHtml(url: string, options: FetchOptions = {}): Promise<string> {
  const args = ["--format", "html", ...commonArgs(options)];
  if (options.selector != null) args.push("--selector", options.selector);
  args.push("--", url);
  return runText(args, { signal: options.signal });
}

/** Fetch a URL and return `document.body.innerText`. */
export async function fetchText(url: string, options: FetchOptions = {}): Promise<string> {
  const args = ["--format", "text", ...commonArgs(options)];
  if (options.selector != null) args.push("--selector", options.selector);
  args.push("--", url);
  return runText(args, { signal: options.signal });
}

/** Fetch a URL and return structured Readability data. */
export async function extract(url: string, options: FetchOptions = {}): Promise<Article> {
  const args = ["--format", "json", ...commonArgs(options)];
  if (options.selector != null) args.push("--selector", options.selector);
  args.push("--", url);
  return parseJson<Article>(await runText(args, { signal: options.signal }));
}

/** Extract structured data using a declarative CSS-selector schema. */
export async function extractSchema<T = unknown>(
  url: string,
  schema: Schema,
  options: FetchOptions = {},
): Promise<T> {
  const dir = await mkdtemp(join(tmpdir(), "servo-fetch-"));
  try {
    const file = join(dir, "schema.json");
    await writeFile(file, schemaToJson(schema));
    const args = [...commonArgs(options), "--schema", file, "--", url];
    const raw = parseJson<SchemaExtractResult>(await runText(args, { signal: options.signal }));
    return raw.extracted as T;
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
}

/** Capture a PNG screenshot of the rendered page. */
export async function screenshot(
  url: string,
  options: FetchOptions & { fullPage?: boolean } = {},
): Promise<Buffer> {
  const args = ["--format", "png", ...commonArgs(options)];
  if (options.fullPage) args.push("--full-page");
  args.push("--", url);
  return runBuffer(args, { signal: options.signal, maxBuffer: options.maxBuffer });
}

/** Execute JavaScript in the page and return the result as a string. */
export async function evaluate(
  url: string,
  expression: string,
  options: FetchOptions = {},
): Promise<string> {
  const args = ["--js", expression, ...commonArgs(options), "--", url];
  return (await runText(args, { signal: options.signal })).replace(/\n$/, "");
}

/** Fetch many URLs concurrently, returning per-URL Markdown or error. */
export async function batchFetch(
  urls: string[],
  options: FetchOptions & { concurrency?: number } = {},
): Promise<BatchResult[]> {
  const concurrency = Math.max(1, options.concurrency ?? 2);
  const results: BatchResult[] = new Array(urls.length);
  let next = 0;

  const worker = async (): Promise<void> => {
    while (next < urls.length) {
      const index = next++;
      const url = urls[index];
      if (url === undefined) break;
      try {
        results[index] = { url, ok: true, markdown: await fetch(url, options) };
      } catch (error) {
        results[index] = {
          url,
          ok: false,
          error: error instanceof Error ? error.message : String(error),
        };
      }
    }
  };

  await Promise.all(Array.from({ length: Math.min(concurrency, urls.length) }, worker));
  return results;
}

function crawlArgs(url: string, o: CrawlOptions): string[] {
  const args = ["crawl", "--format", "json"];
  if (o.limit != null) args.push("--limit", String(o.limit));
  if (o.maxDepth != null) args.push("--max-depth", String(o.maxDepth));
  for (const glob of asArray(o.include)) args.push("--include", glob);
  for (const glob of asArray(o.exclude)) args.push("--exclude", glob);
  if (o.concurrency != null) args.push("--concurrency", String(o.concurrency));
  if (o.delayMs != null) args.push("--delay-ms", String(o.delayMs));
  if (o.timeout != null) args.push("-t", String(o.timeout));
  if (o.settle != null) args.push("--settle", String(o.settle));
  if (o.userAgent != null) args.push("--user-agent", o.userAgent);
  if (o.cookiesFile != null) args.push("--cookies", o.cookiesFile);
  if (o.selector != null) args.push("--selector", o.selector);
  if (o.allowPrivateAddresses) args.push("--allow-private-addresses");
  args.push(...headerArgs(o.headers));
  args.push("--", url);
  return args;
}

/** Crawl a site, yielding each page as it completes (BFS, respects robots.txt). */
export async function* crawl(url: string, options: CrawlOptions = {}): AsyncGenerator<CrawlResult> {
  for await (const line of runStream(crawlArgs(url, options), { signal: options.signal })) {
    const record = parseJson<CrawlEvent>(line);
    if (record.type === "stats") continue;
    if (record.type === "error") {
      yield {
        ok: false,
        url: record.url,
        depth: record.depth,
        fetchedAt: record.fetchedAt,
        error: record.error,
      };
    } else {
      yield {
        ok: true,
        url: record.url,
        depth: record.depth,
        fetchedAt: record.fetchedAt,
        title: record.title ?? null,
        content: record.content,
        linksFound: record.linksFound,
      };
    }
  }
}

/** Crawl a site and collect every result into an array. */
export async function crawlAll(url: string, options: CrawlOptions = {}): Promise<CrawlResult[]> {
  const results: CrawlResult[] = [];
  for await (const result of crawl(url, options)) results.push(result);
  return results;
}

/** Discover URLs on a site via sitemaps (no rendering). */
export async function map(url: string, options: MapOptions = {}): Promise<MappedUrl[]> {
  const args = ["map", "--json"];
  if (options.limit != null) args.push("--limit", String(options.limit));
  for (const glob of asArray(options.include)) args.push("--include", glob);
  for (const glob of asArray(options.exclude)) args.push("--exclude", glob);
  if (options.userAgent != null) args.push("--user-agent", options.userAgent);
  if (options.timeout != null) args.push("-t", String(options.timeout));
  if (options.noFallback) args.push("--no-fallback");
  if (options.allowPrivateAddresses) args.push("--allow-private-addresses");
  args.push(...headerArgs(options.headers));
  args.push("--", url);
  return parseJson<MappedUrl[]>(await runText(args, { signal: options.signal }));
}

/** The version of the underlying servo-fetch binary. */
export async function version(): Promise<string> {
  return (await runText(["--version"])).trim().replace(/^servo-fetch\s+/, "");
}
