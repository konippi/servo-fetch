//! Map tool helper.

use rmcp::ErrorData;

pub(crate) async fn discover_urls(
    url: &str,
    limit: usize,
    include_glob: &[String],
    exclude_glob: &[String],
) -> Result<Vec<String>, ErrorData> {
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
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

    Ok(results.iter().map(|entry| entry.url.clone()).collect())
}
