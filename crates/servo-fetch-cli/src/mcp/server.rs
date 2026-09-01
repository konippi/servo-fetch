//! MCP server handler — tool routing and server info.

use std::future::Future;
use std::marker::PhantomData;

use base64::Engine as _;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock, JsonObject, ProtocolVersion, ServerCapabilities, ServerInfo};
use rmcp::schemars::{JsonSchema, Schema, SchemaGenerator};
use rmcp::{ErrorData, ServerHandler, tool, tool_handler, tool_router};
use serde::de::DeserializeOwned;
use servo_fetch::FetchOptions;
use servo_fetch_types::{
    BatchFetchRequest, CrawlRequest, EvaluateRequest, FetchRequest, MapRequest, ScreenshotRequest,
};

use super::{output, tools};
use crate::tools::limits::{CRAWL_LIMIT, DEFAULT_MAX_LENGTH, MAX_BATCH_URLS, MAX_JS_LEN, clamp_count, to_len};

#[derive(serde::Deserialize)]
#[serde(transparent, bound(deserialize = ""))]
struct RawArguments<T> {
    value: JsonObject,
    #[serde(skip)]
    request: PhantomData<fn() -> T>,
}

impl<T: JsonSchema> JsonSchema for RawArguments<T> {
    fn inline_schema() -> bool {
        T::inline_schema()
    }

    fn schema_name() -> std::borrow::Cow<'static, str> {
        T::schema_name()
    }

    fn schema_id() -> std::borrow::Cow<'static, str> {
        T::schema_id()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        T::json_schema(generator)
    }
}

