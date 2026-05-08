//! MCP tool parameter types.

use rmcp::schemars;

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

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(super) enum BatchFormat {
    #[default]
    Markdown,
    Json,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub(super) struct FetchParams {
    #[schemars(description = "URL to fetch (http/https only)")]
    pub url: String,
    #[schemars(description = "Output format: markdown (default), json, html, text, or accessibility_tree")]
    pub format: Option<OutputFormat>,
    #[schemars(description = "Max characters to return. Default: 5000")]
    pub max_length: Option<usize>,
    #[schemars(description = "Character offset for pagination. Default: 0")]
    pub start_index: Option<usize>,
    #[schemars(description = "Page load timeout in seconds. Default: 30")]
    pub timeout: Option<u64>,
    #[schemars(
        description = "Extra wait in ms after the `load` event, for SPAs that keep hydrating. Default: 0. Max: 10000."
    )]
    pub settle_ms: Option<u64>,
    #[schemars(description = "CSS selector to extract a specific section instead of full-page Readability extraction")]
    pub selector: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub(super) struct ScreenshotParams {
    #[schemars(description = "URL to capture (http/https only)")]
    pub url: String,
    #[schemars(description = "Capture the full scrollable page instead of just the viewport. Default: false")]
    pub full_page: Option<bool>,
    #[schemars(description = "Page load timeout in seconds. Default: 30")]
    pub timeout: Option<u64>,
    #[schemars(description = "Extra wait in ms after the `load` event. Default: 0. Max: 10000.")]
    pub settle_ms: Option<u64>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub(super) struct ExecuteJsParams {
    #[schemars(description = "URL to load before executing JS")]
    pub url: String,
    #[schemars(description = "JavaScript expression to evaluate")]
    pub expression: String,
    #[schemars(description = "Page load timeout in seconds. Default: 30")]
    pub timeout: Option<u64>,
    #[schemars(description = "Extra wait in ms after the `load` event. Default: 0. Max: 10000.")]
    pub settle_ms: Option<u64>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub(super) struct BatchFetchParams {
    #[schemars(description = "URLs to fetch (http/https only). Max 20.")]
    pub urls: Vec<String>,
    #[schemars(description = "Output format: markdown (default) or json")]
    pub format: Option<BatchFormat>,
    #[schemars(description = "Max characters per URL result. Default: 5000")]
    pub max_length: Option<usize>,
    #[schemars(description = "Page load timeout in seconds (per URL). Default: 30")]
    pub timeout: Option<u64>,
    #[schemars(description = "Extra wait in ms after the `load` event. Default: 0. Max: 10000.")]
    pub settle_ms: Option<u64>,
    #[schemars(description = "CSS selector to extract a specific section")]
    pub selector: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub(super) struct CrawlParams {
    #[schemars(description = "Starting URL to crawl (http/https only)")]
    pub url: String,
    #[schemars(description = "Maximum pages to crawl. Default: 20. Max: 500.")]
    pub limit: Option<usize>,
    #[schemars(description = "Maximum link depth from seed URL. Default: 3. Max: 10.")]
    pub max_depth: Option<usize>,
    #[schemars(description = "Output format per page: markdown (default) or json")]
    pub format: Option<BatchFormat>,
    #[schemars(description = "URL path glob patterns to include (e.g. [\"/docs/**\"])")]
    pub include_glob: Option<Vec<String>>,
    #[schemars(description = "URL path glob patterns to exclude (e.g. [\"/archive/**\"])")]
    pub exclude_glob: Option<Vec<String>>,
    #[schemars(description = "Max characters per page result. Default: 5000")]
    pub max_length: Option<usize>,
    #[schemars(description = "Page load timeout in seconds per page. Default: 30")]
    pub timeout: Option<u64>,
    #[schemars(description = "Extra wait in ms after load event per page. Default: 0. Max: 10000.")]
    pub settle_ms: Option<u64>,
    #[schemars(description = "CSS selector to extract a specific section per page")]
    pub selector: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub(super) struct MapParams {
    #[schemars(description = "URL to discover links from (http/https only)")]
    pub url: String,
    #[schemars(description = "Maximum URLs to discover. Default: 5000. Max: 100000.")]
    pub limit: Option<usize>,
    #[schemars(description = "URL path glob patterns to include (e.g. [\"/docs/**\"])")]
    pub include_glob: Option<Vec<String>>,
    #[schemars(description = "URL path glob patterns to exclude (e.g. [\"/archive/**\"])")]
    pub exclude_glob: Option<Vec<String>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_format_deserializes() {
        assert!(matches!(
            serde_json::from_str::<OutputFormat>("\"markdown\"").unwrap(),
            OutputFormat::Markdown
        ));
        assert!(matches!(
            serde_json::from_str::<OutputFormat>("\"json\"").unwrap(),
            OutputFormat::Json
        ));
        assert!(matches!(
            serde_json::from_str::<OutputFormat>("\"accessibility_tree\"").unwrap(),
            OutputFormat::AccessibilityTree
        ));
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
        assert!(serde_json::from_str::<FetchParams>("{}").is_err());
    }

    #[test]
    fn fetch_params_accepts_minimal() {
        let p: FetchParams = serde_json::from_str(r#"{"url":"https://example.com"}"#).unwrap();
        assert_eq!(p.url, "https://example.com");
        assert!(p.format.is_none());
    }

    #[test]
    fn execute_js_params_requires_both() {
        assert!(serde_json::from_str::<ExecuteJsParams>(r#"{"url":"https://example.com"}"#).is_err());
        assert!(serde_json::from_str::<ExecuteJsParams>(r#"{"expression":"1+1"}"#).is_err());
    }

    #[test]
    fn batch_fetch_params_requires_urls() {
        assert!(serde_json::from_str::<BatchFetchParams>("{}").is_err());
    }

    #[test]
    fn batch_format_deserializes() {
        assert!(matches!(
            serde_json::from_str::<BatchFormat>("\"markdown\"").unwrap(),
            BatchFormat::Markdown
        ));
        assert!(matches!(
            serde_json::from_str::<BatchFormat>("\"json\"").unwrap(),
            BatchFormat::Json
        ));
    }
}
