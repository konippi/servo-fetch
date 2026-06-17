//! JSON-RPC method handlers — request DTO in, typed wire result out.

use std::time::Instant;

use base64::Engine as _;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use servo_fetch::FetchOptions;
use servo_fetch_types::{
    CrawlRequest, CrawlStats, ErrorKind, EvaluateRequest, ExtractRequest, FetchRequest, InitializeResult, MapRequest,
    SchemaExtractRequest, ScreenshotRequest, ServerCapabilities, ServerInfo,
};
use tokio::sync::mpsc::UnboundedSender;

use super::protocol::{Notification, PROTOCOL_VERSION, RequestId, ResponseError, code};
use crate::tools::limits::MAX_JS_LEN;
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
    tools::validate_selector(req.selector.as_deref())?;

    let format = req.format.unwrap_or_default();
    let opts = tools::content_options(&url, format, tools::visibility_policy(req.visibility));
    let page = tools::fetch_with(tools::apply_options(opts, req.options)?).await?;

    let full = tools::render_page(&page, &url, format, req.selector.as_deref())?;
    let sanitized = servo_fetch::sanitize::sanitize(&full);
    let content = tools::paginate_opt(&sanitized, req.start_index, req.max_length);
    Ok(json!(content))
}

async fn extract(params: Value) -> Result<Value, ResponseError> {
    let req: ExtractRequest = parse(params)?;
    let url = tools::validated_url(&req.url)?;
    tools::validate_selector(req.selector.as_deref())?;

    let opts = FetchOptions::new(&url).visibility(tools::visibility_policy(req.visibility));
    let page = tools::fetch_with(tools::apply_options(opts, req.options)?).await?;

    let data = match req.selector.as_deref() {
        Some(s) => page.article_with_selector(&url, s),
        None => page.article(&url),
    }?;
    Ok(serde_json::to_value(crate::wire::article(data))?)
}

async fn screenshot(params: Value) -> Result<Value, ResponseError> {
    let req: ScreenshotRequest = parse(params)?;
    let url = tools::validated_url(&req.url)?;

    let opts = FetchOptions::screenshot(&url, req.full_page.unwrap_or(false));
    let page = tools::fetch_with(tools::apply_options(opts, req.options)?).await?;

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

    let opts = FetchOptions::javascript(&url, &req.expression);
    let page = tools::fetch_with(tools::apply_options(opts, req.options)?).await?;

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
        .visibility(tools::visibility_policy(req.visibility))
        .schema(schema);
    let page = tools::fetch_with(tools::apply_options(opts, req.options)?).await?;

    let extracted = page.extracted.unwrap_or(Value::Null);
    Ok(serde_json::to_value(crate::wire::schema_extract(&url, extracted))?)
}

async fn map(params: Value) -> Result<Value, ResponseError> {
    let req: MapRequest = parse(params)?;
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
    let entries = tools::map_with(opts).await?;

    let mapped: Vec<_> = entries.iter().map(crate::wire::mapped_url).collect();
    Ok(serde_json::to_value(mapped)?)
}

async fn crawl(params: Value, id: &RequestId, tx: &UnboundedSender<String>) -> Result<Value, ResponseError> {
    let req: CrawlRequest = parse(params)?;
    let url = tools::validated_url(&req.url)?;
    tools::validate_selector(req.selector.as_deref())?;

    let opts = tools::build_crawl_options(&tools::CrawlSpec {
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
    })?;

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

fn parse<T: DeserializeOwned>(params: Value) -> Result<T, ResponseError> {
    serde_json::from_value(params).map_err(|e| ResponseError::invalid_params(e.to_string()))
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
