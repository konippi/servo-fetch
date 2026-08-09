//! Serializable request, result, progress, and error types for isolated sessions.

use std::io::Write;
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::{MAX_WORKER_FRAME_BYTES, worker_error};
use crate::error::{Error, Result};
use crate::fetch::{ConsoleLevel, ConsoleMessage, FetchMode, FetchOptions, Page};
use crate::{CrawlOptions, CrawlPage, CrawlResult, VisibilityPolicy};

const MAX_ERROR_KIND_BYTES: usize = 64;
const MAX_ERROR_MESSAGE_BYTES: usize = 8 * 1024;
const MAX_ERROR_URL_BYTES: usize = 64 * 1024;
const MAX_ERROR_HOST_BYTES: usize = 1024;

fn bounded_text(mut value: String, max: usize) -> String {
    const MARKER: &str = "…";
    if value.len() > max {
        let keep = max.saturating_sub(MARKER.len());
        value.truncate(crate::sanitize::floor_char_boundary(&value, keep));
        value.push_str(MARKER);
    }
    value
}

fn error_kind(error: &Error) -> &'static str {
    match error {
        Error::Timeout { .. } => "timeout",
        Error::InvalidUrl { .. } => "invalid_url",
        Error::AddressNotAllowed { .. } => "address_not_allowed",
        Error::JavaScript { .. } => "javascript",
        Error::Screenshot { .. } => "screenshot",
        Error::OutputTooLarge { .. } => "output_too_large",
        Error::InvalidHeader(_) => "invalid_header",
        Error::InvalidGlob(_) => "invalid_glob",
        Error::Schema(_) => "schema",
        Error::Extract(_) => "extract",
        Error::Cookies { .. } => "cookies",
        Error::Engine { .. } => "engine",
        _ => "worker",
    }
}

#[derive(Serialize, Deserialize)]
pub(crate) struct WorkerErrorWire {
    kind: String,
    message: String,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    timeout_ms: Option<u64>,
    #[serde(default)]
    host: Option<String>,
    #[serde(default)]
    output_kind: Option<String>,
    #[serde(default)]
    size: Option<u64>,
    #[serde(default)]
    max: Option<u64>,
}

impl WorkerErrorWire {
    pub(crate) fn failure(kind: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            kind: bounded_text(kind.into(), MAX_ERROR_KIND_BYTES),
            message: bounded_text(message.into(), MAX_ERROR_MESSAGE_BYTES),
            url: None,
            timeout_ms: None,
            host: None,
            output_kind: None,
            size: None,
            max: None,
        }
    }

    pub(crate) fn from_error(error: &Error) -> Self {
        let (url, timeout_ms, host) = match error {
            Error::Timeout { url, timeout } => (
                Some(url.clone()),
                Some(u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX)),
                None,
            ),
            Error::InvalidUrl { url, .. } => (Some(url.clone()), None, None),
            Error::AddressNotAllowed { host } => (None, None, Some(host.clone())),
            Error::Engine { url, .. } | Error::JavaScript { url, .. } | Error::Screenshot { url, .. } => {
                (url.clone(), None, None)
            }
            _ => (None, None, None),
        };
        let (output_kind, size, max) = match error {
            Error::OutputTooLarge { kind, size, max } => (
                Some((*kind).to_owned()),
                Some(u64::try_from(*size).unwrap_or(u64::MAX)),
                Some(u64::try_from(*max).unwrap_or(u64::MAX)),
            ),
            _ => (None, None, None),
        };
        let message = match error {
            Error::InvalidUrl { reason, .. } => reason.clone(),
            Error::Engine { source, .. } | Error::JavaScript { source, .. } | Error::Screenshot { source, .. } => {
                source.to_string()
            }
            _ => error.to_string(),
        };
        Self {
            kind: error_kind(error).to_owned(),
            message: bounded_text(message, MAX_ERROR_MESSAGE_BYTES),
            url: url.map(|url| bounded_text(url, MAX_ERROR_URL_BYTES)),
            timeout_ms,
            host: host.map(|host| bounded_text(host, MAX_ERROR_HOST_BYTES)),
            output_kind,
            size,
            max,
        }
    }

    pub(crate) fn into_error(self) -> Error {
        match self.kind.as_str() {
            "timeout" => Error::Timeout {
                url: self.url.unwrap_or_default(),
                timeout: Duration::from_millis(self.timeout_ms.unwrap_or_default()),
            },
            "invalid_url" => Error::InvalidUrl {
                url: self.url.unwrap_or_default(),
                reason: self.message,
            },
            "address_not_allowed" => Error::AddressNotAllowed {
                host: self.host.unwrap_or_default(),
            },
            "javascript" => Error::javascript(self.message, self.url),
            "screenshot" => Error::screenshot(self.message, self.url),
            "output_too_large" => Error::OutputTooLarge {
                kind: match self.output_kind.as_deref() {
                    Some("screenshot") => "screenshot",
                    Some("page") => "page",
                    _ => "worker output",
                },
                size: usize::try_from(self.size.unwrap_or(u64::MAX)).unwrap_or(usize::MAX),
                max: usize::try_from(self.max.unwrap_or(u64::MAX)).unwrap_or(usize::MAX),
            },
            "engine" => Error::engine(self.message, self.url),
            _ => worker_error(format!("{}: {}", self.kind, self.message)),
        }
    }
}

