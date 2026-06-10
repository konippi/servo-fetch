# servo-fetch

[![CI](https://github.com/konippi/servo-fetch/actions/workflows/ci.yml/badge.svg)](https://github.com/konippi/servo-fetch/actions/workflows/ci.yml)
[![npm](https://img.shields.io/npm/v/servo-fetch)](https://www.npmjs.com/package/servo-fetch)
[![node](https://img.shields.io/node/v/servo-fetch)](https://www.npmjs.com/package/servo-fetch)
[![pnpm](https://img.shields.io/badge/pnpm-f69220?logo=pnpm&logoColor=white)](https://pnpm.io/)
[![Biome](https://img.shields.io/badge/Biome-648fff?style=flat&logo=biome)](https://biomejs.dev)

Node.js bindings for [servo-fetch](https://github.com/konippi/servo-fetch) — fetch, render, and extract web content with an embedded Servo browser engine.

- **No Chromium** — single self-contained binary, bundled in the package
- **JavaScript execution** — full Servo engine with SpiderMonkey
- **Schema extraction** — declarative CSS-selector → structured JSON, no LLM
- **Streaming crawl** — `for await` over pages as they complete
- **Typed** — first-class TypeScript types, ESM and CommonJS

## Install

```bash
npm install servo-fetch
```

The prebuilt binary for your platform is selected automatically through
[`optionalDependencies`](https://docs.npmjs.com/cli/configuring-npm/package-json#optionaldependencies).
Supported targets: macOS (arm64, x64), Linux glibc (x64, arm64), Windows x64.

## Quick Start

```ts
import { fetch, extract, crawl, map, screenshot } from "servo-fetch";

const md = await fetch("https://example.com"); // readable Markdown

const article = await extract("https://example.com"); // Readability data
console.log(article.title, article.textContent);

for await (const page of crawl("https://docs.example.com", { limit: 50 })) {
  if (page.ok) console.log(page.url, page.title);
}

const urls = await map("https://example.com"); // sitemap discovery, no render
const png = await screenshot("https://example.com", { fullPage: true });
```

## Schema Extraction

```ts
import { extractSchema } from "servo-fetch";

const products = await extractSchema("https://shop.example.com", {
  baseSelector: ".product",
  fields: [
    { name: "title", selector: "h2", type: "text" },
    { name: "price", selector: ".price", type: "text" },
    { name: "url", selector: "a", type: "attribute", attribute: "href" },
  ],
});
```

## CLI

The bundled binary is also runnable directly:

```bash
npx servo-fetch "https://example.com"
npx servo-fetch "https://example.com" --format png -o page.png
```

## API

| Function | Returns | CLI equivalent |
| --- | --- | --- |
| `fetch(url, opts?)` | `Promise<string>` (Markdown) | default |
| `fetchHtml(url, opts?)` | `Promise<string>` | `--format html` |
| `fetchText(url, opts?)` | `Promise<string>` | `--format text` |
| `extract(url, opts?)` | `Promise<Article>` | `--format json` |
| `extractSchema<T>(url, schema, opts?)` | `Promise<T>` | `--schema` |
| `screenshot(url, opts?)` | `Promise<Buffer>` | `--format png` |
| `evaluate(url, expr, opts?)` | `Promise<string>` | `--js` |
| `batchFetch(urls, opts?)` | `Promise<BatchResult[]>` | parallel fetch |
| `crawl(url, opts?)` | `AsyncGenerator<CrawlResult>` | `crawl` |
| `crawlAll(url, opts?)` | `Promise<CrawlResult[]>` | `crawl` |
| `map(url, opts?)` | `Promise<MappedUrl[]>` | `map` |
| `version()` | `Promise<string>` | `--version` |

Single-page calls accept `{ timeout, settle, userAgent, cookiesFile, selector, visibility, signal }`.
`SERVO_FETCH_BINARY_PATH` overrides binary resolution.

## Develop

Requires [pnpm](https://pnpm.io/).

```bash
pnpm install
pnpm run build      # tsdown → dist (ESM + CJS + d.ts)
pnpm test           # vitest
pnpm run typecheck  # tsc --noEmit
pnpm run lint       # biome (lint + format + import sort)
```
