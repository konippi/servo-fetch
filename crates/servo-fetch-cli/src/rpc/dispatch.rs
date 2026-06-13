//! JSON-RPC method handlers — request DTO in, typed wire result out.

use std::time::{Duration, Instant};

use base64::Engine as _;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use servo_fetch::{FetchOptions, MapOptions, VisibilityPolicy};
use servo_fetch_types::{
    CrawlRequest, CrawlStats, ErrorKind, EvaluateRequest, ExtractRequest, FetchFormat, FetchRequest, InitializeResult,
    MapRequest, SchemaExtractRequest, ScreenshotRequest, ServerCapabilities, ServerInfo, Visibility,
};
use tokio::sync::mpsc::UnboundedSender;

use super::protocol::{Notification, PROTOCOL_VERSION, RequestId, ResponseError, code};
use crate::tools::limits::{
    MAX_CRAWL_CONCURRENCY, MAX_CRAWL_DEPTH, MAX_CRAWL_PAGES, MAX_JS_LEN, MAX_MAP_URLS, MAX_SELECTOR_LEN, MAX_SETTLE_MS,
    MAX_TIMEOUT_SECS,
};
use crate::tools::{self, ToolError};

/// Route a request to its handler (`id`/`tx` drive `crawl` progress streaming).
pub(crate) async fn dispatch(
    method: &str,
    params: Value,
    id: &RequestId,
    tx: &UnboundedSender<String>,
) -> Result<Value, ResponseError> {
    match method {
        "initialize" => Ok(initialize()),
        "fetch" => fetch(params).await,
        "extract" => extract(params).await,
        "screenshot" => screenshot(params).await,
        "evaluate" => evaluate(params).await,
        "extractSchema" => extract_schema(params).await,
        "map" => map(params).await,
        "crawl" => crawl(params, id, tx).await,
        other => Err(ResponseError::method_not_found(other)),
    }
}

fn initialize() -> Value {
    serde_json::to_value(InitializeResult {
        protocol_version: PROTOCOL_VERSION,
        server_info: ServerInfo {
            name: "servo-fetch".to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
        },
        capabilities: ServerCapabilities::default(),
    })
    .expect("InitializeResult is always serializable")
}

async fn fetch(params: Value) -> Result<Value, ResponseError> {
    let req: FetchRequest = parse(params)?;
    let url = tools::validated_url(&req.url)?;
    validate_selector(req.selector.as_deref())?;

    let opts = FetchOptions::new(&url)
        .timed(req.timeout, req.settle_ms)
        .visibility(visibility_policy(req.visibility));
    let page = tools::fetch_with(with_ua_and_cookies(opts, req.user_agent, req.cookies_file)?).await?;

    match req.format.unwrap_or_default() {
        FetchFormat::Html => Ok(json!(page.html)),
        FetchFormat::Text => Ok(json!(page.inner_text)),
        FetchFormat::Markdown => {
            let md = match req.selector.as_deref() {
                Some(s) => page.markdown_with_selector(&url, s),
                None => page.markdown_with_url(&url),
            }?;
            Ok(json!(md))
        }
    }
}

async fn extract(params: Value) -> Result<Value, ResponseError> {
    let req: ExtractRequest = parse(params)?;
    let url = tools::validated_url(&req.url)?;
    validate_selector(req.selector.as_deref())?;

    let opts = FetchOptions::new(&url)
        .timed(req.timeout, req.settle_ms)
        .visibility(visibility_policy(req.visibility));
    let page = tools::fetch_with(with_ua_and_cookies(opts, req.user_agent, req.cookies_file)?).await?;

    let data = match req.selector.as_deref() {
        Some(s) => page.article_with_selector(&url, s),
        None => page.article(&url),
    }?;
    Ok(serde_json::to_value(crate::wire::article(data))?)
}

async fn screenshot(params: Value) -> Result<Value, ResponseError> {
    let req: ScreenshotRequest = parse(params)?;
    let url = tools::validated_url(&req.url)?;

    let opts = FetchOptions::screenshot(&url, req.full_page.unwrap_or(false)).timed(req.timeout, req.settle_ms);
    let page = tools::fetch_with(with_ua_and_cookies(opts, req.user_agent, req.cookies_file)?).await?;

    let png = page
        .screenshot_png()
        .ok_or_else(|| ResponseError::new(code::INTERNAL_ERROR, ErrorKind::Internal, "screenshot capture failed"))?;
    Ok(json!(base64::engine::general_purpose::STANDARD.encode(png)))
}

