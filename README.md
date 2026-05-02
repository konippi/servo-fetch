<div align="center">
  <h1 align="center">servo-fetch</h1>
  <p align="center">A self-contained browser engine that fetches, renders, and extracts web content. No Chrome, no API key, no setup.</p>
  <p>
    <a href="https://github.com/konippi/servo-fetch/actions"><img src="https://github.com/konippi/servo-fetch/workflows/CI/badge.svg" alt="CI"></a>
    <a href="https://crates.io/crates/servo-fetch"><img src="https://img.shields.io/crates/v/servo-fetch.svg" alt="crates.io"></a>
    <img src="https://img.shields.io/badge/Rust-1.86.0-blue?color=fc8d62&logo=rust" alt="MSRV">
    <img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="MIT">
  </p>
</div>

servo-fetch embeds the [Servo](https://servo.org/) browser engine. It executes JavaScript, computes CSS layout, captures screenshots with a software renderer, and extracts clean content — available as both a CLI tool and a Rust library.

```bash
servo-fetch "https://example.com"                        # CLI: clean Markdown
servo-fetch "https://example.com" --screenshot page.png  # CLI: PNG screenshot
```

```rust
let md = servo_fetch::markdown("https://example.com")?;  // Library: one-liner
```

## Why servo-fetch

- **Zero dependencies** — single binary, no Chrome, no Docker, no API key
- **Real JS execution** — SpiderMonkey runs JavaScript, parallel CSS engine computes layout
- **Layout-aware extraction** — strips navbars, sidebars, footers by actual rendered position
- **Parallel batch fetch** — multiple URLs fetched concurrently
- **Site crawling** — BFS link traversal with robots.txt, same-site scope, and rate limiting
- **Screenshots without GPU** — software renderer captures PNG/full-page screenshots anywhere
- **Accessibility tree** — AccessKit integration with roles, names, and bounding boxes

## Performance

Parallel fetch — 4 URLs, JS executed, full CSS rendering:

| Tool | Peak Memory | Time |
| ---- | ----------- | ---- |
| **servo-fetch** | **114 MB** | **1.5s** |
| Playwright | 502 MB | 3.3s |
| Puppeteer | 1065 MB | 4.3s |

Same rendering capabilities, 4–9× less memory, 2–3× faster. [Methodology →](benchmarks/)

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
servo-fetch URL1 URL2 URL3                               # Parallel batch
servo-fetch crawl "https://docs.example.com" --limit 20  # Crawl a site
servo-fetch mcp                                          # MCP server (stdio)
```

Full CLI reference → [`servo-fetch-cli`](crates/servo-fetch-cli/README.md)

### Library

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
    servo_fetch::CrawlOptions::new("https://docs.example.com").limit(100),
    |result| println!("{}: {:?}", result.url, result.status),
)?;
```

Full API reference → [`servo-fetch`](crates/servo-fetch/README.md)

## MCP Server

Built-in [Model Context Protocol](https://modelcontextprotocol.io/) server with five tools: `fetch`, `batch_fetch`, `crawl`, `screenshot`, and `execute_js`.

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

## Agent Skills

servo-fetch ships with an [Agent Skills](https://agentskills.io/) package for AI coding agents:

```bash
npx skills add https://github.com/konippi/servo-fetch/tree/main/skills/servo-fetch
```

## Security

servo-fetch blocks all private and reserved IP ranges ([RFC 6890](https://datatracker.ietf.org/doc/html/rfc6890)), strips credentials from URLs, disables HTTP redirects to prevent SSRF bypass, and sanitizes all output against terminal escape injection ([CVE-2021-42574](https://www.cve.org/CVERecord?id=CVE-2021-42574)). See [SECURITY.md](./SECURITY.md) for details.

## Limitations

- Servo's web compatibility is [improving monthly](https://servo.org/blog/) but does not yet match Chromium. Some SPAs with complex client-side rendering may not fully render.
- Best results on documentation, blogs, news sites, and server-rendered pages.
- Sites behind login walls or CAPTCHAs are not supported.

## Contributing

See [CONTRIBUTING.md](./CONTRIBUTING.md) for development setup, commit conventions, and PR guidelines.

## License

[MIT](./LICENSE)
