//! Request and response types for the HTTP API.

use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum Format {
    #[default]
    Markdown,
    Json,
    Html,
    Text,
    #[serde(rename = "accessibility_tree")]
    AccessibilityTree,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum BatchFormat {
    #[default]
    Markdown,
    Json,
}

#[derive(Debug, Deserialize)]
pub(super) struct FetchRequest {
    pub url: String,
    #[serde(default)]
    pub format: Format,
    pub max_length: Option<usize>,
    pub start_index: Option<usize>,
    pub timeout: Option<u64>,
    pub settle_ms: Option<u64>,
    pub selector: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ScreenshotRequest {
    pub url: String,
    pub full_page: Option<bool>,
    pub timeout: Option<u64>,
    pub settle_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ExecuteJsRequest {
    pub url: String,
    pub expression: String,
    pub timeout: Option<u64>,
    pub settle_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub(super) struct BatchFetchRequest {
    pub urls: Vec<String>,
    #[serde(default)]
    pub format: BatchFormat,
    pub max_length: Option<usize>,
    pub timeout: Option<u64>,
    pub settle_ms: Option<u64>,
    pub selector: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct CrawlRequest {
    pub url: String,
    pub limit: Option<usize>,
    pub max_depth: Option<usize>,
    #[serde(default)]
    pub format: BatchFormat,
    pub include_glob: Option<Vec<String>>,
    pub exclude_glob: Option<Vec<String>>,
    pub max_length: Option<usize>,
    pub timeout: Option<u64>,
    pub settle_ms: Option<u64>,
    pub selector: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct MapRequest {
    pub url: String,
    pub limit: Option<usize>,
    pub include_glob: Option<Vec<String>>,
    pub exclude_glob: Option<Vec<String>>,
}

#[derive(Debug, Serialize)]
pub(super) struct FetchResponse {
    pub url: String,
    pub content: String,
}

#[derive(Debug, Serialize)]
pub(super) struct BatchFetchResponse {
    pub results: Vec<FetchResponse>,
}

#[derive(Debug, Serialize)]
pub(super) struct ExecuteJsResponse {
    pub url: String,
    pub result: String,
    pub console: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(super) struct CrawlResponse {
    pub results: Vec<FetchResponse>,
}

#[derive(Debug, Serialize)]
pub(super) struct MapResponse {
    pub urls: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(super) struct HealthResponse {
    pub status: &'static str,
}

#[derive(Debug, Serialize)]
pub(super) struct VersionResponse {
    pub name: &'static str,
    pub version: &'static str,
}
