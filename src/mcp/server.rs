//! MCP server handler — tool routing and server info.

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content, ProtocolVersion, ServerCapabilities, ServerInfo};
use rmcp::{ErrorData, ServerHandler, schemars, tool, tool_handler, tool_router};

use super::tools;

const MAX_JS_LEN: usize = 10_000;
const MAX_JS_OUTPUT_LEN: usize = 1_000_000;
const MAX_TIMEOUT_SECS: u64 = 300;
const MAX_SELECTOR_LEN: usize = 1_000;
const MAX_SETTLE_MS: u64 = 10_000;
const MAX_BATCH_URLS: usize = 20;

/// Output format requested by the MCP `fetch` tool.
#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(super) enum OutputFormat {
    #[default]
    Markdown,
    Json,
    Html,
    Text,
    #[serde(rename = "accessibility_tree")]
    AccessibilityTree,
}

/// Parameters for the MCP `fetch` tool.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub(super) struct FetchParams {
    #[schemars(description = "URL to fetch (http/https only)")]
    url: String,
    #[schemars(description = "Output format: markdown (default), json, html, text, or accessibility_tree")]
    format: Option<OutputFormat>,
    #[schemars(description = "Max characters to return. Default: 5000")]
    max_length: Option<usize>,
    #[schemars(description = "Character offset for pagination. Default: 0")]
    start_index: Option<usize>,
    #[schemars(description = "Page load timeout in seconds. Default: 30")]
    timeout: Option<u64>,
    #[schemars(
        description = "Extra wait in ms after the `load` event, for SPAs that keep hydrating. Default: 0. Max: 10000."
    )]
    settle_ms: Option<u64>,
    #[schemars(description = "CSS selector to extract a specific section instead of full-page Readability extraction")]
    selector: Option<String>,
}

/// Parameters for the MCP `screenshot` tool.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub(super) struct ScreenshotParams {
    #[schemars(description = "URL to capture (http/https only)")]
    url: String,
    #[schemars(description = "Capture the full scrollable page instead of just the viewport. Default: false")]
    full_page: Option<bool>,
    #[schemars(description = "Page load timeout in seconds. Default: 30")]
    timeout: Option<u64>,
    #[schemars(description = "Extra wait in ms after the `load` event. Default: 0. Max: 10000.")]
    settle_ms: Option<u64>,
}

/// Parameters for the MCP `execute_js` tool.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub(super) struct ExecuteJsParams {
    #[schemars(description = "URL to load before executing JS")]
    url: String,
    #[schemars(description = "JavaScript expression to evaluate")]
    expression: String,
    #[schemars(description = "Page load timeout in seconds. Default: 30")]
    timeout: Option<u64>,
    #[schemars(description = "Extra wait in ms after the `load` event. Default: 0. Max: 10000.")]
    settle_ms: Option<u64>,
}

/// Parameters for the MCP `batch_fetch` tool.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub(super) struct BatchFetchParams {
    #[schemars(description = "URLs to fetch (http/https only). Max 20.")]
    urls: Vec<String>,
    #[schemars(description = "Output format: markdown (default) or json")]
    format: Option<BatchFormat>,
    #[schemars(description = "Max characters per URL result. Default: 5000")]
    max_length: Option<usize>,
    #[schemars(description = "Page load timeout in seconds (per URL). Default: 30")]
    timeout: Option<u64>,
    #[schemars(description = "Extra wait in ms after the `load` event. Default: 0. Max: 10000.")]
    settle_ms: Option<u64>,
    #[schemars(description = "CSS selector to extract a specific section")]
    selector: Option<String>,
}

/// Output format for `batch_fetch`.
#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(super) enum BatchFormat {
    #[default]
    Markdown,
    Json,
}

