//! Shared request limits.

pub(crate) const MAX_TIMEOUT_SECS: u64 = 300;
pub(crate) const MAX_SETTLE_MS: u64 = 10_000;
pub(crate) const MAX_SELECTOR_LEN: usize = 1_000;
pub(crate) const MAX_JS_LEN: usize = 10_000;
pub(crate) const MAX_JS_OUTPUT_LEN: usize = 1_000_000;
pub(crate) const MAX_BATCH_URLS: usize = 20;
pub(crate) const MAX_CRAWL_PAGES: usize = 500;
pub(crate) const MAX_CRAWL_DEPTH: usize = 10;
pub(crate) const MAX_CRAWL_CONCURRENCY: usize = 16;
pub(crate) const MAX_MAP_URLS: usize = 100_000;