async fn complete_decoded_tool_call<T, F, Fut>(
    tool: &'static str,
    arguments: RawArguments<T>,
    run: F,
) -> Result<CallToolResult, ErrorData>
where
    T: DeserializeOwned,
    F: FnOnce(T) -> Fut,
    Fut: Future<Output = Result<CallToolResult, tools::ToolError>>,
{
    let request = serde_json::from_value(serde_json::Value::Object(arguments.value))
        .map_err(|error| tools::ToolError::invalid_params(format!("failed to deserialize parameters: {error}")));
    match request {
        Ok(request) => complete_tool_call(tool, run(request).await),
        Err(error) => complete_tool_call(tool, Err(error)),
    }
}

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
    async fn fetch(&self, Parameters(p): Parameters<RawArguments<FetchRequest>>) -> Result<CallToolResult, ErrorData> {
        complete_decoded_tool_call("fetch", p, run_fetch).await
    }

    #[tool(
        description = "Capture a PNG screenshot of a web page. Uses Servo's software renderer — no GPU required. Set `fullPage` to capture the full scrollable content instead of just the viewport.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn screenshot(
        &self,
        Parameters(p): Parameters<RawArguments<ScreenshotRequest>>,
    ) -> Result<CallToolResult, ErrorData> {
        complete_decoded_tool_call("screenshot", p, run_screenshot).await
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
    async fn execute_js(
        &self,
        Parameters(p): Parameters<RawArguments<EvaluateRequest>>,
    ) -> Result<CallToolResult, ErrorData> {
        complete_decoded_tool_call("execute_js", p, run_execute_js).await
    }

    #[tool(
        description = "Fetch multiple URLs in parallel and extract readable content. Results are returned as separate content entries in completion order. Failed URLs are reported inline without aborting the batch.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn batch_fetch(
        &self,
        Parameters(p): Parameters<RawArguments<BatchFetchRequest>>,
    ) -> Result<CallToolResult, ErrorData> {
        complete_decoded_tool_call("batch_fetch", p, run_batch_fetch).await
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
    async fn crawl(&self, Parameters(p): Parameters<RawArguments<CrawlRequest>>) -> Result<CallToolResult, ErrorData> {
        complete_decoded_tool_call("crawl", p, run_crawl).await
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
    async fn map(&self, Parameters(p): Parameters<RawArguments<MapRequest>>) -> Result<CallToolResult, ErrorData> {
        complete_decoded_tool_call("map", p, run_map).await
    }
}

const INTERNAL_ERROR_MESSAGE: &str = "Internal server error";

fn complete_tool_call(
    tool: &'static str,
    result: Result<CallToolResult, tools::ToolError>,
) -> Result<CallToolResult, ErrorData> {
    match result {
        Ok(result) => Ok(result),
        Err(error) if error.is_internal() => {
            tracing::error!(tool, kind = ?error.kind(), error = ?error, "MCP tool internal failure");
            Err(ErrorData::internal_error(INTERNAL_ERROR_MESSAGE, None))
        }
        Err(error) => Ok(tools::tool_error(error)),
    }
}

fn labeled_results(
    results: Vec<(String, Result<String, tools::ToolError>)>,
) -> Result<CallToolResult, tools::ToolError> {
    let mut output = output::TextOutput::success();
    for (url, result) in results {
        match result {
            Ok(content) => {
                output.push(output::labeled_content(&url, content));
            }
            Err(error) if error.is_operation() => {
                output.push(output::labeled_content(&url, format_args!("[error] {error}")));
            }
            Err(error) => return Err(error),
        }
    }
    Ok(output.finish())
}

async fn run_fetch(p: FetchRequest) -> Result<CallToolResult, tools::ToolError> {
    let max_length = output::effective_item_length(to_len(p.max_length, DEFAULT_MAX_LENGTH), 1);
    let url = tools::validated_url(&p.url)?;
    tools::validate_selector(p.selector.as_deref())?;
    let format = p.format.unwrap_or_default();
    let opts = tools::content_options(&url, format, tools::visibility_policy(p.visibility));
    let page = tools::fetch_with(tools::apply_options(opts, p.options)?).await?;
    let full = tools::render_page(&page, &url, format, p.selector.as_deref())?;
    let content = tools::paginate(
        &servo_fetch::sanitize::sanitize(&full),
        to_len(p.start_index, 0),
        max_length,
    );
    let mut output = output::TextOutput::success();
    output.push(content);
    Ok(output.finish())
}

async fn run_screenshot(p: ScreenshotRequest) -> Result<CallToolResult, tools::ToolError> {
    let url = tools::validated_url(&p.url)?;
    let opts = FetchOptions::screenshot(&url, p.full_page.unwrap_or(false));
    let page = tools::fetch_with(tools::apply_options(opts, p.options)?).await?;
    let png = page
        .screenshot_png()
        .ok_or_else(|| tools::ToolError::internal("screenshot capture failed"))?;
    output::checked_base64_length(png.len(), output::MAX_MCP_SCREENSHOT_BASE64_BYTES)?;
    Ok(CallToolResult::success(vec![ContentBlock::image(
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
    let result = output::javascript_text(
        page.js_result.as_deref().unwrap_or_default(),
        page.console_messages
            .iter()
            .map(|msg| (format!("{:?}", msg.level), msg.message.as_str())),
    );
    let mut output = output::TextOutput::success();
    output.push(result);
    Ok(output.finish())
}

async fn run_batch_fetch(p: BatchFetchRequest) -> Result<CallToolResult, tools::ToolError> {
    let requested_max_length = to_len(p.max_length, DEFAULT_MAX_LENGTH);
    if p.urls.is_empty() {
        return Err(tools::ToolError::invalid_params("urls must not be empty"));
    }
    if p.urls.len() > MAX_BATCH_URLS {
        return Err(tools::ToolError::invalid_params(format!(
            "urls exceeds {MAX_BATCH_URLS} URL limit"
        )));
    }
    tools::validate_selector(p.selector.as_deref())?;
    let max_len = output::effective_item_length(requested_max_length, p.urls.len());
    let validated: Vec<String> = p
        .urls
        .iter()
        .map(|u| tools::validated_url(u))
        .collect::<Result<_, _>>()?;
    let results = tools::batch_fetch_pages(tools::BatchSpec {
        urls: &validated,
        format: p.format.unwrap_or_default(),
        selector: p.selector.as_deref(),
        max_len,
        visibility: tools::visibility_policy(p.visibility),
        options: p.options,
    })
    .await?;
    labeled_results(results)
}

async fn run_crawl(p: CrawlRequest) -> Result<CallToolResult, tools::ToolError> {
    let requested_max_length = to_len(p.max_length, DEFAULT_MAX_LENGTH);
    let page_limit = clamp_count(p.limit, CRAWL_LIMIT);
    let max_len = output::effective_item_length(requested_max_length, page_limit);
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
        max_len,
    )
    .await?;
    labeled_results(results)
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
    let text = output::map_text(tools::map_with(opts).await?.into_iter().map(|entry| entry.url));
    let mut output = output::TextOutput::success();
    output.push(text);
    Ok(output.finish())
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

    struct IdentitySchema;

    impl JsonSchema for IdentitySchema {
        fn inline_schema() -> bool {
            true
        }

        fn schema_name() -> std::borrow::Cow<'static, str> {
            "identity-name".into()
        }

        fn schema_id() -> std::borrow::Cow<'static, str> {
            "identity-id".into()
        }

        fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
            serde_json::json!({"type": "object"}).try_into().expect("valid schema")
        }
    }

    #[test]
    fn raw_arguments_delegate_schema_identity() {
        assert_eq!(
            RawArguments::<IdentitySchema>::inline_schema(),
            IdentitySchema::inline_schema()
        );
        assert_eq!(
            RawArguments::<IdentitySchema>::schema_name(),
            IdentitySchema::schema_name()
        );
        assert_eq!(RawArguments::<IdentitySchema>::schema_id(), IdentitySchema::schema_id());
    }

    #[test]
    fn validation_and_operation_failures_remain_tool_errors() {
        let validation = complete_tool_call("fetch", Err(tools::ToolError::invalid_params("caller can fix this")))
            .expect("validation should be a tool result");
        assert_eq!(validation.is_error, Some(true));

        let operation = complete_tool_call(
            "fetch",
            Err(tools::ToolError::from(servo_fetch::Error::Timeout {
                url: "https://example.com".to_string(),
                timeout: std::time::Duration::from_secs(1),
            })),
        )
        .expect("operation failure should be a tool result");
        assert_eq!(operation.is_error, Some(true));
    }

    #[test]
    fn internal_failure_is_generic_json_rpc_error_without_data_or_detail() {
        let detail = "sensitive worker transport detail";
        let failure = servo_fetch::Error::WorkerUnavailable {
            source: std::io::Error::other(detail).into(),
        };
        let error = complete_tool_call("fetch", Err(tools::ToolError::from(failure)))
            .expect_err("internal failure should be a JSON-RPC error");

        assert_eq!(error.code, rmcp::model::ErrorCode::INTERNAL_ERROR);
        assert_eq!(error.message, INTERNAL_ERROR_MESSAGE);
        assert_eq!(error.data, None);
        assert!(!serde_json::to_string(&error).unwrap().contains(detail));
    }

    #[test]
    fn batch_and_crawl_typed_partial_results_keep_labels_and_propagate_input_and_internal_errors() {
        let results = vec![
            ("https://example.com/ok".to_string(), Ok("body".to_string())),
            (
                "https://example.com/failed".to_string(),
                Err(tools::ToolError::from(servo_fetch::Error::Timeout {
                    url: "https://example.com/failed".to_string(),
                    timeout: std::time::Duration::from_secs(1),
                })),
            ),
        ];
        let result = labeled_results(results).expect("ordinary per-item failures should be partial results");
        let text: Vec<_> = result
            .content
            .iter()
            .map(|block| block.as_text().expect("text block").text.as_str())
            .collect();
        assert_eq!(text.len(), 2);
        assert_eq!(text[0], "URL: https://example.com/ok\n\nbody");
        assert!(text[1].starts_with("URL: https://example.com/failed\n\n[error] page load timed out"));

        let invalid_input = labeled_results(vec![(
            "https://example.com/invalid".to_string(),
            Err(tools::ToolError::invalid_params("invalid shared option")),
        )])
        .expect_err("invalid per-item input must become an isError tool result");
        assert_eq!(invalid_input.to_string(), "invalid shared option");

        let internal = labeled_results(vec![(
            "https://example.com/private".to_string(),
            Err(tools::ToolError::internal("private detail")),
        )])
        .expect_err("internal per-item failure must not be returned inline");
        assert_eq!(internal.to_string(), "private detail");
    }
}