/// MCP handler that exposes servo-fetch's tools.
#[derive(Debug, Clone)]
pub(crate) struct ServoMcp {
    #[allow(dead_code)] // accessed by tool_router macro
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl ServoMcp {
    /// Construct a new MCP handler with the tool router wired up.
    pub(crate) fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        description = "Fetch a URL and extract readable content using the Servo browser engine (JS execution + CSS layout). Navbars, sidebars, and footers are stripped automatically. Use `selector` to extract a specific CSS-selected section instead of full-page Readability extraction. Set format to `accessibility_tree` to get the page's accessibility tree with bounding boxes. Long content is truncated at max_length; use start_index to paginate."
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

        let include_a11y = matches!(p.format, Some(OutputFormat::AccessibilityTree));

        // Probe PDFs before the Servo thread so one slow HEAD never stalls concurrent WebViews.
        let full = if let Some(pdf_bytes) = tools::probe_pdf(&url, timeout).await {
            servo_fetch::extract::extract_pdf(&pdf_bytes)
        } else {
            let page = tools::fetch_page(
                &url,
                timeout,
                settle_ms,
                crate::bridge::FetchMode::Content { include_a11y },
            )
            .await?;
            let fmt = p.format.unwrap_or_default();
            match fmt {
                OutputFormat::Html => page.html,
                OutputFormat::Text => page.inner_text.unwrap_or_default(),
                OutputFormat::Json => tools::extract(&page, &url, true, p.selector.as_deref())?,
                OutputFormat::Markdown => tools::extract(&page, &url, false, p.selector.as_deref())?,
                OutputFormat::AccessibilityTree => page.accessibility_tree.unwrap_or_default(),
            }
        };
        Ok(CallToolResult::success(vec![Content::text(tools::paginate(
            &servo_fetch::sanitize::sanitize(&full),
            start,
            max_len,
        ))]))
    }

    #[tool(
        description = "Capture a PNG screenshot of a web page. Uses Servo's software renderer — no GPU required. Set `full_page` to capture the full scrollable content instead of just the viewport."
    )]
    async fn screenshot(&self, Parameters(p): Parameters<ScreenshotParams>) -> Result<CallToolResult, ErrorData> {
        let url = tools::validated_url(&p.url)?;
        let timeout = p.timeout.unwrap_or(30).clamp(1, MAX_TIMEOUT_SECS);
        let settle_ms = p.settle_ms.unwrap_or(0).min(MAX_SETTLE_MS);
        tools::take_screenshot(&url, timeout, settle_ms, p.full_page.unwrap_or(false)).await
    }

    #[tool(
        description = "Evaluate a JavaScript expression in a loaded page. Console messages (log, warn, error) are appended to the result. Examples: document.title, [...document.querySelectorAll('h2')].map(e => e.textContent)"
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

        let page = tools::fetch_page(
            &url,
            timeout,
            settle_ms,
            crate::bridge::FetchMode::ExecuteJs {
                expression: p.expression,
            },
        )
        .await?;
        let mut result = page.js_result.unwrap_or_default();
        if result.len() > MAX_JS_OUTPUT_LEN {
            result.truncate(servo_fetch::sanitize::floor_char_boundary(&result, MAX_JS_OUTPUT_LEN));
            result.push_str("\n<output truncated>");
        }
        if !page.console_messages.is_empty() {
            result.push_str("\n\n--- console output ---\n");
            for msg in &page.console_messages {
                use std::fmt::Write as _;
                let _ = writeln!(result, "[{}] {}", msg.level, msg.message);
            }
        }
        Ok(CallToolResult::success(vec![Content::text(
            servo_fetch::sanitize::sanitize(&result).into_owned(),
        )]))
    }

    #[tool(
        description = "Fetch multiple URLs in parallel and extract readable content. Results are returned as separate content entries, one per URL, in completion order. Failed URLs are reported inline without aborting the batch."
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
}

