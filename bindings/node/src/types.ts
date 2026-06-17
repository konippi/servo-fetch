import type { Visibility } from "./generated/index.js";

/** Options shared by every single-page operation. */
export interface FetchOptions {
  /** Page-load timeout in seconds. Default: 30. */
  timeout?: number;
  /** Extra wait in milliseconds after the `load` event, for SPAs. */
  settle?: number;
  /** Override the `User-Agent` header. */
  userAgent?: string;
  /** Path to a Netscape-format `cookies.txt` file. */
  cookiesFile?: string;
  /** CSS selector to extract a specific section. */
  selector?: string;
  /** Visibility filtering policy. Default: "moderate". */
  visibility?: Visibility;
  /** Custom request headers sent with the navigation request. */
  headers?: Record<string, string>;
  /** Abort the in-flight request. */
  signal?: AbortSignal;
}

/** Per-URL result from {@link batchFetch}. */
export type BatchResult =
  | { url: string; ok: true; markdown: string }
  | { url: string; ok: false; error: string };

export interface CrawlOptions {
  /** Maximum number of pages to crawl. Default: 50. */
  limit?: number;
  /** Maximum link depth from the seed URL. Default: 3. */
  maxDepth?: number;
  /** URL path glob patterns to include (e.g. "/docs/**"). */
  include?: string | string[];
  /** URL path glob patterns to exclude. */
  exclude?: string | string[];
  /** Maximum parallel page fetches. Default: 1. */
  concurrency?: number;
  /** Minimum dispatch interval in milliseconds (0 to disable). */
  delayMs?: number;
  /** Per-page timeout in seconds. */
  timeout?: number;
  /** Extra wait in milliseconds after load, per page. */
  settle?: number;
  /** Override the `User-Agent` header. */
  userAgent?: string;
  /** Path to a Netscape-format `cookies.txt` file. */
  cookiesFile?: string;
  /** CSS selector to extract a specific section per page. */
  selector?: string;
  /** Custom request headers sent with each page's navigation request. */
  headers?: Record<string, string>;
  signal?: AbortSignal;
}

/** One page (or error) yielded by {@link crawl}. */
export type CrawlResult =
  | {
      ok: true;
      url: string;
      depth: number;
      fetchedAt: string;
      title: string | null;
      content: string;
      linksFound: number;
    }
  | { ok: false; url: string; depth: number; fetchedAt: string; error: string };

export interface MapOptions {
  /** Maximum number of URLs to discover. Default: 5000. */
  limit?: number;
  /** URL path glob patterns to include (e.g. "/docs/**"). */
  include?: string | string[];
  /** URL path glob patterns to exclude. */
  exclude?: string | string[];
  /** Override the `User-Agent` header. */
  userAgent?: string;
  /** Per-request timeout in seconds. */
  timeout?: number;
  /** Skip the HTML link fallback when no sitemap is found. */
  noFallback?: boolean;
  /** Custom request headers sent with each discovery request. */
  headers?: Record<string, string>;
  signal?: AbortSignal;
}
