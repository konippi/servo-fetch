//! Shared helpers for MCP tool implementations.

use std::sync::OnceLock;

use base64::Engine as _;
use rmcp::ErrorData;
use rmcp::model::{CallToolResult, Content};
use tokio::sync::Semaphore;

use crate::bridge;
use crate::net;
use servo_fetch::extract::{self, ExtractInput};

/// Upper bound on the number of Servo fetches processed concurrently.
const DEFAULT_MAX_CONCURRENT_FETCHES: usize = 4;

/// Hard ceiling on concurrency regardless of env override.
const MAX_ALLOWED_CONCURRENCY: usize = 16;

/// Hard ceiling on crawl page count via MCP.
const MAX_MCP_CRAWL_PAGES: usize = 500;

fn fetch_semaphore() -> &'static Semaphore {
    static SEMAPHORE: OnceLock<Semaphore> = OnceLock::new();
    SEMAPHORE.get_or_init(|| {
        let limit = std::env::var("SERVO_FETCH_MAX_CONCURRENCY")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|n| *n > 0)
            .map_or(DEFAULT_MAX_CONCURRENT_FETCHES, |n| n.min(MAX_ALLOWED_CONCURRENCY));
        Semaphore::new(limit)
    })
}

/// Validate a URL and return it in canonical form as a string.
pub(super) fn validated_url(url: &str) -> Result<String, ErrorData> {
    net::validate_url(url)
        .map(|u| u.to_string())
        .map_err(|e| ErrorData::invalid_params(format!("{e:#}"), None))
}

/// Run a Servo fetch on the blocking thread pool, gated by a semaphore so a
/// burst of concurrent tool calls does not create unbounded live `WebView`s.
pub(super) async fn fetch_page(
    url: &str,
    timeout: u64,
    settle_ms: u64,
    mode: bridge::FetchMode,
) -> Result<bridge::ServoPage, ErrorData> {
    let _permit = fetch_semaphore()
        .acquire()
        .await
        .map_err(|e| ErrorData::internal_error(format!("fetch semaphore closed: {e}"), None))?;

    let url = url.to_string();
    tokio::task::spawn_blocking(move || {
        bridge::fetch_page(bridge::FetchOptions {
            url: &url,
            timeout_secs: timeout,
            settle_ms,
            mode,
        })
    })
    .await
    .map_err(|e| ErrorData::internal_error(e.to_string(), None))?
    .map_err(|e| ErrorData::internal_error(format!("{e:#}"), None))
}

pub(super) async fn probe_pdf(url: &str, timeout: u64) -> Option<Vec<u8>> {
    let url = url.to_string();
    tokio::task::spawn_blocking(move || crate::pdf::probe(&url, timeout))
        .await
        .ok()
        .flatten()
}

/// Fetch multiple URLs in parallel, returning `(url, rendered_text)` pairs in completion order.
pub(super) async fn batch_fetch_pages(
    urls: &[String],
    timeout: u64,
    settle_ms: u64,
    json: bool,
    selector: Option<&str>,
    max_len: usize,
) -> Vec<(String, String)> {
    let (tx, mut rx) = tokio::sync::mpsc::channel(urls.len().max(1));

    for url in urls {
        let permit = fetch_semaphore().acquire().await.ok();
        let tx = tx.clone();
        let url = url.clone();
        let selector = selector.map(String::from);
        tokio::task::spawn_blocking(move || {
            let text = fetch_and_render(&url, timeout, settle_ms, json, selector.as_deref(), max_len);
            let _ = tx.blocking_send((url, text));
            drop(permit);
        });
    }
    drop(tx);

    let mut results = Vec::with_capacity(urls.len());
    while let Some(pair) = rx.recv().await {
        results.push(pair);
    }
    results
}

fn fetch_and_render(
    url: &str,
    timeout: u64,
    settle_ms: u64,
    json: bool,
    selector: Option<&str>,
    max_len: usize,
) -> String {
    let page = match bridge::fetch_page(bridge::FetchOptions {
        url,
        timeout_secs: timeout,
        settle_ms,
        mode: bridge::FetchMode::Content { include_a11y: false },
    }) {
        Ok(p) => p,
        Err(e) => return format!("[error] {e:#}"),
    };

    let input = ExtractInput::new(&page.html, url)
        .with_layout_json(page.layout_json.as_deref())
        .with_inner_text(page.inner_text.as_deref())
        .with_selector(selector);

    let full = if json {
        extract::extract_json(&input).unwrap_or_default()
    } else {
        extract::extract_text(&input).unwrap_or_default()
    };

    paginate(&servo_fetch::sanitize::sanitize(&full), 0, max_len)
}

/// Run Readability-based extraction on a fetched page, as Markdown or JSON.
pub(super) fn extract(
    page: &bridge::ServoPage,
    url: &str,
    json: bool,
    selector: Option<&str>,
) -> Result<String, ErrorData> {
    let input = ExtractInput::new(&page.html, url)
        .with_layout_json(page.layout_json.as_deref())
        .with_inner_text(page.inner_text.as_deref())
        .with_selector(selector);
    if json {
        extract::extract_json(&input)
    } else {
        extract::extract_text(&input)
    }
    .map_err(|e| ErrorData::internal_error(e.to_string(), None))
}

