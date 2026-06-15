//! HTTP handlers — thin wrappers over [`crate::tools`] with validation and
//! response shaping.

use axum::Json as AxumJson;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::IntoResponse;
use servo_fetch::FetchOptions;
use servo_fetch_types::{
    BatchFetchRequest, CrawlRequest, EvaluateRequest, FetchRequest, MapRequest, ScreenshotRequest,
};

use super::error::ApiError;
use super::extract::Json;
use super::response::{BatchFetchResponse, CrawlResponse, FetchResponse, HealthResponse, MapResponse, VersionResponse};
use crate::tools::limits::{DEFAULT_MAX_LENGTH, MAX_BATCH_URLS, MAX_JS_LEN, to_len};
use crate::tools::{self, BatchSpec, CrawlSpec, ToolError};

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
    tools::validate_selector(req.selector.as_deref())?;

    let opts = FetchOptions::new(&url).visibility(tools::visibility_policy(req.visibility));
    let page = tools::fetch_with(tools::apply_options(opts, req.options)?).await?;
    let full = tools::render_page(&page, &url, req.format.unwrap_or_default(), req.selector.as_deref())?;
    let content = tools::paginate(
        &servo_fetch::sanitize::sanitize(&full),
        to_len(req.start_index, 0),
        to_len(req.max_length, DEFAULT_MAX_LENGTH),
    );
    Ok(AxumJson(FetchResponse { url, content }))
}

pub(super) async fn screenshot(Json(req): Json<ScreenshotRequest>) -> Result<impl IntoResponse, ApiError> {
    let url = tools::validated_url(&req.url)?;

    let opts = FetchOptions::screenshot(&url, req.full_page.unwrap_or(false));
    let page = tools::fetch_with(tools::apply_options(opts, req.options)?).await?;
    let png = page
        .screenshot_png()
        .map(<[u8]>::to_vec)
        .ok_or_else(|| ToolError::internal("screenshot capture failed"))?;
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

pub(super) async fn execute_js(
    Json(req): Json<EvaluateRequest>,
) -> Result<AxumJson<servo_fetch_types::EvaluateResult>, ApiError> {
    if req.expression.len() > MAX_JS_LEN {
        return Err(ApiError::bad_request(format!(
            "expression exceeds {MAX_JS_LEN} character limit"
        )));
    }
    let url = tools::validated_url(&req.url)?;

    let opts = FetchOptions::javascript(&url, &req.expression);
    let page = tools::fetch_with(tools::apply_options(opts, req.options)?).await?;
    let result = tools::clamp_js_output(page.js_result.unwrap_or_default());
    let result = servo_fetch::sanitize::sanitize(&result).into_owned();
    Ok(AxumJson(crate::wire::evaluate_result(
        url,
        result,
        &page.console_messages,
    )))
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
    tools::validate_selector(req.selector.as_deref())?;

    let validated: Vec<String> = req
        .urls
        .iter()
        .map(|u| tools::validated_url(u))
        .collect::<Result<_, _>>()?;
    let raw = tools::batch_fetch_pages(BatchSpec {
        urls: &validated,
        format: req.format.unwrap_or_default(),
        selector: req.selector.as_deref(),
        max_len: to_len(req.max_length, DEFAULT_MAX_LENGTH),
        visibility: tools::visibility_policy(req.visibility),
        options: req.options,
    })
    .await;
    let results = raw
        .into_iter()
        .map(|(url, content)| FetchResponse { url, content })
        .collect();
    Ok(AxumJson(BatchFetchResponse { results }))
}

pub(super) async fn crawl(Json(req): Json<CrawlRequest>) -> Result<AxumJson<CrawlResponse>, ApiError> {
    let url = tools::validated_url(&req.url)?;
    tools::validate_selector(req.selector.as_deref())?;

    let pages = tools::crawl_pages(
        CrawlSpec {
            url: &url,
            limit: req.limit,
            max_depth: req.max_depth,
            format: req.format.unwrap_or_default(),
            selector: req.selector.as_deref(),
            include: req.include.as_deref(),
            exclude: req.exclude.as_deref(),
            concurrency: req.concurrency,
            delay_ms: req.delay_ms,
            options: req.options,
        },
        to_len(req.max_length, DEFAULT_MAX_LENGTH),
    )
    .await?;
    let results = pages
        .into_iter()
        .map(|(url, content)| FetchResponse { url, content })
        .collect();
    Ok(AxumJson(CrawlResponse { results }))
}

pub(super) async fn map(Json(req): Json<MapRequest>) -> Result<AxumJson<MapResponse>, ApiError> {
    let url = tools::validated_url(&req.url)?;

    let opts = tools::build_map_options(tools::MapSpec {
        url: &url,
        limit: req.limit,
        include: req.include.as_deref(),
        exclude: req.exclude.as_deref(),
        no_fallback: req.no_fallback.unwrap_or(false),
        user_agent: req.user_agent.as_deref(),
        timeout: req.timeout,
        headers: req.headers,
    })?;
    let urls = tools::map_with(opts)
        .await?
        .into_iter()
        .map(|entry| entry.url)
        .collect();
    Ok(AxumJson(MapResponse { urls }))
}
