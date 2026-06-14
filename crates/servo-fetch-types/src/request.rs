//! Request DTOs — the input counterpart to the output wire types, shared by the
//! servo-fetch servers and language bindings.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::Visibility;

/// Output format for the `fetch` method (all string-valued).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(ts_rs::TS), ts(export, export_to = "index.ts"))]
#[serde(rename_all = "lowercase")]
pub enum FetchFormat {
    /// Readability-extracted Markdown.
    #[default]
    Markdown,
    /// Raw rendered HTML (post-JS execution).
    Html,
    /// Plain text (`document.body.innerText`).
    Text,
}

/// Parameters for the `fetch` method (Markdown/HTML/text — returns a string).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(ts_rs::TS), ts(export, export_to = "index.ts"))]
#[serde(rename_all = "camelCase")]
pub struct FetchRequest {
    /// URL to fetch (http/https only).
    pub url: String,
    /// Output format (default: markdown).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional))]
    pub format: Option<FetchFormat>,
    /// CSS selector to extract a specific section (Markdown only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional))]
    pub selector: Option<String>,
    /// Page-load timeout in seconds (default: 30).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional, type = "number"))]
    pub timeout: Option<u64>,
    /// Extra wait in milliseconds after the load event (default: 0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional, type = "number"))]
    pub settle_ms: Option<u64>,
    /// Override the User-Agent header.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional))]
    pub user_agent: Option<String>,
    /// Path to a Netscape-format cookies.txt file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional))]
    pub cookies_file: Option<String>,
    /// Custom request headers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional))]
    pub headers: Option<BTreeMap<String, String>>,
    /// Visibility filtering policy (default: moderate).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional))]
    pub visibility: Option<Visibility>,
}

/// Parameters for the `extract` method (Readability article JSON).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(ts_rs::TS), ts(export, export_to = "index.ts"))]
#[serde(rename_all = "camelCase")]
pub struct ExtractRequest {
    /// URL to fetch (http/https only).
    pub url: String,
    /// CSS selector to extract a specific section.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional))]
    pub selector: Option<String>,
    /// Page-load timeout in seconds (default: 30).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional, type = "number"))]
    pub timeout: Option<u64>,
    /// Extra wait in milliseconds after the load event (default: 0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional, type = "number"))]
    pub settle_ms: Option<u64>,
    /// Override the User-Agent header.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional))]
    pub user_agent: Option<String>,
    /// Path to a Netscape-format cookies.txt file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional))]
    pub cookies_file: Option<String>,
    /// Custom request headers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional))]
    pub headers: Option<BTreeMap<String, String>>,
    /// Visibility filtering policy (default: moderate).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional))]
    pub visibility: Option<Visibility>,
}

/// Parameters for the `screenshot` method.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(ts_rs::TS), ts(export, export_to = "index.ts"))]
#[serde(rename_all = "camelCase")]
pub struct ScreenshotRequest {
    /// URL to capture (http/https only).
    pub url: String,
    /// Capture the full scrollable page instead of just the viewport.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional))]
    pub full_page: Option<bool>,
    /// Page-load timeout in seconds (default: 30).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional, type = "number"))]
    pub timeout: Option<u64>,
    /// Extra wait in milliseconds after the load event (default: 0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional, type = "number"))]
    pub settle_ms: Option<u64>,
    /// Override the User-Agent header.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional))]
    pub user_agent: Option<String>,
    /// Path to a Netscape-format cookies.txt file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional))]
    pub cookies_file: Option<String>,
    /// Custom request headers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional))]
    pub headers: Option<BTreeMap<String, String>>,
}

/// Parameters for the `evaluate` method.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(ts_rs::TS), ts(export, export_to = "index.ts"))]
#[serde(rename_all = "camelCase")]
pub struct EvaluateRequest {
    /// URL to load before evaluating.
    pub url: String,
    /// JavaScript expression to evaluate.
    pub expression: String,
    /// Page-load timeout in seconds (default: 30).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional, type = "number"))]
    pub timeout: Option<u64>,
    /// Extra wait in milliseconds after the load event (default: 0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional, type = "number"))]
    pub settle_ms: Option<u64>,
    /// Override the User-Agent header.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional))]
    pub user_agent: Option<String>,
    /// Path to a Netscape-format cookies.txt file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional))]
    pub cookies_file: Option<String>,
    /// Custom request headers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional))]
    pub headers: Option<BTreeMap<String, String>>,
}