async fn evaluate(params: Value) -> Result<Value, ResponseError> {
    let req: EvaluateRequest = parse(params)?;
    if req.expression.len() > MAX_JS_LEN {
        return Err(ResponseError::invalid_params(format!(
            "expression exceeds {MAX_JS_LEN} character limit"
        )));
    }
    let url = tools::validated_url(&req.url)?;

    let opts = FetchOptions::javascript(&url, &req.expression).timed(req.timeout, req.settle_ms);
    let page = tools::fetch_with(with_ua_and_cookies(opts, req.user_agent, req.cookies_file)?).await?;

    let result = page.js_result.unwrap_or_default();
    Ok(serde_json::to_value(crate::wire::evaluate_result(
        url,
        result,
        &page.console_messages,
    ))?)
}

async fn extract_schema(params: Value) -> Result<Value, ResponseError> {
    let req: SchemaExtractRequest = parse(params)?;
    let url = tools::validated_url(&req.url)?;
    let schema = servo_fetch::schema::ExtractSchema::from_json(&req.schema.to_string())
        .map_err(|e| ResponseError::invalid_params(format!("schema: {e}")))?;

    let opts = FetchOptions::new(&url)
        .timed(req.timeout, req.settle_ms)
        .visibility(visibility_policy(req.visibility))
        .schema(schema);
    let page = tools::fetch_with(with_ua_and_cookies(opts, req.user_agent, req.cookies_file)?).await?;

    let extracted = page.extracted.unwrap_or(Value::Null);
    Ok(serde_json::to_value(crate::wire::schema_extract(&url, extracted))?)
}

async fn map(params: Value) -> Result<Value, ResponseError> {
    let req: MapRequest = parse(params)?;
    let url = tools::validated_url(&req.url)?;
    let limit = clamp_usize(req.limit, 5000, MAX_MAP_URLS);

    let mut opts = MapOptions::new(&url).limit(limit);
    if let Some(include) = req.include.as_deref().filter(|v| !v.is_empty()) {
        opts = opts.include(&glob_refs(include));
    }
    if let Some(exclude) = req.exclude.as_deref().filter(|v| !v.is_empty()) {
        opts = opts.exclude(&glob_refs(exclude));
    }
    if let Some(ua) = req.user_agent.as_deref() {
        opts = opts.user_agent(ua);
    }
    if let Some(secs) = req.timeout {
        opts = opts.timeout(secs);
    }
    if req.no_fallback.unwrap_or(false) {
        opts = opts.no_fallback(true);
    }
    let entries = tools::map_with(opts).await?;

    let mapped: Vec<_> = entries.iter().map(crate::wire::mapped_url).collect();
    Ok(serde_json::to_value(mapped)?)
}

async fn crawl(params: Value, id: &RequestId, tx: &UnboundedSender<String>) -> Result<Value, ResponseError> {
    let req: CrawlRequest = parse(params)?;
    let url = tools::validated_url(&req.url)?;
    validate_selector(req.selector.as_deref())?;

    let mut opts = servo_fetch::CrawlOptions::new(&url)
        .limit(clamp_usize(req.limit, 50, MAX_CRAWL_PAGES))
        .max_depth(clamp_usize(req.max_depth, 3, MAX_CRAWL_DEPTH))
        .timeout(Duration::from_secs(timeout_secs(req.timeout)))
        .settle(Duration::from_millis(settle_ms(req.settle_ms)))
        .concurrency(clamp_usize(req.concurrency, 1, MAX_CRAWL_CONCURRENCY))
        .delay(match req.delay_ms {
            Some(0) => None,
            Some(ms) => Some(Duration::from_millis(ms)),
            None => Some(Duration::from_millis(500)),
        });
    if let Some(s) = req.selector.as_deref() {
        opts = opts.selector(s);
    }
    if let Some(globs) = req.include.as_deref().filter(|g| !g.is_empty()) {
        opts = opts.include(&glob_refs(globs));
    }
    if let Some(globs) = req.exclude.as_deref().filter(|g| !g.is_empty()) {
        opts = opts.exclude(&glob_refs(globs));
    }
    if let Some(ua) = req.user_agent {
        opts = opts.user_agent(ua);
    }
    if let Some(path) = req.cookies_file {
        opts = opts.cookies(load_cookies(&path)?);
    }

    // Stream each page as a `$/progress` notification. Buffering is bounded by
    // the crawl `limit`; the run loop cancels by dropping this future.
    let started = Instant::now();
    let mut crawled = 0u64;
    let mut errors = 0u64;
    {
        let crawled = &mut crawled;
        let errors = &mut errors;
        let tx = tx.clone();
        let id = id.clone();
        servo_fetch::crawl_each(&opts, move |result| {
            if result.outcome.is_ok() {
                *crawled += 1;
            } else {
                *errors += 1;
            }
            if let Ok(value) = serde_json::to_value(crate::wire::crawl_event(result)) {
                let note = Notification::new("$/progress", json!({ "token": &id, "value": value }));
                if let Ok(line) = serde_json::to_string(&note) {
                    let _ = tx.send(line);
                }
            }
        })
        .await
        .map_err(ResponseError::from)?;
    }

    let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    Ok(serde_json::to_value(CrawlStats {
        crawled,
        errors,
        elapsed_ms,
    })?)
}

