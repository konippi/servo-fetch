//! Map tool helper.

use servo_fetch::{MapOptions, MappedUrl};

use super::error::{ToolError, ToolResult};

/// Run a built map on the engine, returning sitemap entries (URL plus `lastmod`).
pub(crate) async fn map_with(opts: MapOptions) -> ToolResult<Vec<MappedUrl>> {
    tokio::task::spawn_blocking(move || servo_fetch::blocking::map(&opts))
        .await
        .map_err(|e| ToolError::internal(e.to_string()))?
        .map_err(ToolError::from)
}

pub(crate) async fn discover_urls(
    url: &str,
    limit: usize,
    include_glob: &[String],
    exclude_glob: &[String],
) -> ToolResult<Vec<String>> {
    let mut opts = MapOptions::new(url).limit(limit);
    if !include_glob.is_empty() {
        opts = opts.include(&include_glob.iter().map(String::as_str).collect::<Vec<_>>());
    }
    if !exclude_glob.is_empty() {
        opts = opts.exclude(&exclude_glob.iter().map(String::as_str).collect::<Vec<_>>());
    }
    let entries = map_with(opts).await?;
    Ok(entries.into_iter().map(|entry| entry.url).collect())
}
