# servo-fetch-cli

[![crates.io](https://img.shields.io/crates/v/servo-fetch-cli.svg)](https://crates.io/crates/servo-fetch-cli)

A browser engine in a binary — fetch, render, and extract web content as Markdown, JSON, or screenshots. Powered by [Servo](https://servo.org/).

For programmatic use in Rust, see the [`servo-fetch`](https://crates.io/crates/servo-fetch) library crate.

## Install

### Pre-built binaries (recommended)

```bash
curl -fsSL https://raw.githubusercontent.com/konippi/servo-fetch/main/install.sh | sh
```

### Cargo

```bash
cargo binstall servo-fetch-cli   # prebuilt binary via cargo-binstall
cargo install servo-fetch-cli    # build from source (requires Rust 1.86.0+)
```

## Usage

### Extract content

```bash
servo-fetch "https://example.com"              # Readable Markdown (default)
servo-fetch "https://example.com" --json       # Structured JSON
servo-fetch "https://example.com" --raw html   # Raw rendered HTML
servo-fetch "https://example.com" --raw text   # Plain text (innerText)
```

### Batch fetch

```bash
servo-fetch URL1 URL2 URL3                     # Parallel fetch, Markdown output
servo-fetch URL1 URL2 --json                   # Parallel fetch, NDJSON output
```

### Screenshots

```bash
servo-fetch "https://example.com" --screenshot page.png
servo-fetch "https://example.com" --screenshot full.png --full-page
```

### JavaScript execution

```bash
servo-fetch "https://example.com" --js "document.title"
servo-fetch "https://example.com" --js "document.querySelectorAll('h2').length"
```

### CSS selector extraction

```bash
servo-fetch "https://example.com" --selector "article"
servo-fetch "https://example.com" --selector ".main-content" --json
```

### Crawl a site

```bash
servo-fetch crawl "https://docs.example.com" --limit 20
servo-fetch crawl "https://docs.example.com" --include "/docs/**" --exclude "/docs/archive/**"
servo-fetch crawl "https://docs.example.com" --json --max-depth 5
```

### Discover URLs (sitemap)

```bash
servo-fetch map "https://example.com"
servo-fetch map "https://example.com" --limit 100 --include "/blog/**"
```

### SPA / dynamic content

```bash
servo-fetch "https://spa.example.com" --settle 3000       # Wait 3s after load for hydration
servo-fetch "https://spa.example.com" -t 60 --settle 5000 # 60s timeout + 5s settle
```

### MCP server

```bash
servo-fetch mcp                # stdio transport (for AI agents)
servo-fetch mcp --port 8080    # Streamable HTTP transport
```

### HTTP API server

```bash
servo-fetch serve                            # 127.0.0.1:3000
servo-fetch serve --host 0.0.0.0 --port 80   # expose to network
```

See [HTTP API server](#http-api-server) below for the endpoint reference.

## Options

| Flag | Description |
| ---- | ----------- |
| `--json` | Structured JSON output (NDJSON for multiple URLs) |
| `--screenshot <FILE>` | Save PNG screenshot |
| `--full-page` | Capture full scrollable page (requires `--screenshot`) |
| `--js <EXPR>` | Execute JavaScript and print result |
| `--selector <CSS>` | Extract specific section by CSS selector |
| `--raw html\|text` | Raw HTML or plain text output |
| `-t, --timeout <SECS>` | Page load timeout in seconds (default: 30) |
| `--settle <MS>` | Extra wait after load event in ms (default: 0, max: 10000) |
| `--user-agent <UA>` | Override the User-Agent string |
| `-v, --verbose` | Increase log verbosity (`-v` info, `-vv` debug, `-vvv` trace) |
| `-q, --quiet` | Suppress all logs except errors |

### JSON output

`--json` returns an object with these fields:

| Field | Type | Description |
| ----- | ---- | ----------- |
| `title` | string | Page title |
| `content` | string | Raw HTML extracted by Readability |
| `text_content` | string | Readable text (Markdown) |
| `byline` | string | Author or byline (omitted if not detected) |
| `excerpt` | string | Short excerpt or description (omitted if not detected) |
| `lang` | string | Document language (omitted if not detected) |
| `url` | string | Canonical URL (omitted if not detected) |

### Crawl subcommand

`servo-fetch crawl <URL>` follows same-site links using BFS. Respects `robots.txt` (RFC 9309) with a minimum 500ms interval.

| Flag | Description |
| ---- | ----------- |
| `--limit <N>` | Maximum pages to crawl (default: 50) |
| `--max-depth <N>` | Maximum link depth (default: 3) |
| `--include <GLOB>` | URL path patterns to include |
| `--exclude <GLOB>` | URL path patterns to exclude |
| `--json` | Output content as JSON per page |
| `--selector <CSS>` | Extract specific section per page |
| `--user-agent <UA>` | Override the User-Agent string |
| `-t, --timeout <SECS>` | Page load timeout in seconds per page (default: 30) |
| `--settle <MS>` | Extra wait after load event in ms per page (default: 0, max: 10000) |

### Map subcommand

`servo-fetch map <URL>` discovers all URLs on a site via sitemaps without rendering. Falls back to HTML link extraction if no sitemap exists.

| Flag | Description |
| ---- | ----------- |
| `--limit <N>` | Maximum URLs to return (default: 5000) |
| `--include <GLOB>` | URL path patterns to include |
| `--exclude <GLOB>` | URL path patterns to exclude |
| `--json` | Output as JSON array with lastmod metadata |
| `--user-agent <UA>` | Override the User-Agent string |
| `-t, --timeout <SECS>` | HTTP request timeout in seconds (default: 30) |
| `--no-fallback` | Skip HTML link extraction fallback |

## Logging

Diagnostic messages go to stderr; stdout is reserved for data output so pipes stay clean.

```bash
servo-fetch -v "https://example.com"                       # info and above
servo-fetch -vv "https://example.com"                      # debug
servo-fetch -vvv "https://example.com"                     # trace
servo-fetch -q "https://example.com"                       # errors only
RUST_LOG="servo_fetch=debug" servo-fetch "https://..."     # fine-grained override
RUST_LOG="servo_fetch=trace,servo=debug" servo-fetch "..." # include Servo internals
```

`RUST_LOG` uses [`tracing-subscriber`'s directive syntax](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html) and always wins over CLI flags.

## Environment Variables

| Variable | Description |
| -------- | ----------- |
| `SERVO_FETCH_USER_AGENT` | Default User-Agent string (overridden by `--user-agent`) |
| `SERVO_FETCH_NO_STDERR_FILTER` | Disable Apple OpenGL driver noise filter (debug use) |
| `RUST_LOG` | Fine-grained log filter (overrides `-v`/`-q`) |

## MCP Server

Built-in [Model Context Protocol](https://modelcontextprotocol.io/) server over stdio or Streamable HTTP.

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

Streamable HTTP: `servo-fetch mcp --port 8080`

<details>
<summary><b>fetch</b> — extract readable content from a URL</summary>

| Parameter | Type | Description |
| --------- | ---- | ----------- |
| `url` | string | URL to fetch (http/https only) |
| `format` | string? | `markdown` (default), `json`, `html`, `text`, or `accessibility_tree` |
| `max_length` | number? | Max characters to return (default 5000) |
| `start_index` | number? | Character offset for pagination (default 0) |
| `timeout` | number? | Page load timeout in seconds (default 30) |
| `settle_ms` | number? | Extra wait in ms after load event (default 0, max 10000) |
| `selector` | string? | CSS selector to extract a specific section |

</details>

<details>
<summary><b>batch_fetch</b> — fetch multiple URLs in parallel</summary>

| Parameter | Type | Description |
| --------- | ---- | ----------- |
| `urls` | string[] | URLs to fetch (http/https only, max 20) |
| `format` | string? | `markdown` (default) or `json` |
| `max_length` | number? | Max characters per URL result (default 5000) |
| `timeout` | number? | Page load timeout in seconds per URL (default 30) |
| `settle_ms` | number? | Extra wait in ms after load event (default 0, max 10000) |
| `selector` | string? | CSS selector to extract a specific section |

</details>

<details>
<summary><b>crawl</b> — crawl a website by following links</summary>

| Parameter | Type | Description |
| --------- | ---- | ----------- |
| `url` | string | Starting URL (http/https only) |
| `limit` | number? | Maximum pages to crawl (default 20, max 500) |
| `max_depth` | number? | Maximum link depth from seed (default 3, max 10) |
| `format` | string? | `markdown` (default) or `json` |
| `include_glob` | string[]? | URL path patterns to include |
| `exclude_glob` | string[]? | URL path patterns to exclude |
| `max_length` | number? | Max characters per page result (default 5000) |
| `timeout` | number? | Page load timeout in seconds per page (default 30) |
| `settle_ms` | number? | Extra wait in ms after load event (default 0, max 10000) |
| `selector` | string? | CSS selector to extract a specific section per page |

</details>

<details>
<summary><b>map</b> — discover URLs via sitemaps without rendering</summary>

| Parameter | Type | Description |
| --------- | ---- | ----------- |
| `url` | string | Site URL to discover pages for (http/https only) |
| `limit` | number? | Maximum URLs to return (default 5000) |
| `include_glob` | string[]? | URL path patterns to include |
| `exclude_glob` | string[]? | URL path patterns to exclude |

</details>

<details>
<summary><b>screenshot</b> — capture a PNG screenshot (no GPU required)</summary>

| Parameter | Type | Description |
| --------- | ---- | ----------- |
| `url` | string | URL to capture (http/https only) |
| `full_page` | boolean? | Capture the full scrollable page (default false) |
| `timeout` | number? | Page load timeout in seconds (default 30) |
| `settle_ms` | number? | Extra wait in ms after load event (default 0, max 10000) |

</details>

<details>
<summary><b>execute_js</b> — evaluate JavaScript in a loaded page</summary>

| Parameter | Type | Description |
| --------- | ---- | ----------- |
| `url` | string | URL to load before executing JS |
| `expression` | string | JavaScript expression to evaluate |
| `timeout` | number? | Page load timeout in seconds (default 30) |
| `settle_ms` | number? | Extra wait in ms after load event (default 0, max 10000) |

</details>

## HTTP API server

`servo-fetch serve` starts a REST API on the given host/port (default `127.0.0.1:3000`). JSON request/response, binary PNG for screenshots.

| Flag | Description |
| ---- | ----------- |
| `--host <HOST>` | Bind address (default `127.0.0.1`) |
| `--port <PORT>` | TCP port (default `3000`) |

Responses include `x-request-id` (auto-generated if the request does not supply one); use this for tracing in logs. Errors use a consistent `{"error": "..."}` JSON shape across all endpoints. Request bodies are capped at 1 MiB. SSRF protection (private/reserved address blocking) applies to every endpoint.

### Endpoints

| Method | Path | Description |
| ------ | ---- | ----------- |
| `GET` | `/health` | Liveness probe (`{"status":"ok"}`) |
| `GET` | `/version` | `{"name":"servo-fetch","version":"..."}` |
| `POST` | `/v1/fetch` | Fetch one URL; returns extracted content |
| `POST` | `/v1/batch_fetch` | Fetch up to 20 URLs in parallel |
| `POST` | `/v1/screenshot` | Capture a PNG; `image/png` body |
| `POST` | `/v1/execute_js` | Evaluate JavaScript in a loaded page |
| `POST` | `/v1/crawl` | BFS crawl starting from a URL |
| `POST` | `/v1/map` | Discover URLs via sitemaps (no rendering) |

Request and response shapes mirror the MCP tool parameters documented above.

### Examples

```bash
# Fetch → Markdown
curl -X POST http://127.0.0.1:3000/v1/fetch \
  -H 'content-type: application/json' \
  -d '{"url":"https://example.com"}'

# Screenshot → PNG
curl -X POST http://127.0.0.1:3000/v1/screenshot \
  -H 'content-type: application/json' \
  -d '{"url":"https://example.com","full_page":true}' \
  -o page.png
```

## Docker

Multi-arch image (`linux/amd64`, `linux/arm64`) on GitHub Container Registry:

```bash
docker run --rm -p 3000:3000 ghcr.io/konippi/servo-fetch:latest
```

Override the default `serve` with any `servo-fetch` subcommand:

```bash
docker run --rm ghcr.io/konippi/servo-fetch:latest https://example.com
docker run --rm ghcr.io/konippi/servo-fetch:latest --version
```

Minimum-privilege deployment:

```bash
docker run --rm -p 3000:3000 \
  --read-only --tmpfs /tmp \
  --cap-drop=ALL \
  --security-opt=no-new-privileges \
  ghcr.io/konippi/servo-fetch:latest
```

The image runs as UID 1001 and includes a `HEALTHCHECK` against `/health`.

### Image signing

Images are signed with [cosign](https://github.com/sigstore/cosign) keyless via GitHub OIDC and ship with [SLSA build provenance](https://slsa.dev/) and an [SPDX SBOM](https://spdx.dev/) as OCI attestations.

```bash
cosign verify ghcr.io/konippi/servo-fetch:latest \
  --certificate-identity-regexp '^https://github.com/konippi/servo-fetch/' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com
```
