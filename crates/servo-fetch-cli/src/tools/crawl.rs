//! Crawl tool helper.

use std::time::Duration;

use servo_fetch_types::{FetchFormat, RequestOptions};

use super::error::{ToolError, ToolResult};
use super::limits::{CRAWL_CONCURRENCY, CRAWL_DEPTH, CRAWL_LIMIT, clamp_count};
use super::options::{build_headers, glob_refs, load_cookies, resolve_settle, resolve_timeout};
use super::render::paginate;

pub(crate) struct CrawlSpec<'a> {
    pub url: &'a str,
    pub limit: Option<u64>,
    pub max_depth: Option<u64>,
    pub format: FetchFormat,
    pub selector: Option<&'a str>,
    pub include: Option<&'a [String]>,
    pub exclude: Option<&'a [String]>,
    pub concurrency: Option<u64>,
    pub delay_ms: Option<u64>,
    pub options: RequestOptions,
}

/// Build the engine crawl options shared by the streaming and collecting paths.
pub(crate) fn build_crawl_options(spec: &CrawlSpec<'_>) -> ToolResult<servo_fetch::CrawlOptions> {
    let mut builder = servo_fetch::CrawlOptions::new(spec.url)
        .limit(clamp_count(spec.limit, CRAWL_LIMIT))
        .max_depth(clamp_count(spec.max_depth, CRAWL_DEPTH))
        .timeout(resolve_timeout(spec.options.timeout))
        .settle(resolve_settle(spec.options.settle_ms))
        .concurrency(clamp_count(spec.concurrency, CRAWL_CONCURRENCY))
        .delay(resolve_delay(spec.delay_ms))
        .json(matches!(spec.format, FetchFormat::Json));
    if let Some(selector) = spec.selector {
        builder = builder.selector(selector);
    }
    if let Some(globs) = spec.include.filter(|g| !g.is_empty()) {
        builder = builder.include(&glob_refs(globs));
    }
    if let Some(globs) = spec.exclude.filter(|g| !g.is_empty()) {
        builder = builder.exclude(&glob_refs(globs));
    }
    if let Some(ua) = spec.options.user_agent.as_deref() {
        builder = builder.user_agent(ua);
    }
    if let Some(path) = spec.options.cookies_file.as_deref() {
        builder = builder.cookies(load_cookies(path)?);
    }
    Ok(builder.headers(build_headers(spec.options.headers.clone())?))
}

/// Resolve the dispatch interval: `Some(0)` disables it, `None` uses the 500ms default.
fn resolve_delay(delay_ms: Option<u64>) -> Option<Duration> {
    match delay_ms {
        Some(0) => None,
        Some(ms) => Some(Duration::from_millis(ms)),
        None => Some(Duration::from_millis(500)),
    }
}

pub(crate) async fn crawl_pages(spec: CrawlSpec<'_>, max_len: usize) -> ToolResult<Vec<(String, String)>> {
    let builder = build_crawl_options(&spec)?;
    tokio::task::spawn_blocking(move || {
        let mut results = Vec::new();
        servo_fetch::blocking::crawl_each(&builder, |r| {
            let text = match &r.outcome {
                Ok(page) => paginate(&servo_fetch::sanitize::sanitize(&page.content), 0, max_len),
                Err(e) => format!("[error] {e}"),
            };
            results.push((r.url, text));
        })
        .map_err(ToolError::from)?;
        Ok(results)
    })
    .await
    .map_err(|e| ToolError::internal(format!("spawn_blocking failed: {e}")))?
}