/// Parameters for the `extractSchema` method.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(ts_rs::TS), ts(export, export_to = "index.ts"))]
#[serde(rename_all = "camelCase")]
pub struct SchemaExtractRequest {
    /// URL to extract from (http/https only).
    pub url: String,
    /// CSS-selector extraction schema.
    #[cfg_attr(feature = "codegen", ts(type = "unknown"))]
    pub schema: serde_json::Value,
    /// Page-load timeout in seconds (default: 30).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional, type = "number"))]
    pub timeout: Option<u64>,
    /// Extra wait in milliseconds after the load event (default: 0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional, type = "number"))]
    pub settle_ms: Option<u64>,
    /// Override the User-Agent header.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional))]
    pub user_agent: Option<String>,
    /// Path to a Netscape-format cookies.txt file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional))]
    pub cookies_file: Option<String>,
    /// Custom request headers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional))]
    pub headers: Option<BTreeMap<String, String>>,
    /// Visibility filtering policy (default: moderate).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional))]
    pub visibility: Option<Visibility>,
}

/// Parameters for the `crawl` method.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(ts_rs::TS), ts(export, export_to = "index.ts"))]
#[serde(rename_all = "camelCase")]
pub struct CrawlRequest {
    /// Seed URL to crawl (http/https only).
    pub url: String,
    /// Maximum number of pages to crawl (default: 50).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional, type = "number"))]
    pub limit: Option<u64>,
    /// Maximum link depth from the seed URL (default: 3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional, type = "number"))]
    pub max_depth: Option<u64>,
    /// URL path glob patterns to include.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional))]
    pub include: Option<Vec<String>>,
    /// URL path glob patterns to exclude.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional))]
    pub exclude: Option<Vec<String>>,
    /// Maximum parallel page fetches (default: 1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional, type = "number"))]
    pub concurrency: Option<u64>,
    /// Minimum dispatch interval in milliseconds (default: 500; 0 disables).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional, type = "number"))]
    pub delay_ms: Option<u64>,
    /// CSS selector to extract a specific section per page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional))]
    pub selector: Option<String>,
    /// Per-page load timeout in seconds (default: 30).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional, type = "number"))]
    pub timeout: Option<u64>,
    /// Extra wait in milliseconds after the load event, per page (default: 0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional, type = "number"))]
    pub settle_ms: Option<u64>,
    /// Override the User-Agent header.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional))]
    pub user_agent: Option<String>,
    /// Path to a Netscape-format cookies.txt file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional))]
    pub cookies_file: Option<String>,
    /// Custom request headers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional))]
    pub headers: Option<BTreeMap<String, String>>,
}

/// Parameters for the `map` method.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(ts_rs::TS), ts(export, export_to = "index.ts"))]
#[serde(rename_all = "camelCase")]
pub struct MapRequest {
    /// URL to discover links from (http/https only).
    pub url: String,
    /// Maximum number of URLs to discover (default: 5000).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional, type = "number"))]
    pub limit: Option<u64>,
    /// URL path glob patterns to include.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional))]
    pub include: Option<Vec<String>>,
    /// URL path glob patterns to exclude.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional))]
    pub exclude: Option<Vec<String>>,
    /// Override the User-Agent header.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional))]
    pub user_agent: Option<String>,
    /// Per-request timeout in seconds (default: 30).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional, type = "number"))]
    pub timeout: Option<u64>,
    /// Skip the HTML link fallback when no sitemap is found.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional))]
    pub no_fallback: Option<bool>,
    /// Custom request headers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional))]
    pub headers: Option<BTreeMap<String, String>>,
}
