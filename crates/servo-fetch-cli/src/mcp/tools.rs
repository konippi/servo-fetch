//! MCP-specific wrappers over the shared [`crate::tools`] business logic.

use rmcp::ErrorData;
use rmcp::model::{CallToolResult, Content};
use servo_fetch_types::ErrorKind;

use crate::tools::ToolError;
pub(super) use crate::tools::{
    BatchSpec, CrawlSpec, MapSpec, apply_options, batch_fetch_pages, build_map_options, clamp_js_output, fetch_with,
    map_with, paginate, render_page, validate_selector, visibility_policy,
};

pub(super) fn validated_url(url: &str) -> Result<String, ErrorData> {
    crate::tools::validated_url(url).map_err(ErrorData::from)
}

pub(super) fn tool_error(msg: impl std::fmt::Display) -> CallToolResult {
    CallToolResult::error(vec![Content::text(msg.to_string())])
}

pub(super) async fn crawl_pages(spec: CrawlSpec<'_>, max_len: usize) -> Result<Vec<(String, String)>, ErrorData> {
    crate::tools::crawl_pages(spec, max_len).await.map_err(ErrorData::from)
}

impl From<ToolError> for ErrorData {
    fn from(err: ToolError) -> Self {
        let msg = err.to_string();
        match err.kind() {
            ErrorKind::InvalidUrl | ErrorKind::AddressNotAllowed | ErrorKind::InvalidParams => {
                Self::invalid_params(msg, None)
            }
            _ => Self::internal_error(msg, None),
        }
    }
}