#[cfg(test)]
pub(crate) fn fetch_watchdog(opts: &FetchOptions) -> Duration {
    fetch_wire_watchdog(&FetchWire::from_options(opts))
}

#[cfg(test)]
pub(crate) fn crawl_watchdog(opts: &CrawlOptions) -> Duration {
    crawl_wire_watchdog(&CrawlWire::from_options(opts))
}

#[cfg(test)]
pub(crate) fn crawl_absolute_watchdog(opts: &CrawlOptions) -> Duration {
    crawl_wire_absolute_watchdog(&CrawlWire::from_options(opts))
}

pub(crate) const MAX_OPERATION_WATCHDOG: Duration = Duration::from_secs(24 * 60 * 60);

fn saturating_duration_mul(mut duration: Duration, mut factor: usize) -> Duration {
    let mut total = Duration::ZERO;
    while factor > 0 {
        if factor & 1 == 1 {
            total = total.saturating_add(duration);
        }
        factor >>= 1;
        if factor > 0 {
            duration = duration.saturating_add(duration);
        }
    }
    total
}

pub(crate) fn fetch_wire_watchdog(wire: &FetchWire) -> Duration {
    Duration::from_millis(wire.timeout_ms)
        .saturating_add(Duration::from_millis(wire.settle_ms))
        .saturating_add(Duration::from_secs(15))
        .min(MAX_OPERATION_WATCHDOG)
}

pub(crate) fn crawl_wire_watchdog(wire: &CrawlWire) -> Duration {
    let dispatch_slots = wire.concurrency.min(wire.limit).max(1);
    let dispatch_wait = saturating_duration_mul(
        wire.delay_ms.map_or(Duration::ZERO, Duration::from_millis),
        dispatch_slots,
    );
    Duration::from_millis(wire.timeout_ms)
        .saturating_mul(2)
        .saturating_add(Duration::from_millis(wire.settle_ms))
        .saturating_add(dispatch_wait)
        .saturating_add(Duration::from_secs(15))
        .min(MAX_OPERATION_WATCHDOG)
}

pub(crate) fn crawl_wire_absolute_watchdog(wire: &CrawlWire) -> Duration {
    let concurrency = wire.concurrency.max(1);
    let waves = wire.limit.max(1).div_ceil(concurrency);
    let page_budget = Duration::from_millis(wire.timeout_ms)
        .saturating_mul(2)
        .saturating_add(Duration::from_millis(wire.settle_ms))
        .saturating_add(Duration::from_secs(15));
    let total_delay = saturating_duration_mul(
        wire.delay_ms.map_or(Duration::ZERO, Duration::from_millis),
        wire.limit.saturating_sub(1),
    );
    saturating_duration_mul(page_budget, waves)
        .saturating_add(total_delay)
        .min(MAX_OPERATION_WATCHDOG)
}

