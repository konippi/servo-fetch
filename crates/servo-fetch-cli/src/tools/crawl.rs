//! Crawl tool helper.

use std::time::Duration;

use super::common::paginate;
use super::error::{ToolError, ToolResult};

pub(crate) const MAX_CRAWL_PAGES: usize = 500;

pub(crate) struct CrawlOptions<'a> {
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

pub(crate) async fn crawl_pages(opts: CrawlOptions<'_>) -> ToolResult<Vec<(String, String)>> {
    let limit = opts.limit.min(MAX_CRAWL_PAGES);

    let mut builder = servo_fetch::CrawlOptions::new(opts.url)
        .limit(limit)
        .max_depth(opts.max_depth)
        .timeout(Duration::from_secs(opts.timeout))
        .settle(Duration::from_millis(opts.settle_ms))
        .json(opts.json);
    if let Some(selector) = opts.selector {
        builder = builder.selector(selector);
    }

    if let Some(globs) = opts.include_glob.filter(|g| !g.is_empty()) {
        let refs: Vec<&str> = globs.iter().map(String::as_str).collect();
        builder = builder.include(&refs);
    }
    if let Some(globs) = opts.exclude_glob.filter(|g| !g.is_empty()) {
        let refs: Vec<&str> = globs.iter().map(String::as_str).collect();
        builder = builder.exclude(&refs);
    }

    let max_len = opts.max_len;
    tokio::task::spawn_blocking(move || {
        let mut results = Vec::new();
        servo_fetch::crawl_each(builder, |r| {
            let text = match &r.outcome {
                Ok(page) => paginate(&servo_fetch::sanitize::sanitize(&page.content), 0, max_len),
                Err(e) => format!("[error] {e}"),
            };
            results.push((r.url.clone(), text));
        })
        .map_err(|e| ToolError::fetch(format!("{e:#}")))?;
        Ok(results)
    })
    .await
    .map_err(|e| ToolError::internal(format!("spawn_blocking failed: {e}")))?
}
