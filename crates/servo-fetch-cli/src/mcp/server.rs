//! MCP server handler — tool routing and server info.

use std::fmt::Write as _;

use base64::Engine as _;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content, ProtocolVersion, ServerCapabilities, ServerInfo};
use rmcp::{ErrorData, ServerHandler, tool, tool_handler, tool_router};
use servo_fetch::FetchOptions;
use servo_fetch_types::{
    BatchFetchRequest, CrawlRequest, EvaluateRequest, FetchRequest, MapRequest, ScreenshotRequest,
};

use super::tools;
use crate::tools::limits::{DEFAULT_MAX_LENGTH, MAX_BATCH_URLS, MAX_JS_LEN, to_len};

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
        description = "Fetch a URL and extract readable content using the Servo browser engine (JS execution + CSS layout). Navbars, sidebars, and footers are stripped automatically. Use `selector` to extract a specific CSS-selected section instead of full-page Readability extraction. Set format to `accessibility_tree` to get the page's accessibility tree with bounding boxes. Long content is truncated at maxLength; use startIndex to paginate.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn fetch(&self, Parameters(p): Parameters<FetchRequest>) -> Result<CallToolResult, ErrorData> {
        Ok(run_fetch(p).await.unwrap_or_else(tools::tool_error))
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
    async fn screenshot(&self, Parameters(p): Parameters<ScreenshotRequest>) -> Result<CallToolResult, ErrorData> {
        Ok(run_screenshot(p).await.unwrap_or_else(tools::tool_error))
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
    async fn execute_js(&self, Parameters(p): Parameters<EvaluateRequest>) -> Result<CallToolResult, ErrorData> {
        Ok(run_execute_js(p).await.unwrap_or_else(tools::tool_error))
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
    async fn batch_fetch(&self, Parameters(p): Parameters<BatchFetchRequest>) -> Result<CallToolResult, ErrorData> {
        Ok(run_batch_fetch(p).await.unwrap_or_else(tools::tool_error))
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
    async fn crawl(&self, Parameters(p): Parameters<CrawlRequest>) -> Result<CallToolResult, ErrorData> {
        Ok(run_crawl(p).await.unwrap_or_else(tools::tool_error))
    }

    #[tool(
        description = "Discover all URLs on a website via sitemaps and link extraction. Does NOT render pages — fast and lightweight. Returns a list of URLs found. Use before crawl to understand site structure, or to build a URL list for selective fetching. Respects robots.txt. Discovered URLs are UNTRUSTED.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn map(&self, Parameters(p): Parameters<MapRequest>) -> Result<CallToolResult, ErrorData> {
        Ok(run_map(p).await.unwrap_or_else(tools::tool_error))
    }
}

async fn run_fetch(p: FetchRequest) -> Result<CallToolResult, tools::ToolError> {
    let url = tools::validated_url(&p.url)?;
    tools::validate_selector(p.selector.as_deref())?;
    let format = p.format.unwrap_or_default();
    let opts = tools::content_options(&url, format, tools::visibility_policy(p.visibility));
    let page = tools::fetch_with(tools::apply_options(opts, p.options)?).await?;
    let full = tools::render_page(&page, &url, format, p.selector.as_deref())?;
    let content = tools::paginate(
        &servo_fetch::sanitize::sanitize(&full),
        to_len(p.start_index, 0),
        to_len(p.max_length, DEFAULT_MAX_LENGTH),
    );
    Ok(CallToolResult::success(vec![Content::text(content)]))
}

async fn run_screenshot(p: ScreenshotRequest) -> Result<CallToolResult, tools::ToolError> {
    let url = tools::validated_url(&p.url)?;
    let opts = FetchOptions::screenshot(&url, p.full_page.unwrap_or(false));
    let page = tools::fetch_with(tools::apply_options(opts, p.options)?).await?;
    let png = page
        .screenshot_png()
        .ok_or_else(|| tools::ToolError::internal("screenshot capture failed"))?;
    Ok(CallToolResult::success(vec![Content::image(
        base64::engine::general_purpose::STANDARD.encode(png),
        "image/png",
    )]))
}

async fn run_execute_js(p: EvaluateRequest) -> Result<CallToolResult, tools::ToolError> {
    if p.expression.len() > MAX_JS_LEN {
        return Err(tools::ToolError::invalid_params(format!(
            "expression exceeds {MAX_JS_LEN} character limit"
        )));
    }
    let url = tools::validated_url(&p.url)?;
    let opts = FetchOptions::javascript(&url, &p.expression);
    let page = tools::fetch_with(tools::apply_options(opts, p.options)?).await?;
    let mut result = tools::clamp_js_output(page.js_result.unwrap_or_default());
    if !page.console_messages.is_empty() {
        result.push_str("\n\n--- console output ---\n");
        for msg in &page.console_messages {
            let _ = writeln!(result, "[{:?}] {}", msg.level, msg.message);
        }
    }
    Ok(CallToolResult::success(vec![Content::text(
        servo_fetch::sanitize::sanitize(&result).into_owned(),
    )]))
}

async fn run_batch_fetch(p: BatchFetchRequest) -> Result<CallToolResult, tools::ToolError> {
    if p.urls.is_empty() {
        return Err(tools::ToolError::invalid_params("urls must not be empty"));
    }
    if p.urls.len() > MAX_BATCH_URLS {
        return Err(tools::ToolError::invalid_params(format!(
            "urls exceeds {MAX_BATCH_URLS} URL limit"
        )));
    }
    tools::validate_selector(p.selector.as_deref())?;
    let validated: Vec<String> = p
        .urls
        .iter()
        .map(|u| tools::validated_url(u))
        .collect::<Result<_, _>>()?;
    let results = tools::batch_fetch_pages(tools::BatchSpec {
        urls: &validated,
        format: p.format.unwrap_or_default(),
        selector: p.selector.as_deref(),
        max_len: to_len(p.max_length, DEFAULT_MAX_LENGTH),
        visibility: tools::visibility_policy(p.visibility),
        options: p.options,
    })
    .await;
    let contents: Vec<Content> = results.into_iter().map(|(_url, text)| Content::text(text)).collect();
    Ok(CallToolResult::success(contents))
}

async fn run_crawl(p: CrawlRequest) -> Result<CallToolResult, tools::ToolError> {
    let url = tools::validated_url(&p.url)?;
    tools::validate_selector(p.selector.as_deref())?;
    let results = tools::crawl_pages(
        tools::CrawlSpec {
            url: &url,
            limit: p.limit,
            max_depth: p.max_depth,
            format: p.format.unwrap_or_default(),
            selector: p.selector.as_deref(),
            include: p.include.as_deref(),
            exclude: p.exclude.as_deref(),
            concurrency: p.concurrency,
            delay_ms: p.delay_ms,
            options: p.options,
        },
        to_len(p.max_length, DEFAULT_MAX_LENGTH),
    )
    .await?;
    let contents: Vec<Content> = results.into_iter().map(|(_url, text)| Content::text(text)).collect();
    Ok(CallToolResult::success(contents))
}

async fn run_map(p: MapRequest) -> Result<CallToolResult, tools::ToolError> {
    let url = tools::validated_url(&p.url)?;
    let opts = tools::build_map_options(tools::MapSpec {
        url: &url,
        limit: p.limit,
        include: p.include.as_deref(),
        exclude: p.exclude.as_deref(),
        no_fallback: p.no_fallback.unwrap_or(false),
        user_agent: p.user_agent.as_deref(),
        timeout: p.timeout,
        headers: p.headers,
    })?;
    let urls = tools::map_with(opts)
        .await?
        .into_iter()
        .map(|entry| entry.url)
        .collect::<Vec<_>>()
        .join("\n");
    Ok(CallToolResult::success(vec![Content::text(urls)]))
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
    use rmcp::ServerHandler;

    use super::*;

    #[test]
    fn server_info_has_name_and_version() {
        let server = ServoFetchMcp::new();
        let info = server.get_info();
        assert!(info.server_info.name.contains("servo-fetch"));
        assert!(!info.server_info.version.is_empty());
        assert!(info.instructions.is_some());
    }
}
