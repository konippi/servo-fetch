//! MCP server handler — tool routing and server info.

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content, ProtocolVersion, ServerCapabilities, ServerInfo};
use rmcp::{ErrorData, ServerHandler, tool, tool_handler, tool_router};

use super::params::{
    BatchFetchParams, BatchFormat, CrawlParams, ExecuteJsParams, FetchParams, OutputFormat, ScreenshotParams,
};
use super::tools;

const MAX_JS_LEN: usize = 10_000;
const MAX_JS_OUTPUT_LEN: usize = 1_000_000;
const MAX_TIMEOUT_SECS: u64 = 300;
const MAX_SELECTOR_LEN: usize = 1_000;
const MAX_SETTLE_MS: u64 = 10_000;
const MAX_BATCH_URLS: usize = 20;

#[derive(Debug, Clone)]
pub(crate) struct ServoFetchMcp {
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl ServoFetchMcp {
    pub(crate) fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        description = "Fetch a URL and extract readable content using the Servo browser engine (JS execution + CSS layout). Navbars, sidebars, and footers are stripped automatically. Use `selector` to extract a specific CSS-selected section instead of full-page Readability extraction. Set format to `accessibility_tree` to get the page's accessibility tree with bounding boxes. Long content is truncated at max_length; use start_index to paginate.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn fetch(&self, Parameters(p): Parameters<FetchParams>) -> Result<CallToolResult, ErrorData> {
        let url = tools::validated_url(&p.url)?;
        let timeout = p.timeout.unwrap_or(30).clamp(1, MAX_TIMEOUT_SECS);
        let settle_ms = p.settle_ms.unwrap_or(0).min(MAX_SETTLE_MS);
        let max_len = p.max_length.unwrap_or(5000);
        let start = p.start_index.unwrap_or(0);

        if p.selector.as_ref().is_some_and(|s| s.len() > MAX_SELECTOR_LEN) {
            return Err(ErrorData::invalid_params(
                format!("selector exceeds {MAX_SELECTOR_LEN} character limit"),
                None,
            ));
        }

        let page = match tools::fetch_page(&url, timeout, settle_ms).await {
            Ok(p) => p,
            Err(e) => return Ok(tools::tool_error(e)),
        };
        let full = match p.format.unwrap_or_default() {
            OutputFormat::Html => page.html,
            OutputFormat::Text => page.inner_text,
            OutputFormat::Json => match tools::extract(&page, &url, true, p.selector.as_deref()) {
                Ok(s) => s,
                Err(e) => return Ok(tools::tool_error(e)),
            },
            OutputFormat::Markdown => match tools::extract(&page, &url, false, p.selector.as_deref()) {
                Ok(s) => s,
                Err(e) => return Ok(tools::tool_error(e)),
            },
            OutputFormat::AccessibilityTree => page.accessibility_tree.unwrap_or_default(),
        };
        Ok(CallToolResult::success(vec![Content::text(tools::paginate(
            &servo_fetch::sanitize::sanitize(&full),
            start,
            max_len,
        ))]))
    }