pub(crate) const MAX_WIRE_DURATION: Duration = Duration::from_secs(24 * 60 * 60);
const MAX_WIRE_USER_AGENT_BYTES: usize = 8 * 1024;
const MAX_WIRE_URL_BYTES: usize = 2 * 1024 * 1024;
const MAX_WIRE_SCRIPT_BYTES: usize = 4 * 1024 * 1024;
const MAX_WIRE_SCHEMA_BYTES: usize = 4 * 1024 * 1024;
const MAX_WIRE_SELECTOR_BYTES: usize = 64 * 1024;
const MAX_WIRE_SCOPE_PATTERNS: usize = 1024;
const MAX_WIRE_SCOPE_PATTERN_BYTES: usize = 256 * 1024;
const MAX_WIRE_CRAWL_LIMIT: usize = 100_000;
const MAX_WIRE_CRAWL_DEPTH: usize = 1024;
pub(crate) const MAX_WIRE_CRAWL_CONCURRENCY: usize = 64;

fn ensure_bytes(value: &str, field: &str, max: usize) -> Result<()> {
    if value.len() > max {
        return Err(worker_error(format!("worker {field} exceeds {max} bytes")));
    }
    Ok(())
}

fn ensure_scope_patterns(include: &[String], exclude: &[String]) -> Result<()> {
    let count = include.len().saturating_add(exclude.len());
    if count > MAX_WIRE_SCOPE_PATTERNS {
        return Err(worker_error(format!(
            "worker crawl scope exceeds {MAX_WIRE_SCOPE_PATTERNS} patterns"
        )));
    }
    let bytes = include
        .iter()
        .chain(exclude)
        .fold(0_usize, |total, pattern| total.saturating_add(pattern.len()));
    if bytes > MAX_WIRE_SCOPE_PATTERN_BYTES {
        return Err(worker_error(format!(
            "worker crawl scope exceeds {MAX_WIRE_SCOPE_PATTERN_BYTES} bytes"
        )));
    }
    Ok(())
}

fn decode_duration_ms(value: u64, field: &str) -> Result<Duration> {
    let duration = Duration::from_millis(value);
    if duration > MAX_WIRE_DURATION {
        return Err(worker_error(format!(
            "worker {field} exceeds the maximum of {} seconds",
            MAX_WIRE_DURATION.as_secs()
        )));
    }
    Ok(duration)
}

fn decode_user_agent(value: Option<String>, field: &str) -> Result<Option<String>> {
    value
        .map(|value| {
            if value.len() > MAX_WIRE_USER_AGENT_BYTES {
                return Err(worker_error(format!(
                    "worker {field} exceeds {MAX_WIRE_USER_AGENT_BYTES} bytes"
                )));
            }
            Ok(crate::net::sanitize_user_agent(value))
        })
        .transpose()
}

#[derive(Serialize, Deserialize)]
enum FetchModeWire {
    Content { include_a11y: bool },
    Screenshot { full_page: bool },
    JavaScript(String),
}

#[derive(Serialize, Deserialize)]
struct SchemaWire {
    json: Vec<u8>,
}
impl SchemaWire {
    fn from_schema(schema: &crate::schema::ExtractSchema) -> Self {
        Self {
            json: serde_json::to_vec(schema).expect("schema serializes"),
        }
    }
    fn into_schema(self) -> Result<crate::schema::ExtractSchema> {
        if self.json.len() > MAX_WIRE_SCHEMA_BYTES {
            return Err(worker_error(format!(
                "worker fetch schema exceeds {MAX_WIRE_SCHEMA_BYTES} bytes"
            )));
        }
        let schema: crate::schema::ExtractSchema =
            serde_json::from_slice(&self.json).map_err(crate::schema::SchemaError::from)?;
        schema.validate()?;
        Ok(schema)
    }
}
#[derive(Serialize, Deserialize)]
pub(crate) struct FetchWire {
    url: String,
    timeout_ms: u64,
    settle_ms: u64,
    mode: FetchModeWire,
    schema: Option<SchemaWire>,
    visibility_bits: u32,
    headers: Vec<(String, Vec<u8>)>,
}

