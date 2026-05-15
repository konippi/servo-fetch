<div align="center">
  <h1 align="center">servo-fetch</h1>
  <p align="center">A self-contained browser engine that fetches, renders, and extracts web content as Markdown, JSON, or screenshots — no Chromium, no API key, no setup.</p>
  <p>
    <a href="https://github.com/konippi/servo-fetch/actions"><img src="https://github.com/konippi/servo-fetch/workflows/CI/badge.svg" alt="CI"></a>
    <a href="https://crates.io/crates/servo-fetch"><img src="https://img.shields.io/crates/v/servo-fetch.svg" alt="crates.io"></a>
    <img src="https://img.shields.io/badge/Rust-1.86.0-blue?color=fc8d62&logo=rust" alt="MSRV">
    <img src="https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg" alt="MIT OR Apache-2.0">
  </p>
  <img src="assets/demo.gif" alt="servo-fetch demo" width="900">
</div>

servo-fetch embeds the [Servo](https://servo.org/) browser engine. It executes JavaScript, computes CSS layout,
captures screenshots with a software renderer, and extracts clean content — available as both a CLI tool and a
Rust library.

```bash
servo-fetch "https://example.com"                        # CLI: clean Markdown
servo-fetch "https://example.com" --screenshot page.png  # CLI: PNG screenshot
```

```rust
let md = servo_fetch::markdown("https://example.com")?;  // Library: one-liner
```

## Why servo-fetch

- **Zero dependencies** — single binary, no Chromium, no API key
- **Real JS execution** — SpiderMonkey runs JavaScript, parallel CSS engine computes layout
- **Layout-aware extraction** — strips navbars, sidebars, footers by actual rendered position
- **Schema-driven JSON** — declarative CSS-selector schema pulls structured data
- **Parallel batch fetch** — multiple URLs fetched concurrently
- **Site crawling** — BFS link traversal with robots.txt, same-site scope, and rate limiting
- **URL discovery** — sitemap-based URL mapping without rendering (fast, lightweight)
- **Screenshots without GPU** — software renderer captures PNG/full-page screenshots anywhere
- **Accessibility tree** — AccessKit integration with roles, names, and bounding boxes

## Performance and quality

Apple M3 Pro, versus Playwright (the typical AI-agent stack):

| Benchmark           | servo-fetch | playwright:optimized |
| ------------------- | ----------: | -------------------: |
| Time — static-small |     ~231 ms |              ~645 ms |
| Time — spa-heavy    |     ~331 ms |              ~798 ms |
| Memory (peak RSS)   |    51–64 MB |           300–328 MB |

Extraction quality: mean word-F1 0.823 vs Readability's 0.723 across
seven page-type fixtures, with `without[]` boilerplate removal at 96.4%
vs 82.1%. Direct-binary engine peers (chrome-headless-shell, Lightpanda,
curl) are opt-in.

Methodology, three-axis breakdown, per-fixture F1, and raw JSON:
[`benchmarks/README.md`](benchmarks/README.md) +
[`benchmarks/results/`](benchmarks/results/).

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/konippi/servo-fetch/main/install.sh | sh
```

Or via [GitHub Releases](https://github.com/konippi/servo-fetch/releases), or with Cargo (requires Rust 1.86.0+):

```bash
cargo binstall servo-fetch-cli   # prebuilt binary
cargo install servo-fetch-cli    # build from source
```

<details>
<summary><b>Platform notes</b></summary>

**Linux** — install runtime deps and use `xvfb-run` on headless servers:

```bash
sudo apt install -y libegl1 libfontconfig1 libfreetype6
xvfb-run --auto-servernum servo-fetch "https://example.com"
```

**Windows** — keep `servo-fetch.exe`, `libEGL.dll`, and `libGLESv2.dll` in the same directory.

**macOS** — no extra setup needed.

</details>

## Quick Start

### CLI

```bash
servo-fetch "https://example.com"                        # Markdown (default)
servo-fetch "https://example.com" --json                 # Structured JSON
servo-fetch "https://example.com" --screenshot page.png  # PNG screenshot
servo-fetch "https://example.com" --js "document.title"  # Run JavaScript
servo-fetch "https://example.com" --schema schema.json   # Schema-driven JSON
servo-fetch URL1 URL2 URL3                               # Parallel batch
servo-fetch crawl "https://docs.example.com" --limit 20  # Crawl a site
servo-fetch map "https://example.com"                    # Discover URLs via sitemap
servo-fetch mcp                                          # MCP server (stdio)
servo-fetch serve                                        # HTTP API server
```

Full CLI reference → [`servo-fetch-cli`](crates/servo-fetch-cli/README.md)

### Rust

```bash
cargo add servo-fetch
```

```rust
// URL → Markdown in one line
let md = servo_fetch::markdown("https://example.com")?;

// Fetch with options
use servo_fetch::{fetch, FetchOptions};
use std::time::Duration;

let page = fetch(FetchOptions::new("https://example.com").timeout(Duration::from_secs(60)))?;
println!("{}", page.html);
let md = page.markdown()?;

// Crawl a site
servo_fetch::crawl_each(
    servo_fetch::CrawlOptions::new("https://docs.example.com")
        .limit(100)
        .user_agent("MyBot/1.0"),
    |result| match &result.outcome {
        Ok(page) => println!("{}: {} chars", result.url, page.content.len()),
        Err(e) => eprintln!("{}: {e}", result.url),
    },
)?;

// Discover URLs via sitemap (no rendering)
let urls = servo_fetch::map(
    servo_fetch::MapOptions::new("https://example.com").limit(1000),
)?;
for u in &urls {
    println!("{}", u.url);
}
```

Full API reference → [`servo-fetch`](crates/servo-fetch/README.md)

### Python

```bash
pip install servo-fetch
```

```python
import servo_fetch

page = servo_fetch.fetch("https://example.com")
print(page.markdown)

# Schema extraction
from servo_fetch import Schema, Field
schema = Schema(
    base_selector=".product",
    fields=[
        Field(name="title", selector="h2", type="text"),
        Field(name="price", selector=".price", type="text"),
    ],
)
page = servo_fetch.fetch("https://shop.example.com", schema=schema)
print(page.extracted)
```

Full API reference → [`bindings/python`](bindings/python/README.md)

## MCP Server

Built-in [Model Context Protocol](https://modelcontextprotocol.io/) server with six tools: `fetch`,
`batch_fetch`, `crawl`, `map`, `screenshot`, and `execute_js`.

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

Full MCP tool reference → [`servo-fetch-cli` README](crates/servo-fetch-cli/README.md)

## HTTP API

REST endpoints for containerized deployments and HTTP clients:

```bash
servo-fetch serve                            # 127.0.0.1:3000
servo-fetch serve --host 0.0.0.0 --port 80   # expose to network

curl -X POST http://127.0.0.1:3000/v1/fetch \
  -H 'content-type: application/json' \
  -d '{"url":"https://example.com"}'
```

Endpoints: `GET /health`, `GET /version`, `POST /v1/fetch`, `POST /v1/batch_fetch`, `POST /v1/screenshot`, `POST /v1/execute_js`, `POST /v1/crawl`, `POST /v1/map`.

Full HTTP API reference → [`servo-fetch-cli` README](crates/servo-fetch-cli/README.md#http-api-server)

## Docker

Multi-arch image on GitHub Container Registry (`linux/amd64`, `linux/arm64`):

```bash
docker run --rm -p 3000:3000 ghcr.io/konippi/servo-fetch:latest
curl -X POST http://127.0.0.1:3000/v1/fetch \
  -H 'content-type: application/json' \
  -d '{"url":"https://example.com"}'
```

Runs as non-root (UID 1001). Images are signed with [cosign](https://github.com/sigstore/cosign) (keyless) and published with SLSA provenance and SBOM attestations.

## Agent Skills

servo-fetch ships with an [Agent Skills](https://agentskills.io/) package for AI coding agents:

```bash
npx skills add https://github.com/konippi/servo-fetch/tree/main/skills/servo-fetch
```

## Security

servo-fetch blocks all private and reserved IP ranges ([RFC 6890](https://datatracker.ietf.org/doc/html/rfc6890)),
strips credentials from URLs, disables HTTP redirects to prevent SSRF bypass, and sanitizes all output against
terminal escape injection ([CVE-2021-42574](https://www.cve.org/CVERecord?id=CVE-2021-42574)).
See [SECURITY.md](./SECURITY.md) for details.

## Limitations

- Sites behind login walls or CAPTCHAs are not supported.

## Contributing

See [CONTRIBUTING.md](./CONTRIBUTING.md) for development setup, commit conventions, and PR guidelines.

## License

[MIT](./LICENSE-MIT) OR [Apache-2.0](./LICENSE-APACHE)
