---
name: servo-fetch
description: "Use when you need to fetch content from JavaScript-heavy sites, take screenshots without GPU, or run DOM queries — all from a single binary with no Chromium. Powered by the Servo browser engine with full JS execution and CSS rendering."
---

# servo-fetch

## When to use

- A URL returns empty or incomplete content with simple HTTP fetch (SPA, React, Vue, Angular)
- You need a screenshot of a web page in CI/Docker (no GPU available)
- You need to evaluate JavaScript in a page context (DOM queries, data extraction)
- You want clean Markdown from a documentation site, blog, or article

## When NOT to use

- The page is a simple static HTML page (use `curl` or built-in web fetch instead)
- You need to interact with the page (click, fill forms) — servo-fetch is read-only
- You need full Chromium compatibility for complex web apps

## Tools (MCP)

Start the MCP server: `servo-fetch mcp`

### fetch

Extract readable content from a URL. JavaScript is executed, CSS layout is computed, and navigation noise (navbars, sidebars, footers) is stripped automatically.

```
fetch(url: "https://docs.rs/tokio", format: "markdown")
```

- `format`: `"markdown"` (default) for readable text, `"json"` for structured data with title/byline/excerpt/language
- `max_length`: max characters to return (default 5000)
- `start_index`: character offset for pagination — check the `next start_index` value in truncated responses

### screenshot

Capture a PNG screenshot. Uses Servo's software renderer — works in Docker, CI, and headless servers without GPU.

```
screenshot(url: "https://example.com")
```

### execute_js

Evaluate a JavaScript expression after the page loads. Use for DOM queries and data extraction.

```
execute_js(url: "https://example.com", expression: "document.title")
execute_js(url: "https://example.com", expression: "[...document.querySelectorAll('h2')].map(e => e.textContent)")
```

## CLI usage

```bash
servo-fetch https://example.com              # Markdown (default)
servo-fetch https://example.com --json       # Structured JSON
servo-fetch https://example.com --screenshot page.png
servo-fetch https://example.com --js "document.title"
```

## Limitations

- Best for documentation, blogs, and SSR sites
- Some SPAs with complex client-side rendering may not fully render
- Default timeout is 30 seconds (configurable with `timeout` parameter)
- Private/reserved IP addresses are blocked (SSRF protection)

For pagination patterns, format selection, and troubleshooting, see reference.md.
