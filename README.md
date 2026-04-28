<div align="center">
  <h1 align="center">servo-fetch</h1>
  <p align="center">A browser engine in a binary — fetch, render, and extract web content without installing a browser.</p>
  <p>
    <a href="https://github.com/konippi/servo-fetch/actions"><img src="https://github.com/konippi/servo-fetch/workflows/CI/badge.svg" alt="CI"></a>
    <a href="https://crates.io/crates/servo-fetch"><img src="https://img.shields.io/crates/v/servo-fetch.svg" alt="crates.io"></a>
    <img src="https://img.shields.io/badge/Rust-1.86.0-blue?color=fc8d62&logo=rust" alt="MSRV">
    <img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="MIT">
  </p>
</div>

servo-fetch embeds the [Servo](https://servo.org/) browser engine into a single, lightweight binary. It executes JavaScript via SpiderMonkey, computes CSS layout with Servo's parallel engine, captures screenshots with a software renderer, and extracts clean content — as a CLI tool or an [MCP](https://modelcontextprotocol.io/) server for AI agents.

```bash
servo-fetch "https://example.com"                        # Clean Markdown
servo-fetch "https://example.com" --screenshot page.png  # PNG screenshot, no GPU needed
servo-fetch "https://example.com" --js "document.title"  # Run JS in the page
```

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

<details><summary><b>Linux</b> — runtime dependencies and headless setup</summary>

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

# Screenshot — rendered to PNG without GPU
servo-fetch "https://example.com" --screenshot page.png

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
| `--json` | Output as structured JSON |
| `--screenshot <FILE>` | Save a PNG screenshot |
| `--js <EXPR>` | Execute JavaScript and print the result |
| `--selector <CSS>` | Extract a specific section by CSS selector |
| `--raw <MODE>` | Output raw `html` or plain `text` (bypasses Readability) |
| `-t`, `--timeout <SECS>` | Page load timeout (default: 30) |
| `--help` | Show help |
| `--version` | Show version |

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

## Why servo-fetch

**Servo is a real browser engine.** Written in Rust by the [Servo project](https://servo.org/), Servo executes JavaScript via SpiderMonkey and computes CSS layout with a parallel engine. servo-fetch embeds this engine so you get browser-grade rendering without a browser runtime.

**CSS layout strips navigation noise.** Most extraction tools guess page structure from HTML tags. servo-fetch calls `getComputedStyle()` and `getBoundingClientRect()` inside the engine to detect fixed navbars, sidebars, and footers — then removes them before extraction. Common cookie banners and newsletter popups are also stripped via injected user stylesheets.

**Accessibility tree with bounding boxes.** servo-fetch can return the page's accessibility tree via Servo's AccessKit integration. Each node includes its role, name, and bounding box — combining semantic structure with visual layout in a single output. Use `format: "accessibility_tree"` in the MCP fetch tool.

**Main content via Readability.** After CSS-based structure removal, Mozilla's [Readability](https://github.com/mozilla/readability) algorithm extracts the main article. PDF URLs are auto-detected via `Content-Type` and extracted directly without the Servo engine.

## MCP server

servo-fetch includes a built-in MCP server with three tools — `fetch`, `screenshot`, and `execute_js` — over stdio or Streamable HTTP.

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

The `fetch` tool accepts these parameters:

| Parameter | Type | Description |
| --------- | ---- | ----------- |
| `url` | string | URL to fetch (http/https only) |
| `format` | string? | `markdown` (default), `json`, `html`, `text`, or `accessibility_tree` |
| `max_length` | number? | Max characters to return (default 5000) |
| `start_index` | number? | Character offset for pagination (default 0) |
| `timeout` | number? | Page load timeout in seconds (default 30) |
| `selector` | string? | CSS selector to extract a specific section |

## Agent Skills

servo-fetch ships with an [Agent Skills](https://agentskills.io/) package for AI coding agents. Install with [`npx skills`](https://github.com/vercel-labs/skills):

```bash
npx skills add https://github.com/konippi/servo-fetch/tree/main/skills/servo-fetch
```

PDF URLs are auto-detected via `Content-Type` and extracted directly without Servo.

## Security

servo-fetch blocks all private and reserved IP ranges ([RFC 6890](https://datatracker.ietf.org/doc/html/rfc6890)), strips credentials from URLs, validates redirect targets, and sanitizes all output against terminal escape injection ([CVE-2021-42574](https://cve.mitre.org/cgi-bin/cvename.cgi?name=CVE-2021-42574)). See [SECURITY.md](./SECURITY.md) for details.

## Limitations

- Best suited for documentation, blogs, and SSR sites
- Some SPAs with complex client-side rendering may not fully render
- Servo's web compatibility is [improving monthly](https://servo.org/blog/)

## Contributing

See [CONTRIBUTING.md](./CONTRIBUTING.md) for development setup, commit conventions, and PR guidelines.

## License

[MIT](./LICENSE)