impl FetchWire {
    pub(crate) fn from_options(opts: &FetchOptions) -> Self {
        Self {
            url: opts.url.clone(),
            timeout_ms: u64::try_from(opts.effective_timeout().as_millis()).unwrap_or(u64::MAX),
            settle_ms: u64::try_from(opts.effective_settle().as_millis()).unwrap_or(u64::MAX),
            mode: match &opts.mode {
                FetchMode::Content { include_a11y } => FetchModeWire::Content {
                    include_a11y: *include_a11y,
                },
                FetchMode::Screenshot { full_page } => FetchModeWire::Screenshot { full_page: *full_page },
                FetchMode::JavaScript(expression) => FetchModeWire::JavaScript(expression.clone()),
            },
            schema: opts.extract_schema.as_ref().map(SchemaWire::from_schema),
            visibility_bits: opts.effective_visibility().strip_if_any.bits(),
            headers: encode_headers(&opts.headers),
        }
    }

    pub(crate) fn into_options(self) -> Result<FetchOptions> {
        ensure_bytes(&self.url, "fetch URL", MAX_WIRE_URL_BYTES)?;
        if let FetchModeWire::JavaScript(expression) = &self.mode {
            ensure_bytes(expression, "JavaScript expression", MAX_WIRE_SCRIPT_BYTES)?;
        }
        let url = crate::net::validate_url(&self.url)?.to_string();
        let timeout = decode_duration_ms(self.timeout_ms, "fetch timeout")?;
        let settle = decode_duration_ms(self.settle_ms, "fetch settle")?;
        let visibility = crate::VisibilityFlags::from_bits(self.visibility_bits)
            .ok_or_else(|| worker_error("worker fetch contains unknown visibility flags"))?;
        let mut opts = match self.mode {
            FetchModeWire::Content { include_a11y } => FetchOptions::new(&url).accessibility(include_a11y),
            FetchModeWire::Screenshot { full_page } => FetchOptions::screenshot(&url, full_page),
            FetchModeWire::JavaScript(expression) => FetchOptions::javascript(&url, expression),
        }
        .timeout(timeout)
        .settle(settle)
        .visibility(VisibilityPolicy {
            strip_if_any: visibility,
        })
        .headers(decode_headers(self.headers)?);
        if let Some(schema) = self.schema {
            opts = opts.schema(schema.into_schema()?);
        }
        Ok(opts)
    }
}

pub(crate) const MAX_SCREENSHOT_BYTES: usize = 32 * 1024 * 1024;
const RESPONSE_FRAME_HEADROOM: usize = 64;

#[derive(Default)]
struct SizeCounter(usize);

impl Write for SizeCounter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 = self.0.saturating_add(bytes.len());
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn encoded_size(value: &impl Serialize) -> Result<usize> {
    let mut counter = SizeCounter::default();
    postcard::to_io(value, &mut counter).map_err(|error| worker_error(error.to_string()))?;
    Ok(counter.0)
}

#[derive(Serialize, Deserialize)]
pub(crate) struct PageWire {
    html: String,
    inner_text: String,
    title: Option<String>,
    layout_json: Option<String>,
    visibility_json: Option<String>,
    js_result: Option<String>,
    console_messages: Vec<ConsoleMessageWire>,
    accessibility_tree: Option<String>,
    extracted_json: Option<Vec<u8>>,
    screenshot_png_bytes: Option<u32>,
    visibility_bits: u32,
}

