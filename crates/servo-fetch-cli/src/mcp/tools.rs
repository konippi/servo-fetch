//! MCP-specific helpers over the shared [`crate::tools`] business logic.

use rmcp::model::CallToolResult;

use super::output::TextOutput;
pub(super) use crate::tools::{
    BatchSpec, CrawlSpec, MapSpec, ToolError, apply_options, batch_fetch_pages, build_map_options, content_options,
    crawl_pages, fetch_with, map_with, paginate, render_page, validate_selector, validated_url, visibility_policy,
};

/// Build an `isError` tool result carrying the failure message for the model to react to.
pub(super) fn tool_error(err: impl std::fmt::Display) -> CallToolResult {
    let mut output = TextOutput::error();
    output.push(err);
    output.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::output::MAX_MCP_SERIALIZED_TEXT_RESULT_BYTES;

    #[test]
    fn tool_error_sanitizes_and_bounds_oversized_malformed_url() {
        let url = format!(
            "not a \x1b[31mURL\x1b[0m/visible\u{202e}\x00\x1b]0;title\x07\u{009b}31mred\u{009b}0m{}unreachable",
            "x".repeat(MAX_MCP_SERIALIZED_TEXT_RESULT_BYTES / 4)
        );
        let err = validated_url(&url).expect_err("malformed URL should fail validation");
        let result = tool_error(err);
        let text = result.content[0].as_text().expect("text error block");

        assert_eq!(result.is_error, Some(true));
        assert!(text.text.chars().count() <= MAX_MCP_SERIALIZED_TEXT_RESULT_BYTES / 4);
        assert!(text.text.starts_with("invalid URL 'not a URL/visible"));
        assert!(!text.text.contains('\x1b'));
        assert!(!text.text.contains('\x00'));
        assert!(!text.text.contains('\u{009b}'));
        assert!(!text.text.contains('\u{202e}'));
        assert!(!text.text.contains("title"));
        assert!(!text.text.contains("unreachable"));
        assert!(text.text.ends_with("<output truncated>"));
        assert!(serde_json::to_vec(&result).unwrap().len() <= MAX_MCP_SERIALIZED_TEXT_RESULT_BYTES);
    }
}