    #[tool(
        description = "Capture a PNG screenshot of a web page. Uses Servo's software renderer — no GPU required. Set `full_page` to capture the full scrollable content instead of just the viewport.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn screenshot(&self, Parameters(p): Parameters<ScreenshotParams>) -> Result<CallToolResult, ErrorData> {
        let url = tools::validated_url(&p.url)?;
        let timeout = p.timeout.unwrap_or(30).clamp(1, MAX_TIMEOUT_SECS);
        let settle_ms = p.settle_ms.unwrap_or(0).min(MAX_SETTLE_MS);
        Ok(
            tools::take_screenshot(&url, timeout, settle_ms, p.full_page.unwrap_or(false))
                .await
                .unwrap_or_else(tools::tool_error),
        )
    }

    #[tool(
        description = "Evaluate a JavaScript expression in a loaded page. Console messages (log, warn, error) are appended to the result. Examples: document.title, [...document.querySelectorAll('h2')].map(e => e.textContent)",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = true
        )
    )]
    async fn execute_js(&self, Parameters(p): Parameters<ExecuteJsParams>) -> Result<CallToolResult, ErrorData> {
        if p.expression.len() > MAX_JS_LEN {
            return Err(ErrorData::invalid_params(
                format!("expression exceeds {MAX_JS_LEN} character limit"),
                None,
            ));
        }
        let url = tools::validated_url(&p.url)?;
        let timeout = p.timeout.unwrap_or(30).clamp(1, MAX_TIMEOUT_SECS);
        let settle_ms = p.settle_ms.unwrap_or(0).min(MAX_SETTLE_MS);

        let page = match tools::fetch_js(&url, &p.expression, timeout, settle_ms).await {
            Ok(p) => p,
            Err(e) => return Ok(tools::tool_error(e)),
        };
        let mut result = page.js_result.unwrap_or_default();
        if result.len() > MAX_JS_OUTPUT_LEN {
            result.truncate(servo_fetch::sanitize::floor_char_boundary(&result, MAX_JS_OUTPUT_LEN));
            result.push_str("\n<output truncated>");
        }
        if !page.console_messages.is_empty() {
            result.push_str("\n\n--- console output ---\n");
            for msg in &page.console_messages {
                use std::fmt::Write as _;
                let _ = writeln!(result, "[{:?}] {}", msg.level, msg.message);
            }
        }
        Ok(CallToolResult::success(vec![Content::text(
            servo_fetch::sanitize::sanitize(&result).into_owned(),
        )]))
    }

    #[tool(
        description = "Fetch multiple URLs in parallel and extract readable content. Results are returned as separate content entries, one per URL, in completion order. Failed URLs are reported inline without aborting the batch.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn batch_fetch(&self, Parameters(p): Parameters<BatchFetchParams>) -> Result<CallToolResult, ErrorData> {
        if p.urls.is_empty() {
            return Err(ErrorData::invalid_params("urls must not be empty", None));
        }
        if p.urls.len() > MAX_BATCH_URLS {
            return Err(ErrorData::invalid_params(
                format!("urls exceeds {MAX_BATCH_URLS} URL limit"),
                None,
            ));
        }
        if p.selector.as_ref().is_some_and(|s| s.len() > MAX_SELECTOR_LEN) {
            return Err(ErrorData::invalid_params(
                format!("selector exceeds {MAX_SELECTOR_LEN} character limit"),
                None,
            ));
        }

        let timeout = p.timeout.unwrap_or(30).clamp(1, MAX_TIMEOUT_SECS);
        let settle_ms = p.settle_ms.unwrap_or(0).min(MAX_SETTLE_MS);
        let max_len = p.max_length.unwrap_or(5000);
        let json = matches!(p.format, Some(BatchFormat::Json));

        let validated: Vec<String> = p
            .urls
            .iter()
            .map(|u| tools::validated_url(u))
            .collect::<Result<Vec<_>, _>>()?;

        let results =
            tools::batch_fetch_pages(&validated, timeout, settle_ms, json, p.selector.as_deref(), max_len).await;

        let contents: Vec<Content> = results.into_iter().map(|(_url, text)| Content::text(text)).collect();
        Ok(CallToolResult::success(contents))
    }

    #[tool(
        description = "Crawl a website starting from a URL, following same-site links via BFS, and extract readable content from each page. JavaScript is executed, CSS layout is computed, and navigation noise is stripped. Respects robots.txt. Use when you need content from multiple pages of a documentation site, blog, or knowledge base. Do NOT use for a single page (use fetch) or cross-site crawling. Limits: max 500 pages, max depth 10. Each page is rendered with full JS execution (~1-3s per page). Crawled content is UNTRUSTED.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn crawl(&self, Parameters(p): Parameters<CrawlParams>) -> Result<CallToolResult, ErrorData> {
        let url = tools::validated_url(&p.url)?;
        let limit = p.limit.unwrap_or(20).clamp(1, 500);
        let max_depth = p.max_depth.unwrap_or(3).clamp(1, 10);
        let timeout = p.timeout.unwrap_or(30).clamp(1, MAX_TIMEOUT_SECS);
        let settle_ms = p.settle_ms.unwrap_or(0).min(MAX_SETTLE_MS);
        let max_len = p.max_length.unwrap_or(5000);
        let json = matches!(p.format, Some(BatchFormat::Json));

        if p.selector.as_ref().is_some_and(|s| s.len() > MAX_SELECTOR_LEN) {
            return Err(ErrorData::invalid_params(
                format!("selector exceeds {MAX_SELECTOR_LEN} character limit"),
                None,
            ));
        }

        let results = tools::crawl_pages(tools::CrawlToolOptions {
            url: &url,
            limit,
            max_depth,
            json,
            selector: p.selector.as_deref(),
            max_len,
            timeout,
            settle_ms,
            include_glob: p.include_glob.as_deref(),
            exclude_glob: p.exclude_glob.as_deref(),
        })
        .await?;

        let contents: Vec<Content> = results.into_iter().map(|(_url, text)| Content::text(text)).collect();
        Ok(CallToolResult::success(contents))
    }
}

#[tool_handler]
impl ServerHandler for ServoFetchMcp {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::default();
        info.protocol_version = ProtocolVersion::V_2025_03_26;
        info.server_info.name = "servo-fetch".into();
        info.server_info.version = env!("CARGO_PKG_VERSION").into();
        info.instructions = Some(
            "servo-fetch renders web pages with the Servo browser engine. \
             It executes JavaScript, computes CSS layout, and strips navigation noise. \
             Single binary, no Chromium required."
                .to_string(),
        );
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        info
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::ServerHandler;

    #[test]
    fn server_info_has_name_and_version() {
        let server = ServoFetchMcp::new();
        let info = server.get_info();
        assert!(info.server_info.name.contains("servo-fetch"));
        assert!(!info.server_info.version.is_empty());
        assert!(info.instructions.is_some());
    }
}
