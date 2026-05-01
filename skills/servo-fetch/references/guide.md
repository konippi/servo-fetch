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

Crawl follows same-site links only (eTLD+1), respects `robots.txt`, and enforces a minimum 500ms interval between requests. Output is one content entry per page.

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
