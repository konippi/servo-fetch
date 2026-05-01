//! Crawl tool helper.

use rmcp::ErrorData;

use crate::net;

use super::common::paginate;

const MAX_MCP_CRAWL_PAGES: usize = 500;

pub(crate) struct CrawlToolOptions<'a> {
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

pub(crate) async fn crawl_pages(opts: CrawlToolOptions<'_>) -> Result<Vec<(String, String)>, ErrorData> {
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
