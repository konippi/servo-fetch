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

/// Default response length when a caller does not specify `maxLength`.
pub(crate) const DEFAULT_MAX_LENGTH: usize = 5000;

/// A count input's default (when absent) and inclusive maximum.
#[derive(Clone, Copy)]
pub(crate) struct CountBound {
    pub default: usize,
    pub max: usize,
}

pub(crate) const CRAWL_LIMIT: CountBound = CountBound {
    default: 50,
    max: MAX_CRAWL_PAGES,
};
pub(crate) const CRAWL_DEPTH: CountBound = CountBound {
    default: 3,
    max: MAX_CRAWL_DEPTH,
};
pub(crate) const CRAWL_CONCURRENCY: CountBound = CountBound {
    default: 1,
    max: MAX_CRAWL_CONCURRENCY,
};
pub(crate) const MAP_LIMIT: CountBound = CountBound {
    default: 5000,
    max: MAX_MAP_URLS,
};

/// Clamp an optional `u64` count into `1..=bound.max`, falling back to `bound.default`.
pub(crate) fn clamp_count(value: Option<u64>, bound: CountBound) -> usize {
    value.map_or(bound.default, |n| {
        usize::try_from(n).unwrap_or(bound.max).clamp(1, bound.max)
    })
}

pub(crate) fn to_len(value: Option<u64>, default: usize) -> usize {
    value.map_or(default, |n| usize::try_from(n).unwrap_or(usize::MAX))
}
