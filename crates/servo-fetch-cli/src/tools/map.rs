//! Map tool helper.

use super::error::{ToolError, ToolResult};

pub(crate) async fn discover_urls(
    url: &str,
    limit: usize,
    include_glob: &[String],
    exclude_glob: &[String],
) -> ToolResult<Vec<String>> {
    let opts = servo_fetch::MapOptions::new(url).limit(limit);
    let opts = if include_glob.is_empty() {
        opts
    } else {
        opts.include(&include_glob.iter().map(String::as_str).collect::<Vec<_>>())
    };
    let opts = if exclude_glob.is_empty() {
        opts
    } else {
        opts.exclude(&exclude_glob.iter().map(String::as_str).collect::<Vec<_>>())
    };

    let results = tokio::task::spawn_blocking(move || servo_fetch::map(opts))
        .await
        .map_err(|e| ToolError::internal(e.to_string()))?
        .map_err(|e| ToolError::fetch(e.to_string()))?;

    Ok(results.iter().map(|entry| entry.url.clone()).collect())
}