/// Render the page and return its screenshot as a base64 PNG MCP content.
pub(super) async fn take_screenshot(
    url: &str,
    timeout: u64,
    settle_ms: u64,
    full_page: bool,
) -> Result<CallToolResult, ErrorData> {
    let page = fetch_page(url, timeout, settle_ms, bridge::FetchMode::Screenshot { full_page }).await?;
    let img = page
        .screenshot
        .ok_or_else(|| ErrorData::internal_error("screenshot capture failed", None))?;

    let mut buf = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

    Ok(CallToolResult::success(vec![Content::image(
        base64::engine::general_purpose::STANDARD.encode(&buf),
        "image/png",
    )]))
}

/// Return a char-aligned slice of `content` and a truncation notice when
/// the slice does not cover the full content.
pub(super) fn paginate(content: &str, start: usize, max_len: usize) -> String {
    use servo_fetch::sanitize::floor_char_boundary;

    let max_len = max_len.max(1);
    let total = content.len();
    let start = floor_char_boundary(content, start);
    if start >= total {
        return format!("<no content at start_index={start}, total_length={total}>");
    }
    let end = floor_char_boundary(content, (start + max_len).min(total));
    let chunk = &content[start..end];
    if end < total {
        format!("{chunk}\n\n<content truncated. total_length={total}, next start_index={end}>")
    } else {
        chunk.to_string()
    }
}

/// Options for [`crawl_pages`].
pub(super) struct CrawlToolOptions<'a> {
    pub url: &'a str,
    pub limit: usize,
    pub max_depth: usize,
    pub json: bool,
    pub selector: Option<&'a str>,
    pub max_len: usize,
    pub timeout: u64,
    pub settle_ms: u64,
    pub include_glob: Option<&'a [String]>,
    pub exclude_glob: Option<&'a [String]>,
}

/// Run a BFS crawl and return per-page text results.
pub(super) async fn crawl_pages(opts: CrawlToolOptions<'_>) -> Result<Vec<(String, String)>, ErrorData> {
    let seed = net::validate_url(opts.url).map_err(|e| ErrorData::invalid_params(format!("{e:#}"), None))?;
    let limit = opts.limit.min(MAX_MCP_CRAWL_PAGES);

    let include = opts
        .include_glob
        .filter(|g| !g.is_empty())
        .map(crate::crawl::build_globset)
        .transpose()
        .map_err(|e| ErrorData::invalid_params(format!("invalid include glob: {e}"), None))?;
    let exclude = opts
        .exclude_glob
        .filter(|g| !g.is_empty())
        .map(crate::crawl::build_globset)
        .transpose()
        .map_err(|e| ErrorData::invalid_params(format!("invalid exclude glob: {e}"), None))?;

    let crawl_opts = crate::crawl::CrawlOptions {
        seed,
        limit,
        max_depth: opts.max_depth,
        timeout_secs: opts.timeout,
        settle_ms: opts.settle_ms,
        include,
        exclude,
        selector: opts.selector.map(String::from),
        json: opts.json,
    };

    let results = crate::crawl::run(crawl_opts, |_| {}).await;

    Ok(results
        .into_iter()
        .map(|r| {
            let text = match r.status {
                crate::crawl::CrawlStatus::Ok => {
                    let content = r.content.unwrap_or_default();
                    paginate(&servo_fetch::sanitize::sanitize(&content), 0, opts.max_len)
                }
                crate::crawl::CrawlStatus::Error => {
                    format!("[error] {}", r.error.unwrap_or_default())
                }
            };
            (r.url, text)
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paginate_full() {
        assert_eq!(paginate("hello", 0, 100), "hello");
    }

    #[test]
    fn paginate_truncates() {
        let r = paginate("hello world", 0, 5);
        assert!(r.starts_with("hello"));
        assert!(r.contains("next start_index=5"));
    }

    #[test]
    fn paginate_offset() {
        assert_eq!(paginate("hello world", 6, 100), "world");
    }

    #[test]
    fn paginate_out_of_bounds() {
        assert!(paginate("hello", 100, 10).contains("no content"));
    }

    #[test]
    fn paginate_multibyte_boundary() {
        // paginate must produce valid UTF-8 at the byte boundary.
        let result = paginate("日本語", 0, 4);
        assert!(result.starts_with("日"));
    }

    #[test]
    fn rejects_private_url() {
        assert!(validated_url("http://127.0.0.1/").is_err());
    }

    #[test]
    fn accepts_public_url() {
        assert!(validated_url("https://example.com").is_ok());
    }

    #[test]
    fn paginate_max_len_zero_clamped() {
        let r = paginate("hello", 0, 0);
        assert!(r.starts_with('h'), "max_len=0 should clamp to 1");
    }

    #[test]
    fn paginate_start_mid_multibyte() {
        // start=1 is inside the first 3-byte char "日"; should snap to 0
        let r = paginate("日本語", 1, 100);
        assert!(r.starts_with("日"), "should snap to char boundary");
    }

    #[test]
    fn rejects_file_scheme() {
        assert!(validated_url("file:///etc/passwd").is_err());
    }
}
