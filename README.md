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

servo-fetch embeds the [Servo](https://servo.org/) browser engine into a single binary. It executes JavaScript, computes CSS layout, captures screenshots with a software renderer, and extracts clean content.

```bash
servo-fetch "https://example.com"                        # Clean Markdown
servo-fetch "https://example.com" --screenshot page.png  # PNG screenshot, no GPU needed
servo-fetch "https://example.com" --js "document.title"  # Run JS in the page
servo-fetch URL1 URL2 URL3                               # Parallel batch fetch
```

## Why servo-fetch

- **Zero dependencies** — single binary, no Chrome, no Docker, no API key
- **Real JS execution** — SpiderMonkey runs JavaScript, parallel CSS engine computes layout
- **Layout-aware extraction** — strips navbars, sidebars, footers by actual rendered position, not HTML guessing
- **Parallel batch fetch** — multiple URLs fetched concurrently, results stream as each completes
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
cargo binstall servo-fetch   # prebuilt binary
cargo install servo-fetch    # build from source
```

### Platform notes

<details>

<summary><b>Linux</b> — runtime dependencies and headless setup</summary>

The Linux binary dynamically links against system libraries. Install them with:

```bash
# Debian/Ubuntu
sudo apt install -y libegl1 libfontconfig1 libfreetype6

# Fedora
sudo dnf install -y mesa-libEGL fontconfig freetype

# Arch
sudo pacman -S --needed mesa fontconfig freetype2
```

servo-fetch needs a working OpenGL ES context, so on headless servers (SSH/container) run it under a virtual display:

```bash
xvfb-run --auto-servernum servo-fetch "https://example.com"
```

</details>

<details><summary><b>Windows</b> — zip layout</summary>

Windows releases ship as a `.zip` containing `servo-fetch.exe` alongside `libEGL.dll` and `libGLESv2.dll` — keep them in the same directory. Download from [Releases](https://github.com/konippi/servo-fetch/releases), extract, and put the folder on your `PATH`.

</details>

<details><summary><b>macOS</b> — no extra setup</summary>

No runtime dependencies. The release binary is ready to run.

</details>

## Usage

### Examples

```bash
# Readable Markdown (default)
servo-fetch "https://example.com"

# Structured JSON
servo-fetch "https://example.com" --json

# Multiple URLs in parallel (Markdown with separators)
servo-fetch "https://a.com" "https://b.com" "https://c.com"

# Multiple URLs as NDJSON (one compact JSON per line)
servo-fetch "https://a.com" "https://b.com" --json

# Screenshot — rendered to PNG without GPU
servo-fetch "https://example.com" --screenshot page.png

# Full-page screenshot (captures the entire scrollable page)
servo-fetch "https://example.com" --screenshot page.png --full-page

# Execute JavaScript in the page context
servo-fetch "https://example.com" --js "document.title"

# Extract a specific section by CSS selector
servo-fetch "https://example.com" --selector "article"

# Raw HTML or plain text (bypasses Readability)
servo-fetch "https://example.com" --raw html
servo-fetch "https://example.com" --raw text

# PDF text extraction (auto-detected via Content-Type)
servo-fetch "https://example.com/report.pdf"
```

### Options

| Flag | Description |
| ---- | ----------- |
| `--json` | Output as structured JSON (NDJSON when multiple URLs) |
| `--screenshot <FILE>` | Save a PNG screenshot (single URL only) |
| `--full-page` | Capture the full scrollable page (requires `--screenshot`) |
| `--js <EXPR>` | Execute JavaScript and print the result (single URL only) |
| `--selector <CSS>` | Extract a specific section by CSS selector |
| `--raw <MODE>` | Output raw `html` or plain `text` (single URL only) |
| `-t`, `--timeout <SECS>` | Page load timeout (default: 30) |
| `--settle <MS>` | Extra wait after load event for SPAs (default: 0, max: 10000) |
| `--help` | Show help |
| `--version` | Show version |

When multiple URLs are given, they are fetched in parallel. Results stream to stdout in completion order — Markdown with `--- URL ---` separators by default, or NDJSON with `--json`.

### JSON output

`--json` returns an object with these fields:

| Field | Type | Description |
| ----- | ---- | ----------- |
| `title` | string | Page title |
| `content` | string | Raw HTML extracted by Readability |
| `text_content` | string | Readable text (Markdown) |
| `byline` | string? | Author or byline |
| `excerpt` | string? | Short excerpt or description |
| `lang` | string? | Document language (e.g. `"en"`) |
| `url` | string? | Canonical URL |

Fields marked `?` are omitted when not detected.

## MCP server

servo-fetch includes a built-in MCP server with four tools — `fetch`, `batch_fetch`, `screenshot`, and `execute_js` — over stdio or Streamable HTTP.

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

For Streamable HTTP transport:

```bash
servo-fetch mcp --port 8080
```

### Tools

<details>

<summary><b>fetch</b> — extract readable content from a URL</summary>

| Parameter | Type | Description |
| --------- | ---- | ----------- |
| `url` | string | URL to fetch (http/https only) |
| `format` | string? | `markdown` (default), `json`, `html`, `text`, or `accessibility_tree` |
| `max_length` | number? | Max characters to return (default 5000) |
| `start_index` | number? | Character offset for pagination (default 0) |
| `timeout` | number? | Page load timeout in seconds (default 30) |
| `settle_ms` | number? | Extra wait in ms after load event for SPAs (default 0, max 10000) |
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

## Agent Skills

servo-fetch ships with an [Agent Skills](https://agentskills.io/) package for AI coding agents. Install with [`npx skills`](https://github.com/vercel-labs/skills):

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
