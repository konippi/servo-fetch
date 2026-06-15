//! Map tool helper.

use std::collections::BTreeMap;

use servo_fetch::{MapOptions, MappedUrl};

use super::error::{ToolError, ToolResult};
use super::limits::{MAP_LIMIT, clamp_count};
use super::options::{build_headers, glob_refs, resolve_timeout};

/// Run a built map on the engine, returning sitemap entries (URL plus `lastmod`).
pub(crate) async fn map_with(opts: MapOptions) -> ToolResult<Vec<MappedUrl>> {
    tokio::task::spawn_blocking(move || servo_fetch::blocking::map(&opts))
        .await
        .map_err(|e| ToolError::internal(e.to_string()))?
        .map_err(ToolError::from)
}

pub(crate) struct MapSpec<'a> {
    pub url: &'a str,
    pub limit: Option<u64>,
    pub include: Option<&'a [String]>,
    pub exclude: Option<&'a [String]>,
    pub no_fallback: bool,
    pub user_agent: Option<&'a str>,
    pub timeout: Option<u64>,
    pub headers: Option<BTreeMap<String, String>>,
}

/// Build `MapOptions`; map does not render, so it honors only UA/timeout/headers.
pub(crate) fn build_map_options(spec: MapSpec<'_>) -> ToolResult<MapOptions> {
    let mut opts = MapOptions::new(spec.url)
        .limit(clamp_count(spec.limit, MAP_LIMIT))
        .timeout(resolve_timeout(spec.timeout).as_secs());
    if let Some(globs) = spec.include.filter(|g| !g.is_empty()) {
        opts = opts.include(&glob_refs(globs));
    }
    if let Some(globs) = spec.exclude.filter(|g| !g.is_empty()) {
        opts = opts.exclude(&glob_refs(globs));
    }
    if let Some(ua) = spec.user_agent {
        opts = opts.user_agent(ua);
    }
    if spec.no_fallback {
        opts = opts.no_fallback(true);
    }
    Ok(opts.headers(build_headers(spec.headers)?))
}
