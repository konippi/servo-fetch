# servo-fetch Reference Guide

## Pagination

Large pages are truncated at `max_length` (default 5000 characters). The response includes a hint:

```text
<content truncated. total_length=42000, next start_index=5000>
```

Fetch the next chunk:

```text
fetch(url: "https://...", start_index: 5000)
```

## Batch fetching

Use `batch_fetch` to fetch multiple URLs in a single call. Results stream back in completion order — faster pages arrive first.

```text
batch_fetch(urls: ["https://a.com", "https://b.com", "https://c.com"])
```

Each URL becomes a separate content entry in the response. Failed URLs are reported inline (prefixed with `[error]`) without aborting the batch.

CLI equivalent:

```bash
servo-fetch https://a.com https://b.com https://c.com          # Markdown
servo-fetch https://a.com https://b.com https://c.com --json   # NDJSON
```

## Crawling

Use `crawl` to follow links within a site and extract content from multiple pages. Results stream as each page completes.

```text
crawl(url: "https://docs.example.com", limit: 20, max_depth: 3)
crawl(url: "https://docs.example.com", include_glob: ["/guide/**"])
```

Crawl follows same-site links only (eTLD+1), respects `robots.txt`, and applies a 500ms interval between requests by default (CLI: `--delay-ms`, library: `CrawlOptions::delay()`). Output is one content entry per page.

CLI equivalent:

```bash
servo-fetch crawl "https://docs.example.com" --limit 20 --max-depth 3
servo-fetch crawl "https://docs.example.com" --include "/guide/**"
```

## Format selection

| Goal | Format |
| ---- | ------ |
| Read content, summarize, answer questions | `markdown` (default) |
| Extract title, byline, excerpt, language | `json` |
| Get raw HTML for further processing | `html` |
| Get plain text (document.body.innerText) | `text` |
| Get page structure with roles and bounding boxes | `accessibility_tree` |

## Selector extraction

Use `selector` to extract a specific section instead of full-page Readability:

```text
fetch(url: "https://example.com", selector: "article")
fetch(url: "https://example.com", selector: ".main-content", format: "json")
```

## Schema extraction

For structured data (product catalogs, listings, comment threads), use `--schema` on the CLI with a schema file. No LLM required — selectors pull fields declaratively.

```bash
servo-fetch "https://shop.example.com" --schema schema.json
servo-fetch URL1 URL2 --schema schema.json    # batch → NDJSON
```

Schema:

```json
{
  "base_selector": ".product",
  "fields": [
    { "name": "title", "selector": "h2", "type": "text" },
    { "name": "price", "selector": ".price", "type": "text" },
    { "name": "url", "selector": "a", "type": "attribute", "attribute": "href" }
  ]
}
```

Field `type` values: `text`, `attribute`, `html`, `inner_html`, `nested_list`. An empty selector (`""`) reads from the matched element itself — handy inside `nested_list` when you want each item's own text or attribute. Schema selectors are validated at load time.

## Troubleshooting

| Symptom | Solution |
| ------- | -------- |
| Empty content | Site may require JS features not yet supported by Servo. Try `execute_js` with `document.body.innerText` |
| Timeout | Increase timeout: `fetch(url: "...", timeout: 60)` |
| Blocked URL | URL resolves to a private IP (SSRF protection). Use a public URL |
| Noisy output | Try `selector` to target the main content area, e.g. `selector: "article"` or `selector: "main"` |

## Screenshots

Default viewport is 1280×800. Screenshots are rendered with Servo's software renderer (no GPU).

## Accessibility tree

The `accessibility_tree` format returns a JSON object of all AccessKit nodes with roles, names, and bounding boxes. Password input values are automatically masked.

## MCP configuration

### stdio (default)

```json
{
  "mcpServers": {
    "servo-fetch": {
      "command": "servo-fetch",
      "args": ["mcp"]
    }
  }
}
```

### Streamable HTTP

```bash
servo-fetch mcp --port 8080
```

Connect your MCP client to `http://127.0.0.1:8080/mcp`.

### HTTP REST API

For non-MCP HTTP clients (direct REST calls, Docker deployments):

```bash
servo-fetch serve --port 3000
```

`POST /v1/fetch`, `/v1/batch_fetch`, `/v1/screenshot`, `/v1/execute_js`, `/v1/crawl`, `/v1/map` accept the same parameters as the MCP tools as JSON bodies. `GET /health` and `/version` are also exposed.

### Docker

Prebuilt multi-arch image published on every release:

```bash
docker run --rm -p 3000:3000 ghcr.io/konippi/servo-fetch:latest
```

The container runs the HTTP API on port 3000 as non-root (UID 1001) and exposes the same endpoints as above.
