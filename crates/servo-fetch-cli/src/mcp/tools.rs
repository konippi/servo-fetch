//! MCP-specific helpers over the shared [`crate::tools`] business logic.

use rmcp::model::{CallToolResult, Content};

pub(super) use crate::tools::{
    BatchSpec, CrawlSpec, MapSpec, ToolError, apply_options, batch_fetch_pages, build_map_options, clamp_js_output,
    crawl_pages, fetch_with, map_with, paginate, render_page, validate_selector, validated_url, visibility_policy,
};

/// Build an `isError` tool result carrying the failure message for the model to react to.
pub(super) fn tool_error(err: impl std::fmt::Display) -> CallToolResult {
    CallToolResult::error(vec![Content::text(err.to_string())])
}
