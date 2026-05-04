# servo-fetch

[![crates.io](https://img.shields.io/crates/v/servo-fetch.svg)](https://crates.io/crates/servo-fetch)
[![docs.rs](https://docs.rs/servo-fetch/badge.svg)](https://docs.rs/servo-fetch)

Fetch, render, and extract web content with an embedded [Servo](https://servo.org/) browser engine.
No Chrome, no containers, no external processes.

Looking for the CLI? See [`servo-fetch-cli`](https://crates.io/crates/servo-fetch-cli).

## Features

- **Real JS execution** — SpiderMonkey runs JavaScript, parallel CSS engine computes layout
- **Layout-aware extraction** — strips navbars, sidebars, footers by rendered position
- **Sync API** — no async runtime required; wrap with `spawn_blocking` for async contexts
- **PDF auto-detection** — URLs returning PDF are automatically extracted as text
- **Typed errors** — `Error::Timeout`, `Error::InvalidUrl`, etc. for match-based retry logic
- **SSRF protection** — blocks private IPs, reserved ranges, and metadata endpoints

## Quick Start

```rust
let md = servo_fetch::markdown("https://example.com")?;
```

## Examples

### Fetch with options

```rust
use servo_fetch::{fetch, FetchOptions};
use std::time::Duration;

let page = fetch(
    FetchOptions::new("https://spa-site.com")
        .timeout(Duration::from_secs(60))
        .settle(Duration::from_millis(3000))
        .user_agent("MyBot/1.0"),
)?;
println!("{}", page.html);
let md = page.markdown()?;
```

### Screenshot

```rust
use servo_fetch::{fetch, FetchOptions};

let page = fetch(FetchOptions::screenshot("https://example.com", true))?;
std::fs::write("page.png", page.screenshot_png().unwrap())?;
```

### JavaScript execution

```rust
use servo_fetch::{fetch, FetchOptions};

let page = fetch(FetchOptions::javascript("https://example.com", "document.title"))?;
println!("{}", page.js_result.unwrap());
```

### Crawl a site

```rust
use servo_fetch::{crawl_each, CrawlOptions};

crawl_each(
    CrawlOptions::new("https://docs.example.com")
        .limit(100)
        .include(&["/docs/**"]),
    |result| match &result.outcome {
        Ok(page) => println!("{}: {} chars", result.url, page.content.len()),
        Err(e) => eprintln!("{}: {e}", result.url),
    },
)?;
```

### Error handling

```rust
use servo_fetch::{fetch, FetchOptions, Error};

match fetch(FetchOptions::new(url)) {
    Ok(page) => { /* ... */ }
    Err(e) if e.is_timeout() => { /* retry */ }
    Err(Error::AddressNotAllowed(_)) => { /* skip */ }
    Err(e) => return Err(e.into()),
}
```

### From async contexts

```rust
let page = tokio::task::spawn_blocking(|| {
    servo_fetch::fetch(servo_fetch::FetchOptions::new("https://example.com"))
}).await??;
```

## Environment Variables

| Variable | Description |
| -------- | ----------- |
| `SERVO_FETCH_USER_AGENT` | Default User-Agent string (overridden by `.user_agent()`) |

## API Overview

| Function | Description |
| -------- | ----------- |
| `markdown(url)` | Fetch → readable Markdown |
| `extract_json(url)` | Fetch → structured JSON |
| `text(url)` | Fetch → plain text (`innerText`) |
| `fetch(opts)` | Fetch with full options → `Page` |
| `crawl(opts)` | Crawl site → `Vec<CrawlResult>` |
| `crawl_each(opts, cb)` | Crawl site, streaming results |

See [docs.rs](https://docs.rs/servo-fetch) for the full API reference and [`examples/`](examples/) for complete runnable programs.

## License

[MIT](https://github.com/konippi/servo-fetch/blob/main/LICENSE-MIT) OR [Apache-2.0](https://github.com/konippi/servo-fetch/blob/main/LICENSE-APACHE)
