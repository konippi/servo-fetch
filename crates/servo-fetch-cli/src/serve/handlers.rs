//! HTTP handlers — thin wrappers over [`crate::tools`] with validation and
//! response shaping.

use axum::Json as AxumJson;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::IntoResponse;

use crate::tools;

use super::error::ApiError;
use super::extract::Json;
use super::params::{
    BatchFetchRequest, BatchFetchResponse, BatchFormat, CrawlRequest, CrawlResponse, ExecuteJsRequest,
    ExecuteJsResponse, FetchRequest, FetchResponse, Format, HealthResponse, MapRequest, MapResponse, ScreenshotRequest,
    VersionResponse,
};

const MAX_TIMEOUT_SECS: u64 = 300;
const MAX_SETTLE_MS: u64 = 10_000;
const MAX_SELECTOR_LEN: usize = 1_000;
const MAX_JS_LEN: usize = 10_000;
const MAX_JS_OUTPUT_LEN: usize = 1_000_000;
const MAX_BATCH_URLS: usize = 20;
const MAX_CRAWL_LIMIT: usize = 500;
const MAX_CRAWL_DEPTH: usize = 10;
const MAX_MAP_LIMIT: usize = 100_000;

pub(super) async fn health() -> AxumJson<HealthResponse> {
    AxumJson(HealthResponse { status: "ok" })
}

pub(super) async fn version() -> AxumJson<VersionResponse> {
    AxumJson(VersionResponse {
        name: "servo-fetch",
        version: env!("CARGO_PKG_VERSION"),
    })
}

pub(super) async fn fetch(Json(req): Json<FetchRequest>) -> Result<AxumJson<FetchResponse>, ApiError> {
    let url = tools::validated_url(&req.url)?;
    let timeout = req.timeout.unwrap_or(30).clamp(1, MAX_TIMEOUT_SECS);
    let settle_ms = req.settle_ms.unwrap_or(0).min(MAX_SETTLE_MS);
    let max_len = req.max_length.unwrap_or(5000);
    let start = req.start_index.unwrap_or(0);
    validate_selector(req.selector.as_deref())?;

    let page = tools::fetch_page(&url, timeout, settle_ms).await?;
    let full = match req.format {
        Format::Html => page.html,
        Format::Text => page.inner_text,
        Format::Json => tools::extract(&page, &url, true, req.selector.as_deref())?,
        Format::Markdown => tools::extract(&page, &url, false, req.selector.as_deref())?,
        Format::AccessibilityTree => page.accessibility_tree.unwrap_or_default(),
    };
    let content = tools::paginate(&servo_fetch::sanitize::sanitize(&full), start, max_len);
    Ok(AxumJson(FetchResponse { url, content }))
}

pub(super) async fn screenshot(Json(req): Json<ScreenshotRequest>) -> Result<impl IntoResponse, ApiError> {
    let url = tools::validated_url(&req.url)?;
    let timeout = req.timeout.unwrap_or(30).clamp(1, MAX_TIMEOUT_SECS);
    let settle_ms = req.settle_ms.unwrap_or(0).min(MAX_SETTLE_MS);
    let full_page = req.full_page.unwrap_or(false);

    let png = tools::take_screenshot(&url, timeout, settle_ms, full_page).await?;
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, HeaderValue::from_static("image/png")),
            (
                header::CONTENT_DISPOSITION,
                HeaderValue::from_static("attachment; filename=\"screenshot.png\""),
            ),
        ],
        png,
    ))
}

pub(super) async fn execute_js(Json(req): Json<ExecuteJsRequest>) -> Result<AxumJson<ExecuteJsResponse>, ApiError> {
    if req.expression.len() > MAX_JS_LEN {
        return Err(ApiError::bad_request(format!(
            "expression exceeds {MAX_JS_LEN} character limit"
        )));
    }
    let url = tools::validated_url(&req.url)?;
    let timeout = req.timeout.unwrap_or(30).clamp(1, MAX_TIMEOUT_SECS);
    let settle_ms = req.settle_ms.unwrap_or(0).min(MAX_SETTLE_MS);

    let page = tools::fetch_js(&url, &req.expression, timeout, settle_ms).await?;
    let mut result = page.js_result.unwrap_or_default();
    if result.len() > MAX_JS_OUTPUT_LEN {
        result.truncate(servo_fetch::sanitize::floor_char_boundary(&result, MAX_JS_OUTPUT_LEN));
        result.push_str("\n<output truncated>");
    }
    let console = page
        .console_messages
        .iter()
        .map(|m| format!("[{:?}] {}", m.level, m.message))
        .collect();
    Ok(AxumJson(ExecuteJsResponse {
        url,
        result: servo_fetch::sanitize::sanitize(&result).into_owned(),
        console,
    }))
}