trait FetchOptionsExt {
    fn timed(self, timeout: Option<u64>, settle: Option<u64>) -> Self;
}

impl FetchOptionsExt for FetchOptions {
    fn timed(self, timeout: Option<u64>, settle: Option<u64>) -> Self {
        self.timeout(Duration::from_secs(timeout_secs(timeout)))
            .settle(Duration::from_millis(settle_ms(settle)))
    }
}

fn glob_refs(globs: &[String]) -> Vec<&str> {
    globs.iter().map(String::as_str).collect()
}

fn with_ua_and_cookies(
    mut opts: FetchOptions,
    user_agent: Option<String>,
    cookies_file: Option<String>,
) -> Result<FetchOptions, ResponseError> {
    if let Some(ua) = user_agent {
        opts = opts.user_agent(ua);
    }
    if let Some(path) = cookies_file {
        opts = opts.cookies(load_cookies(&path)?);
    }
    Ok(opts)
}

fn load_cookies(path: &str) -> Result<Vec<servo_fetch::CookieSpec>, ResponseError> {
    servo_fetch::load_cookies(path).map_err(|e| ResponseError::invalid_params(format!("cookies '{path}': {e}")))
}

fn visibility_policy(v: Option<Visibility>) -> VisibilityPolicy {
    match v {
        Some(Visibility::Strict) => VisibilityPolicy::strict(),
        Some(Visibility::Off) => VisibilityPolicy::off(),
        Some(Visibility::Moderate) | None => VisibilityPolicy::moderate(),
    }
}

fn validate_selector(selector: Option<&str>) -> Result<(), ResponseError> {
    if selector.is_some_and(|s| s.len() > MAX_SELECTOR_LEN) {
        return Err(ResponseError::invalid_params(format!(
            "selector exceeds {MAX_SELECTOR_LEN} character limit"
        )));
    }
    Ok(())
}

fn parse<T: DeserializeOwned>(params: Value) -> Result<T, ResponseError> {
    serde_json::from_value(params).map_err(|e| ResponseError::invalid_params(e.to_string()))
}

fn timeout_secs(timeout: Option<u64>) -> u64 {
    timeout.unwrap_or(30).clamp(1, MAX_TIMEOUT_SECS)
}

fn settle_ms(settle: Option<u64>) -> u64 {
    settle.unwrap_or(0).min(MAX_SETTLE_MS)
}

fn clamp_usize(value: Option<u64>, default: usize, max: usize) -> usize {
    value.map_or(default, |n| usize::try_from(n).unwrap_or(max).clamp(1, max))
}

impl From<ToolError> for ResponseError {
    fn from(err: ToolError) -> Self {
        let code = match err.kind() {
            ErrorKind::InvalidUrl | ErrorKind::AddressNotAllowed | ErrorKind::InvalidParams => code::INVALID_PARAMS,
            ErrorKind::Internal => code::INTERNAL_ERROR,
            _ => code::FETCH_ERROR,
        };
        Self::new(code, err.kind(), err.to_string())
    }
}

impl From<servo_fetch::Error> for ResponseError {
    fn from(err: servo_fetch::Error) -> Self {
        Self::from(ToolError::from(err))
    }
}

impl From<serde_json::Error> for ResponseError {
    fn from(err: serde_json::Error) -> Self {
        Self::new(code::INTERNAL_ERROR, ErrorKind::Internal, err.to_string())
    }
}
