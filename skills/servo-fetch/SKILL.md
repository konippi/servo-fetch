---
name: servo-fetch
description: "Fetch and render web pages using the Servo browser engine — a single binary with JS execution, CSS layout, screenshots, and content extraction. Use when a URL returns empty or incomplete content with plain HTTP fetch, when you need a screenshot without GPU, or when you need to run JavaScript in a page context. No browser download required."
---

# servo-fetch

## When to use

- A URL returns empty or incomplete content with simple HTTP fetch (SPA, React, Vue)
- You need a screenshot of a web page in CI/Docker (no GPU available)
- You need to evaluate JavaScript in a page context (DOM queries, data extraction)
- You want clean Markdown from a documentation site, blog, or article
- You need to crawl an entire documentation site or blog for RAG / knowledge ingestion
- You need the accessibility tree with bounding boxes for a page

## When NOT to use

- The page is simple static HTML (use `curl` or built-in web fetch instead)
- You need to interact with the page (click, fill forms) — servo-fetch is read-only
- You need full Chromium compatibility for complex web apps

## Tools (MCP)

Start the MCP server: `servo-fetch mcp` (stdio) or `servo-fetch mcp --port 8080` (Streamable HTTP)

### fetch

Extract readable content from a URL. JavaScript is executed, CSS layout is computed, and navigation noise (navbars, sidebars, footers, cookie banners) is stripped automatically.

Parameters:

- `url` (required): URL to fetch (http/https only)
- `format`: `"markdown"` (default), `"json"`, `"html"`, `"text"`, or `"accessibility_tree"`
- `selector`: CSS selector to extract a specific section instead of full-page extraction
- `max_length`: max characters to return (default 5000)
- `start_index`: character offset for pagination
- `timeout`: page load timeout in seconds (default 30)
- `settle_ms`: extra wait in ms after load event for SPAs (default 0, max 10000)

```text
fetch(url: "https://docs.rs/tokio", format: "markdown")
fetch(url: "https://example.com", format: "json", selector: "article")
fetch(url: "https://example.com", format: "accessibility_tree")
```

PDF URLs are auto-detected via Content-Type and extracted directly.

### batch_fetch

Fetch multiple URLs in parallel. Results are returned as separate content entries in completion order. Failed URLs are reported inline without aborting the batch.

Parameters:

- `urls` (required): array of URLs to fetch (http/https only, max 20)
- `format`: `"markdown"` (default) or `"json"`
- `selector`: CSS selector to extract a specific section
- `max_length`: max characters per URL result (default 5000)
- `timeout`: page load timeout in seconds per URL (default 30)
- `settle_ms`: extra wait in ms after load event (default 0, max 10000)

```text
batch_fetch(urls: ["https://a.com", "https://b.com"], format: "markdown")
batch_fetch(urls: ["https://a.com", "https://b.com"], format: "json", selector: "article")
```

### crawl

Crawl a website starting from a URL, following same-site links via BFS. JavaScript is executed, CSS layout is computed, and navigation noise is stripped. Respects robots.txt.

Parameters:

- `url` (required): starting URL to crawl (http/https only)
- `limit`: max pages to crawl (default 20, max 500)
- `max_depth`: max link depth from seed (default 3, max 10)
- `format`: `"markdown"` (default) or `"json"`
- `include_glob`: URL path patterns to include (e.g. `["/docs/**"]`)
- `exclude_glob`: URL path patterns to exclude
- `max_length`: max characters per page result (default 5000)
- `timeout`: page load timeout in seconds per page (default 30)
- `settle_ms`: extra wait in ms after load event (default 0, max 10000)
- `selector`: CSS selector to extract a specific section per page

```text
crawl(url: "https://docs.example.com", limit: 20, max_depth: 3)
crawl(url: "https://docs.example.com", include_glob: ["/guide/**"], limit: 50)
```

### screenshot

Capture a PNG screenshot. Uses Servo's software renderer — works without GPU.

Parameters:

- `url` (required): URL to capture
- `full_page`: capture the full scrollable page (default false)
- `timeout`: page load timeout in seconds (default 30)
- `settle_ms`: extra wait in ms after load event (default 0, max 10000)

```text
screenshot(url: "https://example.com")
screenshot(url: "https://example.com", full_page: true)
```

### execute_js

Evaluate a JavaScript expression after the page loads. Console messages (log, warn, error) are appended to the result.

Parameters:

- `url` (required): URL to load
- `expression` (required): JavaScript expression to evaluate
- `timeout`: page load timeout in seconds (default 30)
- `settle_ms`: extra wait in ms after load event (default 0, max 10000)

```text
execute_js(url: "https://example.com", expression: "document.title")
execute_js(url: "https://example.com", expression: "[...document.querySelectorAll('h2')].map(e => e.textContent)")
```

## CLI

```bash
servo-fetch https://example.com                    # Markdown (default)
servo-fetch https://example.com --json             # Structured JSON
servo-fetch URL1 URL2 URL3                         # Parallel batch (Markdown with separators)
servo-fetch URL1 URL2 --json                       # Parallel batch (NDJSON)
servo-fetch https://example.com --screenshot out.png
servo-fetch https://example.com --js "document.title"
servo-fetch https://example.com --selector article
servo-fetch https://example.com --schema schema.json  # Schema-driven JSON
servo-fetch https://example.com --raw html         # Raw HTML
servo-fetch https://example.com --raw text         # Plain text
servo-fetch https://example.com -t 60              # Custom timeout
servo-fetch https://example.com --settle 500       # Extra wait for SPAs
servo-fetch crawl https://docs.example.com --limit 20  # Crawl a site (BFS)
servo-fetch crawl https://docs.example.com --include "/docs/**"  # Crawl with path filter
```

## Gotchas

- Servo's web compatibility is improving but not at Chromium level — best for docs, blogs, and SSR sites
- Private/reserved IP addresses are blocked (SSRF protection)
- Default timeout is 30 seconds; increase with `timeout` parameter for slow pages
- Cookie banners and newsletter popups are stripped via injected user stylesheets

For pagination patterns, format selection, and MCP configuration, see `references/guide.md`.