pub(super) async fn batch_fetch(Json(req): Json<BatchFetchRequest>) -> Result<AxumJson<BatchFetchResponse>, ApiError> {
    if req.urls.is_empty() {
        return Err(ApiError::bad_request("urls must not be empty"));
    }
    if req.urls.len() > MAX_BATCH_URLS {
        return Err(ApiError::bad_request(format!(
            "urls exceeds {MAX_BATCH_URLS} URL limit"
        )));
    }
    validate_selector(req.selector.as_deref())?;

    let timeout = req.timeout.unwrap_or(30).clamp(1, MAX_TIMEOUT_SECS);
    let settle_ms = req.settle_ms.unwrap_or(0).min(MAX_SETTLE_MS);
    let max_len = req.max_length.unwrap_or(5000);
    let json = matches!(req.format, BatchFormat::Json);

    let validated: Vec<String> = req
        .urls
        .iter()
        .map(|u| tools::validated_url(u))
        .collect::<Result<_, _>>()?;
    let raw = tools::batch_fetch_pages(&validated, timeout, settle_ms, json, req.selector.as_deref(), max_len).await;
    let results = raw
        .into_iter()
        .map(|(url, content)| FetchResponse { url, content })
        .collect();
    Ok(AxumJson(BatchFetchResponse { results }))
}

pub(super) async fn crawl(Json(req): Json<CrawlRequest>) -> Result<AxumJson<CrawlResponse>, ApiError> {
    let url = tools::validated_url(&req.url)?;
    let limit = req.limit.unwrap_or(20).clamp(1, MAX_CRAWL_LIMIT);
    let max_depth = req.max_depth.unwrap_or(3).clamp(1, MAX_CRAWL_DEPTH);
    let timeout = req.timeout.unwrap_or(30).clamp(1, MAX_TIMEOUT_SECS);
    let settle_ms = req.settle_ms.unwrap_or(0).min(MAX_SETTLE_MS);
    let max_len = req.max_length.unwrap_or(5000);
    let json = matches!(req.format, BatchFormat::Json);
    validate_selector(req.selector.as_deref())?;

    let pages = tools::crawl_pages(tools::CrawlOptions {
        url: &url,
        limit,
        max_depth,
        json,
        selector: req.selector.as_deref(),
        max_len,
        timeout,
        settle_ms,
        include_glob: req.include_glob.as_deref(),
        exclude_glob: req.exclude_glob.as_deref(),
    })
    .await?;
    let results = pages
        .into_iter()
        .map(|(url, content)| FetchResponse { url, content })
        .collect();
    Ok(AxumJson(CrawlResponse { results }))
}

pub(super) async fn map(Json(req): Json<MapRequest>) -> Result<AxumJson<MapResponse>, ApiError> {
    let url = tools::validated_url(&req.url)?;
    let limit = req.limit.unwrap_or(5000).clamp(1, MAX_MAP_LIMIT);

    let urls = tools::discover_urls(
        &url,
        limit,
        req.include_glob.as_deref().unwrap_or_default(),
        req.exclude_glob.as_deref().unwrap_or_default(),
    )
    .await?;
    Ok(AxumJson(MapResponse { urls }))
}

fn validate_selector(selector: Option<&str>) -> Result<(), ApiError> {
    if selector.is_some_and(|s| s.len() > MAX_SELECTOR_LEN) {
        return Err(ApiError::bad_request(format!(
            "selector exceeds {MAX_SELECTOR_LEN} character limit"
        )));
    }
    Ok(())
}
