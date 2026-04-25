<div align="center">
  <h1 align="center">servo-fetch</h1>
  <p align="center">Web rendering for your terminal and AI agents — no Chromium, no browser download.</p>
  <p>
    <a href="https://github.com/konippi/servo-fetch/actions"><img src="https://github.com/konippi/servo-fetch/workflows/CI/badge.svg" alt="CI"></a>
    <a href="https://crates.io/crates/servo-fetch"><img src="https://img.shields.io/crates/v/servo-fetch.svg" alt="crates.io"></a>
    <img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="MIT">
  </p>
</div>

**servo-fetch** is a single-binary CLI and MCP server that renders web pages using the [Servo](https://servo.org/) browser engine. It executes JavaScript, computes CSS layout, captures screenshots, and extracts clean content — all without downloading a browser.

```bash
servo-fetch "https://example.com"                    # Clean Markdown
servo-fetch "https://example.com" --screenshot page.png  # PNG screenshot, no GPU needed
servo-fetch "https://example.com" --js "document.title"  # Run JS in the page
```

## Why servo-fetch?

- **Screenshots without a browser runtime.** Servo renders pages to PNG with a software renderer. No GPU, no Xvfb, no Chromium download. Drop the binary into Docker or CI and it just works.

- **Reads JavaScript-heavy pages.** SPAs, React, Vue — servo-fetch executes JS via SpiderMonkey and extracts the rendered content. Plain HTTP fetchers return empty HTML; servo-fetch returns the real page.

- **Strips navigation noise using CSS layout.** Most tools guess page structure from HTML tags. servo-fetch uses `getComputedStyle()` and `getBoundingClientRect()` to detect fixed navbars, sidebars, and footers — then removes them before extraction.

- **Single binary, zero runtime dependencies.** `cargo install` or download a prebuilt binary. No Node.js, no `npx playwright install`, no `apt-get install chromium`.

- **Built-in MCP server for AI agents.** Three tools (`fetch`, `screenshot`, `execute_js`) over stdio or Streamable HTTP. AI agents can read SPAs and take screenshots without any browser setup.

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/konippi/servo-fetch/main/install.sh | sh
```

Or via [GitHub Releases](https://github.com/konippi/servo-fetch/releases), [cargo-binstall](https://github.com/cargo-bins/cargo-binstall), or build from source:

```bash
cargo binstall servo-fetch   # prebuilt binary
cargo install servo-fetch    # build from source (see CONTRIBUTING.md)
```

```bash
# Readable Markdown (default)
servo-fetch "https://example.com"

# Structured JSON
servo-fetch "https://example.com" --json

# Screenshot — rendered to PNG without GPU
servo-fetch "https://example.com" --screenshot page.png

# Execute JavaScript in the page context
servo-fetch "https://example.com" --js "document.title"
servo-fetch "https://example.com" --js "[...document.querySelectorAll('h2')].map(e => e.textContent)"

# Extract a specific section by CSS selector
servo-fetch "https://example.com" --selector "article"

# Pipe to other tools
servo-fetch "https://docs.rs/tokio" | grep "async"
servo-fetch "https://example.com" --json | jq .title
```

## Options

| Flag | Description |
| ---- | ----------- |
| `--json` | Output as structured JSON |
| `--screenshot <FILE>` | Save a PNG screenshot |
| `--js <EXPR>` | Execute JavaScript and print the result |
| `--selector <CSS>` | Extract a specific section by CSS selector |
| `-t`, `--timeout <SECS>` | Page load timeout (default: 30) |
| `--help` | Show help |
| `--version` | Show version |

## JSON output

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

## How it works

1. Servo loads the page and executes JavaScript via SpiderMonkey
2. CSS is computed with Servo's parallel layout engine — `getComputedStyle()` and `getBoundingClientRect()` identify page structure
3. Navbars, sidebars, and footers are stripped using CSS layout data
4. Mozilla's [Readability](https://github.com/mozilla/readability) algorithm extracts the main content
5. Content is output as Markdown, JSON, or PNG

## MCP server

servo-fetch includes a built-in [MCP](https://modelcontextprotocol.io/) server for AI agents with three tools: `fetch`, `screenshot`, and `execute_js`.

```bash
# stdio transport (default)
servo-fetch mcp

# Streamable HTTP transport
servo-fetch mcp --port 8080
```

Add to your MCP client config (Claude Code, Codex, Cursor, etc.):

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

## Security

servo-fetch blocks all private and reserved IP ranges ([RFC 6890](https://datatracker.ietf.org/doc/html/rfc6890)), strips credentials from URLs, validates redirect targets, and sanitizes all output against terminal escape injection ([CVE-2021-42574](https://cve.mitre.org/cgi-bin/cvename.cgi?name=CVE-2021-42574)). See [SECURITY.md](./SECURITY.md) for details.

## Limitations

- Best suited for documentation, blogs, and SSR sites
- Some SPAs with complex client-side rendering may not fully render
- Servo's web compatibility is [improving monthly](https://servo.org/blog/)

## License

[MIT](./LICENSE)