impl PageWire {
    pub(crate) fn from_page(mut page: Page) -> Result<(Self, Option<Vec<u8>>)> {
        let screenshot_size = page.screenshot_png.as_ref().map_or(0, Vec::len);
        if screenshot_size > MAX_SCREENSHOT_BYTES {
            return Err(Error::OutputTooLarge {
                kind: "screenshot",
                size: screenshot_size,
                max: MAX_SCREENSHOT_BYTES,
            });
        }
        let extracted_json = page
            .extracted
            .as_ref()
            .map(serde_json::to_vec)
            .transpose()
            .map_err(worker_error)?;
        let screenshot_png = page.screenshot_png.take();
        let screenshot_png_bytes = screenshot_png
            .as_ref()
            .map(|png| u32::try_from(png.len()).expect("bounded screenshot size fits u32"));
        let wire = Self {
            html: page.html,
            inner_text: page.inner_text,
            title: page.title,
            layout_json: page.layout_json,
            visibility_json: page.visibility_json,
            js_result: page.js_result,
            console_messages: page
                .console_messages
                .into_iter()
                .map(ConsoleMessageWire::from_message)
                .collect(),
            accessibility_tree: page.accessibility_tree,
            extracted_json,
            screenshot_png_bytes,
            visibility_bits: page.visibility_policy.strip_if_any.bits(),
        };
        let frame_size = encoded_size(&wire)?.saturating_add(RESPONSE_FRAME_HEADROOM);
        if frame_size > MAX_WORKER_FRAME_BYTES {
            return Err(Error::OutputTooLarge {
                kind: "page",
                size: frame_size,
                max: MAX_WORKER_FRAME_BYTES,
            });
        }
        Ok((wire, screenshot_png))
    }

    pub(crate) fn screenshot_png_bytes(&self) -> Option<usize> {
        self.screenshot_png_bytes.and_then(|size| usize::try_from(size).ok())
    }

    #[cfg(test)]
    pub(crate) fn set_screenshot_png_bytes(&mut self, size: Option<u32>) {
        self.screenshot_png_bytes = size;
    }

    pub(crate) fn into_page(self, screenshot_png: Option<Vec<u8>>) -> Result<Page> {
        let actual_size = screenshot_png.as_ref().map(Vec::len);
        let expected_size = self.screenshot_png_bytes.and_then(|size| usize::try_from(size).ok());
        if actual_size != expected_size {
            return Err(worker_error(format!(
                "screenshot payload size mismatch: expected {expected_size:?}, got {actual_size:?}"
            )));
        }
        let extracted = self
            .extracted_json
            .as_deref()
            .map(serde_json::from_slice)
            .transpose()
            .map_err(worker_error)?;
        let a11y = self
            .accessibility_tree
            .as_deref()
            .map(serde_json::from_str)
            .transpose()
            .map_err(worker_error)?
            .map(Arc::new);
        let console_messages = self
            .console_messages
            .into_iter()
            .map(ConsoleMessageWire::into_message)
            .collect::<Result<Vec<_>>>()?;
        let visibility = crate::VisibilityFlags::from_bits(self.visibility_bits)
            .ok_or_else(|| worker_error("worker page contains unknown visibility flags"))?;
        Ok(Page {
            html: self.html,
            inner_text: self.inner_text,
            title: self.title,
            layout_json: self.layout_json,
            visibility_json: self.visibility_json,
            js_result: self.js_result,
            console_messages,
            accessibility_tree: self.accessibility_tree,
            extracted,
            screenshot_png,
            a11y,
            visibility_policy: VisibilityPolicy {
                strip_if_any: visibility,
            },
        })
    }
}

#[derive(Serialize, Deserialize)]
struct ConsoleMessageWire {
    level: String,
    message: String,
}

impl ConsoleMessageWire {
    fn from_message(message: ConsoleMessage) -> Self {
        Self {
            level: message.level.as_str().to_owned(),
            message: message.message,
        }
    }

    fn into_message(self) -> Result<ConsoleMessage> {
        let level = match self.level.as_str() {
            "log" => ConsoleLevel::Log,
            "debug" => ConsoleLevel::Debug,
            "info" => ConsoleLevel::Info,
            "warn" => ConsoleLevel::Warn,
            "error" => ConsoleLevel::Error,
            "trace" => ConsoleLevel::Trace,
            "dir" => ConsoleLevel::Dir,
            _ => return Err(worker_error("worker page contains an unknown console level")),
        };
        Ok(ConsoleMessage {
            level,
            message: self.message,
        })
    }
}