#[tool_handler]
impl ServerHandler for ServoMcp {
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
        let server = ServoMcp::new();
        let info = server.get_info();
        assert!(info.server_info.name.contains("servo-fetch"));
        assert!(!info.server_info.version.is_empty());
        assert!(info.instructions.is_some());
    }

    #[test]
    fn output_format_deserializes() {
        let md: OutputFormat = serde_json::from_str("\"markdown\"").unwrap();
        assert!(matches!(md, OutputFormat::Markdown));
        let json: OutputFormat = serde_json::from_str("\"json\"").unwrap();
        assert!(matches!(json, OutputFormat::Json));
    }

    #[test]
    fn output_format_rejects_invalid() {
        assert!(serde_json::from_str::<OutputFormat>("\"xml\"").is_err());
    }

    #[test]
    fn output_format_default_is_markdown() {
        assert!(matches!(OutputFormat::default(), OutputFormat::Markdown));
    }

    #[test]
    fn fetch_params_requires_url() {
        let result = serde_json::from_str::<FetchParams>("{}");
        assert!(result.is_err());
    }

    #[test]
    fn fetch_params_accepts_minimal() {
        let p: FetchParams = serde_json::from_str(r#"{"url":"https://example.com"}"#).unwrap();
        assert_eq!(p.url, "https://example.com");
        assert!(p.format.is_none());
        assert!(p.max_length.is_none());
        assert!(p.start_index.is_none());
        assert!(p.timeout.is_none());
        assert!(p.selector.is_none());
    }

    #[test]
    fn fetch_params_accepts_selector() {
        let p: FetchParams = serde_json::from_str(r#"{"url":"https://example.com","selector":"article"}"#).unwrap();
        assert_eq!(p.selector.as_deref(), Some("article"));
    }

    #[test]
    fn execute_js_params_requires_both() {
        assert!(serde_json::from_str::<ExecuteJsParams>(r#"{"url":"https://example.com"}"#).is_err());
        assert!(serde_json::from_str::<ExecuteJsParams>(r#"{"expression":"1+1"}"#).is_err());
    }

    #[test]
    fn output_format_accessibility_tree() {
        let at: OutputFormat = serde_json::from_str("\"accessibility_tree\"").unwrap();
        assert!(matches!(at, OutputFormat::AccessibilityTree));
    }

    #[test]
    fn fetch_params_with_settle_ms() {
        let p: FetchParams = serde_json::from_str(r#"{"url":"https://example.com","settle_ms":500}"#).unwrap();
        assert_eq!(p.settle_ms, Some(500));
    }

    #[test]
    fn screenshot_params_all_fields() {
        let p: ScreenshotParams =
            serde_json::from_str(r#"{"url":"https://example.com","full_page":true,"timeout":60,"settle_ms":1000}"#)
                .unwrap();
        assert_eq!(p.full_page, Some(true));
        assert_eq!(p.timeout, Some(60));
        assert_eq!(p.settle_ms, Some(1000));
    }

    #[test]
    fn execute_js_params_all_fields() {
        let p: ExecuteJsParams =
            serde_json::from_str(r#"{"url":"https://example.com","expression":"1+1","timeout":10,"settle_ms":200}"#)
                .unwrap();
        assert_eq!(p.timeout, Some(10));
        assert_eq!(p.settle_ms, Some(200));
    }

    #[test]
    fn batch_fetch_params_requires_urls() {
        assert!(serde_json::from_str::<BatchFetchParams>("{}").is_err());
    }

    #[test]
    fn batch_fetch_params_accepts_minimal() {
        let p: BatchFetchParams = serde_json::from_str(r#"{"urls":["https://example.com"]}"#).unwrap();
        assert_eq!(p.urls.len(), 1);
        assert!(p.format.is_none());
    }

    #[test]
    fn batch_format_deserializes() {
        let md: BatchFormat = serde_json::from_str("\"markdown\"").unwrap();
        assert!(matches!(md, BatchFormat::Markdown));
        let json: BatchFormat = serde_json::from_str("\"json\"").unwrap();
        assert!(matches!(json, BatchFormat::Json));
    }
}
