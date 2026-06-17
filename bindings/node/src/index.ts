import type {
  Article,
  CrawlEvent,
  CrawlRequest,
  EvaluateRequest,
  EvaluateResult,
  ExtractRequest,
  FetchRequest,
  InitializeResult,
  MappedUrl,
  MapRequest,
  RequestOptions,
  SchemaExtractRequest,
  SchemaExtractResult,
  ScreenshotRequest,
} from "./generated/index.js";
import { request, stream } from "./rpc-client.js";
import { type Schema, schemaToValue } from "./schema.js";
import type { BatchResult, CrawlOptions, CrawlResult, FetchOptions, MapOptions } from "./types.js";

export { binaryPath } from "./binary.js";
export {
  EngineError,
  FetchTimeoutError,
  InvalidUrlError,
  NetworkError,
  ServoFetchError,
} from "./errors.js";
export type { Article, MappedUrl, Visibility } from "./generated/index.js";
export { shutdown } from "./rpc-client.js";
export type { Field, Schema } from "./schema.js";
export type { BatchResult, CrawlOptions, CrawlResult, FetchOptions, MapOptions } from "./types.js";

type CommonOptions = Pick<
  FetchOptions,
  "timeout" | "settle" | "userAgent" | "cookiesFile" | "headers"
>;

function common(o: CommonOptions): RequestOptions {
  return {
    timeout: o.timeout,
    settleMs: o.settle,
    userAgent: o.userAgent,
    cookiesFile: o.cookiesFile,
    headers: o.headers,
  };
}

function asArray(value: string | string[] | undefined): string[] | undefined {
  if (value === undefined) return undefined;
  return Array.isArray(value) ? value : [value];
}

/** Fetch a URL and return readable Markdown. */
export function fetch(url: string, options: FetchOptions = {}): Promise<string> {
  return request(
    "fetch",
    {
      url,
      format: "markdown",
      selector: options.selector,
      visibility: options.visibility,
      ...common(options),
    } satisfies FetchRequest,
    options.signal,
  );
}

/** Fetch a URL and return the rendered HTML (post-JS execution). */
export function fetchHtml(url: string, options: FetchOptions = {}): Promise<string> {
  return request(
    "fetch",
    {
      url,
      format: "html",
      selector: options.selector,
      visibility: options.visibility,
      ...common(options),
    } satisfies FetchRequest,
    options.signal,
  );
}

/** Fetch a URL and return `document.body.innerText`. */
export function fetchText(url: string, options: FetchOptions = {}): Promise<string> {
  return request(
    "fetch",
    {
      url,
      format: "text",
      visibility: options.visibility,
      ...common(options),
    } satisfies FetchRequest,
    options.signal,
  );
}

/** Fetch a URL and return structured Readability data. */
export function extract(url: string, options: FetchOptions = {}): Promise<Article> {
  return request(
    "extract",
    {
      url,
      selector: options.selector,
      visibility: options.visibility,
      ...common(options),
    } satisfies ExtractRequest,
    options.signal,
  );
}

/** Extract structured data using a declarative CSS-selector schema. */
export async function extractSchema<T = unknown>(
  url: string,
  schema: Schema,
  options: FetchOptions = {},
): Promise<T> {
  const result = await request<SchemaExtractResult>(
    "extractSchema",
    {
      url,
      schema: schemaToValue(schema),
      visibility: options.visibility,
      ...common(options),
    } satisfies SchemaExtractRequest,
    options.signal,
  );
  return result.extracted as T;
}

/** Capture a PNG screenshot of the rendered page. */
export async function screenshot(
  url: string,
  options: FetchOptions & { fullPage?: boolean } = {},
): Promise<Buffer> {
  const png = await request<string>(
    "screenshot",
    { url, fullPage: options.fullPage, ...common(options) } satisfies ScreenshotRequest,
    options.signal,
  );
  return Buffer.from(png, "base64");
}

/** Execute JavaScript in the page and return the result as a string. */
export async function evaluate(
  url: string,
  expression: string,
  options: FetchOptions = {},
): Promise<string> {
  const result = await request<EvaluateResult>(
    "evaluate",
    { url, expression, ...common(options) } satisfies EvaluateRequest,
    options.signal,
  );
  return result.result;
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

/** Crawl a site, yielding each page as it completes (BFS, respects robots.txt). */
export async function* crawl(url: string, options: CrawlOptions = {}): AsyncGenerator<CrawlResult> {
  const params = {
    url,
    limit: options.limit,
    maxDepth: options.maxDepth,
    include: asArray(options.include),
    exclude: asArray(options.exclude),
    concurrency: options.concurrency,
    delayMs: options.delayMs,
    selector: options.selector,
    ...common(options),
  } satisfies CrawlRequest;
  for await (const event of stream<CrawlEvent>("crawl", params, options.signal)) {
    if (event.type === "stats") continue;
    if (event.type === "error") {
      yield {
        ok: false,
        url: event.url,
        depth: event.depth,
        fetchedAt: event.fetchedAt,
        error: event.error,
      };
    } else {
      yield {
        ok: true,
        url: event.url,
        depth: event.depth,
        fetchedAt: event.fetchedAt,
        title: event.title ?? null,
        content: event.content,
        linksFound: event.linksFound,
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
export function map(url: string, options: MapOptions = {}): Promise<MappedUrl[]> {
  return request(
    "map",
    {
      url,
      limit: options.limit,
      include: asArray(options.include),
      exclude: asArray(options.exclude),
      noFallback: options.noFallback,
      userAgent: options.userAgent,
      timeout: options.timeout,
      headers: options.headers,
    } satisfies MapRequest,
    options.signal,
  );
}

/** The version of the underlying servo-fetch binary. */
export async function version(): Promise<string> {
  const info = await request<InitializeResult>("initialize", {});
  return info.serverInfo.version;
}
