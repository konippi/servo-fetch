//! MCP-specific wrappers over the shared [`crate::tools`] business logic.

use base64::Engine as _;
use rmcp::ErrorData;
use rmcp::model::{CallToolResult, Content};

use crate::tools::{ToolError, ToolResult};

pub(super) use crate::tools::{
    CrawlOptions as CrawlToolOptions, batch_fetch_pages, extract, fetch_js, fetch_page, paginate,
};

pub(super) fn validated_url(url: &str) -> Result<String, ErrorData> {
    crate::tools::validated_url(url).map_err(ErrorData::from)
}

pub(super) fn tool_error(msg: impl std::fmt::Display) -> CallToolResult {
    CallToolResult::error(vec![Content::text(msg.to_string())])
}

pub(super) async fn take_screenshot(
    url: &str,
    timeout: u64,
    settle_ms: u64,
    full_page: bool,
) -> ToolResult<CallToolResult> {
    let png = crate::tools::take_screenshot(url, timeout, settle_ms, full_page).await?;
    Ok(CallToolResult::success(vec![Content::image(
        base64::engine::general_purpose::STANDARD.encode(png),
        "image/png",
    )]))
}

pub(super) async fn crawl_pages(opts: CrawlToolOptions<'_>) -> Result<Vec<(String, String)>, ErrorData> {
    crate::tools::crawl_pages(opts).await.map_err(ErrorData::from)
}

pub(super) async fn discover_urls(
    url: &str,
    limit: usize,
    include_glob: &[String],
    exclude_glob: &[String],
) -> Result<Vec<String>, ErrorData> {
    crate::tools::discover_urls(url, limit, include_glob, exclude_glob)
        .await
        .map_err(ErrorData::from)
}

impl From<ToolError> for ErrorData {
    fn from(err: ToolError) -> Self {
        let msg = err.to_string();
        match err {
            ToolError::InvalidInput(_) => Self::invalid_params(msg, None),
            ToolError::Fetch(_) | ToolError::Internal(_) => Self::internal_error(msg, None),
        }
    }
}