#[derive(Serialize, Deserialize)]
pub(crate) struct CrawlWire {
    url: String,
    limit: usize,
    max_depth: usize,
    timeout_ms: u64,
    settle_ms: u64,
    include: Vec<String>,
    exclude: Vec<String>,
    selector: Option<String>,
    json: bool,
    robots_user_agent: Option<String>,
    concurrency: usize,
    delay_ms: Option<u64>,
    headers: Vec<(String, Vec<u8>)>,
}

impl CrawlWire {
    pub(crate) fn from_options(opts: &CrawlOptions) -> Self {
        Self {
            url: opts.url.clone(),
            limit: opts.limit,
            max_depth: opts.max_depth,
            timeout_ms: u64::try_from(opts.timeout.as_millis()).unwrap_or(u64::MAX),
            settle_ms: u64::try_from(opts.settle.as_millis()).unwrap_or(u64::MAX),
            include: opts.include.clone(),
            exclude: opts.exclude.clone(),
            selector: opts.selector.clone(),
            json: opts.json,
            robots_user_agent: opts.robots_user_agent.clone(),
            concurrency: opts.concurrency,
            delay_ms: opts
                .delay
                .map(|delay| u64::try_from(delay.as_millis()).unwrap_or(u64::MAX)),
            headers: encode_headers(&opts.headers),
        }
    }

