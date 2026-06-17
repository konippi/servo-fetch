//! Fetch and batch-fetch tool helpers.

use std::sync::OnceLock;

use servo_fetch::{FetchOptions, Page, VisibilityPolicy};
use servo_fetch_types::{FetchFormat, RequestOptions};
use tokio::sync::Semaphore;
use tokio::task::{JoinSet, spawn_blocking};

use super::error::{ToolError, ToolResult};
use super::options::{apply_options, content_options};
use super::render::{paginate, render_page};

const DEFAULT_MAX_CONCURRENT_FETCHES: usize = 4;
const MAX_ALLOWED_CONCURRENCY: usize = 16;

/// Process-wide gate bounding concurrent engine fetches (`SERVO_FETCH_MAX_CONCURRENCY`).
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

/// Run a built fetch on the warm engine, bounded by the global fetch semaphore.
pub(crate) async fn fetch_with(opts: FetchOptions) -> ToolResult<Page> {
    let _permit = fetch_semaphore()
        .acquire()
        .await
        .map_err(|e| ToolError::internal(format!("fetch semaphore closed: {e}")))?;
    spawn_blocking(move || servo_fetch::blocking::fetch(&opts))
        .await
        .map_err(|e| ToolError::internal(e.to_string()))?
        .map_err(ToolError::from)
}

pub(crate) struct BatchSpec<'a> {
    pub urls: &'a [String],
    pub format: FetchFormat,
    pub selector: Option<&'a str>,
    pub max_len: usize,
    pub visibility: VisibilityPolicy,
    pub options: RequestOptions,
}

pub(crate) async fn batch_fetch_pages(spec: BatchSpec<'_>) -> Vec<(String, String)> {
    let mut set = JoinSet::new();

    for url in spec.urls {
        let permit = fetch_semaphore().acquire().await.ok();
        let url = url.clone();
        let selector = spec.selector.map(String::from);
        let format = spec.format;
        let max_len = spec.max_len;
        let visibility = spec.visibility;
        let options = spec.options.clone();
        set.spawn_blocking(move || {
            let _permit = permit;
            let text = render_one(&url, format, selector.as_deref(), max_len, visibility, options);
            (url, text)
        });
    }

    let mut results = Vec::with_capacity(spec.urls.len());
    while let Some(joined) = set.join_next().await {
        if let Ok(pair) = joined {
            results.push(pair);
        }
    }
    results
}

fn render_one(
    url: &str,
    format: FetchFormat,
    selector: Option<&str>,
    max_len: usize,
    visibility: VisibilityPolicy,
    options: RequestOptions,
) -> String {
    let opts = match apply_options(content_options(url, format, visibility), options) {
        Ok(opts) => opts,
        Err(e) => return format!("[error] {e}"),
    };
    match servo_fetch::blocking::fetch(&opts) {
        Ok(page) => match render_page(&page, url, format, selector) {
            Ok(full) => paginate(&servo_fetch::sanitize::sanitize(&full), 0, max_len),
            Err(e) => format!("[error] {e}"),
        },
        Err(e) => format!("[error] {e}"),
    }
}
