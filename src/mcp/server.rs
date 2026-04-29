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
        let max_len = p.max_length.unwrap_or(5000);
        let start = p.start_index.unwrap_or(0);

        if p.selector.as_ref().is_some_and(|s| s.len() > MAX_SELECTOR_LEN) {
            return Err(ErrorData::invalid_params(
                format!("selector exceeds {MAX_SELECTOR_LEN} character limit"),
                None,
            ));
        }

        let include_a11y = matches!(p.format, Some(OutputFormat::AccessibilityTree));
        let page = tools::fetch_page(&url, timeout, crate::bridge::FetchMode::Content { include_a11y }).await?;
        let full = if let Some(ref pdf_bytes) = page.pdf_data {
            servo_fetch::extract::extract_pdf(pdf_bytes)
        } else {
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
        tools::take_screenshot(&url, timeout, p.full_page.unwrap_or(false)).await
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

        let page = tools::fetch_page(
            &url,
            timeout,
            crate::bridge::FetchMode::ExecuteJs {
                expression: p.expression,
            },
        )
        .await?;
        let mut result = page.js_result.unwrap_or_default();
        if result.len() > MAX_JS_OUTPUT_LEN {
            result.truncate(tools::floor_char_boundary(&result, MAX_JS_OUTPUT_LEN));
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
}