    pub(crate) fn into_options(self, fallback_user_agent: Option<&str>) -> Result<CrawlOptions> {
        ensure_bytes(&self.url, "crawl URL", MAX_WIRE_URL_BYTES)?;
        if self.limit > MAX_WIRE_CRAWL_LIMIT {
            return Err(worker_error(format!(
                "worker crawl limit exceeds {MAX_WIRE_CRAWL_LIMIT}"
            )));
        }
        if self.max_depth > MAX_WIRE_CRAWL_DEPTH {
            return Err(worker_error(format!(
                "worker crawl depth exceeds {MAX_WIRE_CRAWL_DEPTH}"
            )));
        }
        ensure_scope_patterns(&self.include, &self.exclude)?;
        if let Some(selector) = self.selector.as_deref() {
            ensure_bytes(selector, "crawl selector", MAX_WIRE_SELECTOR_BYTES)?;
        }
        if !(1..=MAX_WIRE_CRAWL_CONCURRENCY).contains(&self.concurrency) {
            return Err(worker_error(format!(
                "worker crawl concurrency must be between 1 and {MAX_WIRE_CRAWL_CONCURRENCY}"
            )));
        }
        let url = crate::net::validate_url(&self.url)?.to_string();
        let timeout = decode_duration_ms(self.timeout_ms, "crawl timeout")?;
        let settle = decode_duration_ms(self.settle_ms, "crawl settle")?;
        let delay = match self.delay_ms {
            None | Some(0) => None,
            Some(delay) => Some(decode_duration_ms(delay, "crawl delay")?),
        };
        let robots_user_agent = decode_user_agent(self.robots_user_agent, "robots user-agent")?
            .or_else(|| fallback_user_agent.map(str::to_owned));
        let include: Vec<&str> = self.include.iter().map(String::as_str).collect();
        let exclude: Vec<&str> = self.exclude.iter().map(String::as_str).collect();
        let mut opts = CrawlOptions::new(&url)
            .limit(self.limit)
            .max_depth(self.max_depth)
            .timeout(timeout)
            .settle(settle)
            .include(&include)
            .exclude(&exclude)
            .json(self.json)
            .concurrency(self.concurrency)
            .delay(delay)
            .headers(decode_headers(self.headers)?);
        opts.robots_user_agent = robots_user_agent;
        if let Some(selector) = self.selector {
            opts = opts.selector(selector);
        }
        Ok(opts)
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct CrawlProgressWire {
    pub(crate) processed: u64,
    pub(crate) emitted: u64,
    pub(crate) suppressed: u64,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct CrawlResultWire {
    url: String,
    depth: usize,
    fetched_at_ms: u64,
    outcome: CrawlOutcomeWire,
}

#[derive(Serialize, Deserialize)]
enum CrawlOutcomeWire {
    Page(CrawlPageWire),
    Error(WorkerErrorWire),
}

#[derive(Serialize, Deserialize)]
struct CrawlPageWire {
    title: Option<String>,
    content: String,
    links_found: usize,
}

impl CrawlResultWire {
    pub(crate) fn from_result(result: CrawlResult) -> Self {
        let fetched_at_ms = result
            .fetched_at
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX));
        match result.outcome {
            Ok(page) => Self {
                url: result.url,
                depth: result.depth,
                fetched_at_ms,
                outcome: CrawlOutcomeWire::Page(CrawlPageWire {
                    title: page.title,
                    content: page.content,
                    links_found: page.links_found,
                }),
            },
            Err(error) => Self {
                url: result.url,
                depth: result.depth,
                fetched_at_ms,
                outcome: CrawlOutcomeWire::Error(WorkerErrorWire::from_error(&error)),
            },
        }
    }

    pub(crate) fn into_result(self) -> Result<CrawlResult> {
        let outcome = match self.outcome {
            CrawlOutcomeWire::Page(page) => Ok(CrawlPage {
                title: page.title,
                content: page.content,
                links_found: page.links_found,
            }),
            CrawlOutcomeWire::Error(error) => Err(error.into_error()),
        };
        let fetched_at = UNIX_EPOCH
            .checked_add(Duration::from_millis(self.fetched_at_ms))
            .ok_or_else(|| worker_error("worker crawl result timestamp is out of range"))?;
        Ok(CrawlResult {
            url: self.url,
            depth: self.depth,
            fetched_at,
            outcome,
        })
    }
}

fn encode_headers(headers: &http::HeaderMap) -> Vec<(String, Vec<u8>)> {
    headers
        .iter()
        .map(|(name, value)| (name.as_str().to_owned(), value.as_bytes().to_vec()))
        .collect()
}

fn decode_headers(headers: Vec<(String, Vec<u8>)>) -> Result<http::HeaderMap> {
    crate::headers::from_wire_pairs(headers)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn beyond_duration_limit_ms() -> u64 {
        u64::try_from(MAX_WIRE_DURATION.as_millis()).unwrap() + 1
    }

    #[test]
    fn fetch_wire_rejects_unbounded_values_and_reserved_headers() {
        let mut timeout = FetchWire::from_options(&FetchOptions::new("https://example.com"));
        timeout.timeout_ms = beyond_duration_limit_ms();
        assert!(matches!(timeout.into_options(), Err(Error::WorkerUnavailable { .. })));

        let mut settle = FetchWire::from_options(&FetchOptions::new("https://example.com"));
        settle.settle_ms = beyond_duration_limit_ms();
        assert!(matches!(settle.into_options(), Err(Error::WorkerUnavailable { .. })));

        let mut visibility = FetchWire::from_options(&FetchOptions::new("https://example.com"));
        visibility.visibility_bits = u32::MAX;
        assert!(matches!(
            visibility.into_options(),
            Err(Error::WorkerUnavailable { .. })
        ));

        let mut headers = FetchWire::from_options(&FetchOptions::new("https://example.com"));
        headers.headers = vec![("host".into(), b"other.example".to_vec())];
        assert!(matches!(headers.into_options(), Err(Error::InvalidHeader(_))));
    }

    #[test]
    fn fetch_wire_uses_validated_normalized_url() {
        let wire = FetchWire::from_options(&FetchOptions::new("https://user:secret@example.com/path"));
        let options = wire.into_options().unwrap();
        assert_eq!(options.url, "https://example.com/path");
    }

    #[test]
    fn crawl_wire_bounds_concurrency_and_waits_and_sanitizes_robots_ua() {
        let mut concurrency = CrawlWire::from_options(&CrawlOptions::new("https://example.com"));
        concurrency.concurrency = MAX_WIRE_CRAWL_CONCURRENCY + 1;
        assert!(matches!(
            concurrency.into_options(None),
            Err(Error::WorkerUnavailable { .. })
        ));

        let mut delay = CrawlWire::from_options(&CrawlOptions::new("https://example.com"));
        delay.delay_ms = Some(beyond_duration_limit_ms());
        assert!(matches!(delay.into_options(None), Err(Error::WorkerUnavailable { .. })));

        let mut user_agent = CrawlWire::from_options(&CrawlOptions::new("https://example.com"));
        user_agent.robots_user_agent = Some("Bot\r\nInjected: yes\0".into());
        let options = user_agent.into_options(Some("SessionBot/1.0")).unwrap();
        assert_eq!(options.robots_user_agent.as_deref(), Some("Bot  Injected: yes "));

        let fallback = CrawlWire::from_options(&CrawlOptions::new("https://example.com"));
        let options = fallback.into_options(Some("SessionBot/1.0")).unwrap();
        assert_eq!(options.robots_user_agent.as_deref(), Some("SessionBot/1.0"));

        let mut wire = CrawlWire::from_options(&CrawlOptions::new("https://example.com"));
        wire.delay_ms = Some(0);
        let options = wire.into_options(None).unwrap();
        assert_eq!(options.delay, None);
    }

    #[test]
    fn request_fields_enforce_semantic_budgets() {
        let mut fetch = FetchWire::from_options(&FetchOptions::new("https://example.com"));
        fetch.url = "x".repeat(MAX_WIRE_URL_BYTES + 1);
        assert!(matches!(fetch.into_options(), Err(Error::WorkerUnavailable { .. })));

        let mut script = FetchWire::from_options(&FetchOptions::javascript("https://example.com", "1"));
        script.mode = FetchModeWire::JavaScript("x".repeat(MAX_WIRE_SCRIPT_BYTES + 1));
        assert!(matches!(script.into_options(), Err(Error::WorkerUnavailable { .. })));

        let mut schema = FetchWire::from_options(&FetchOptions::new("https://example.com"));
        schema.schema = Some(SchemaWire {
            json: vec![b' '; MAX_WIRE_SCHEMA_BYTES + 1],
        });
        assert!(matches!(schema.into_options(), Err(Error::WorkerUnavailable { .. })));

        let mut crawl = CrawlWire::from_options(&CrawlOptions::new("https://example.com"));
        crawl.limit = MAX_WIRE_CRAWL_LIMIT + 1;
        assert!(matches!(crawl.into_options(None), Err(Error::WorkerUnavailable { .. })));

        let mut scope = CrawlWire::from_options(&CrawlOptions::new("https://example.com"));
        scope.include = vec![String::new(); MAX_WIRE_SCOPE_PATTERNS + 1];
        assert!(matches!(scope.into_options(None), Err(Error::WorkerUnavailable { .. })));
    }

    #[test]
    fn worker_errors_are_bounded_before_serializing() {
        let error = Error::InvalidUrl {
            url: "u".repeat(MAX_ERROR_URL_BYTES * 2),
            reason: "reason".repeat(MAX_ERROR_MESSAGE_BYTES),
        };
        let wire = WorkerErrorWire::from_error(&error);
        assert!(wire.url.as_ref().is_some_and(|url| url.len() <= MAX_ERROR_URL_BYTES));
        assert!(wire.message.len() <= MAX_ERROR_MESSAGE_BYTES);
        assert!(postcard::to_stdvec(&wire).unwrap().len() < MAX_WORKER_FRAME_BYTES);

        let failure = WorkerErrorWire::failure(
            "k".repeat(MAX_ERROR_KIND_BYTES * 2),
            "m".repeat(MAX_ERROR_MESSAGE_BYTES * 2),
        );
        assert!(failure.kind.len() <= MAX_ERROR_KIND_BYTES);
        assert!(failure.message.len() <= MAX_ERROR_MESSAGE_BYTES);
    }

    #[test]
    fn page_output_uses_exact_encoded_size_and_caps_screenshots() {
        let screenshot = Page {
            screenshot_png: Some(vec![0; MAX_SCREENSHOT_BYTES + 1]),
            ..Page::default()
        };
        assert!(matches!(
            PageWire::from_page(screenshot),
            Err(Error::OutputTooLarge { kind: "screenshot", .. })
        ));

        let page = Page {
            html: "x".repeat(MAX_WORKER_FRAME_BYTES),
            ..Page::default()
        };
        assert!(matches!(
            PageWire::from_page(page),
            Err(Error::OutputTooLarge { kind: "page", .. })
        ));
    }
}
