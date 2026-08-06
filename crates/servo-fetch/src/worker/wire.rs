//! Serializable request, result, progress, and error types for isolated sessions.

#[cfg(test)]
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::{MAX_WORKER_FRAME_BYTES, worker_error};
#[cfg(test)]
use crate::CrawlPage;
use crate::error::{Error, Result};
#[cfg(test)]
use crate::fetch::{ConsoleLevel, FetchMode};
use crate::fetch::{ConsoleMessage, FetchOptions, Page};
use crate::{CrawlOptions, CrawlResult, VisibilityPolicy};

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
pub(super) struct WorkerErrorWire {
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
    pub(super) fn failure(kind: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            message: message.into(),
            url: None,
            timeout_ms: None,
            host: None,
            output_kind: None,
            size: None,
            max: None,
        }
    }

    pub(super) fn from_error(error: &Error) -> Self {
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
            message,
            url,
            timeout_ms,
            host,
            output_kind,
            size,
            max,
        }
    }

    #[cfg(test)]
    pub(super) fn into_error(self) -> Error {
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
            "invalid_header" => Error::InvalidHeader(self.message),
            "engine" => Error::engine(self.message, self.url),
            _ => worker_error(format!("{}: {}", self.kind, self.message)),
        }
    }
}

pub(super) const MAX_WIRE_DURATION: Duration = Duration::from_secs(24 * 60 * 60);
const MAX_WIRE_USER_AGENT_BYTES: usize = 8 * 1024;
pub(super) const MAX_WIRE_CRAWL_CONCURRENCY: usize = 64;

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
    #[cfg(test)]
    fn from_schema(schema: &crate::schema::ExtractSchema) -> Self {
        Self {
            json: serde_json::to_vec(schema).expect("schema serializes"),
        }
    }
    fn into_schema(self) -> Result<crate::schema::ExtractSchema> {
        let schema: crate::schema::ExtractSchema =
            serde_json::from_slice(&self.json).map_err(crate::schema::SchemaError::from)?;
        schema.validate()?;
        Ok(schema)
    }
}
#[derive(Serialize, Deserialize)]
pub(super) struct FetchWire {
    url: String,
    timeout_ms: u64,
    settle_ms: u64,
    mode: FetchModeWire,
    schema: Option<SchemaWire>,
    visibility_bits: u32,
    headers: Vec<(String, Vec<u8>)>,
}

impl FetchWire {
    #[cfg(test)]
    pub(super) fn from_options(opts: &FetchOptions) -> Self {
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

    pub(super) fn into_options(self) -> Result<FetchOptions> {
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

pub(super) const MAX_SCREENSHOT_BYTES: usize = 32 * 1024 * 1024;
const PAGE_FRAME_HEADROOM: usize = 64 * 1024;

#[derive(Serialize, Deserialize)]
pub(super) struct PageWire {
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
    pub(super) fn from_page(mut page: Page) -> Result<(Self, Option<Vec<u8>>)> {
        let screenshot_size = page.screenshot_png.as_ref().map_or(0, Vec::len);
        if screenshot_size > MAX_SCREENSHOT_BYTES {
            return Err(Error::OutputTooLarge {
                kind: "screenshot",
                size: screenshot_size,
                max: MAX_SCREENSHOT_BYTES,
            });
        }
        let text_size = page
            .html
            .len()
            .saturating_add(page.inner_text.len())
            .saturating_add(page.title.as_ref().map_or(0, String::len))
            .saturating_add(page.layout_json.as_ref().map_or(0, String::len))
            .saturating_add(page.visibility_json.as_ref().map_or(0, String::len))
            .saturating_add(page.js_result.as_ref().map_or(0, String::len))
            .saturating_add(page.accessibility_tree.as_ref().map_or(0, String::len))
            .saturating_add(
                page.console_messages
                    .iter()
                    .map(|message| message.message.len())
                    .sum::<usize>(),
            );
        let extracted_json = page
            .extracted
            .as_ref()
            .map(serde_json::to_vec)
            .transpose()
            .map_err(worker_error)?;
        let extracted_size = extracted_json.as_ref().map_or(0, Vec::len);
        let estimated_frame_size = text_size
            .saturating_add(extracted_size)
            .saturating_add(PAGE_FRAME_HEADROOM);
        if estimated_frame_size > MAX_WORKER_FRAME_BYTES {
            return Err(Error::OutputTooLarge {
                kind: "page",
                size: estimated_frame_size,
                max: MAX_WORKER_FRAME_BYTES,
            });
        }
        let screenshot_png = page.screenshot_png.take();
        let screenshot_png_bytes = screenshot_png
            .as_ref()
            .map(|png| u32::try_from(png.len()).expect("bounded screenshot size fits u32"));
        Ok((
            Self {
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
            },
            screenshot_png,
        ))
    }

    #[cfg(test)]
    pub(super) fn into_page(self, screenshot_png: Option<Vec<u8>>) -> Result<Page> {
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

    #[cfg(test)]
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
pub(super) struct CrawlWire {
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
    #[cfg(test)]
    pub(super) fn from_options(opts: &CrawlOptions) -> Self {
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

    pub(super) fn into_options(self, fallback_user_agent: Option<&str>) -> Result<CrawlOptions> {
        if !(1..=MAX_WIRE_CRAWL_CONCURRENCY).contains(&self.concurrency) {
            return Err(worker_error(format!(
                "worker crawl concurrency must be between 1 and {MAX_WIRE_CRAWL_CONCURRENCY}"
            )));
        }
        let url = crate::net::validate_url(&self.url)?.to_string();
        let timeout = decode_duration_ms(self.timeout_ms, "crawl timeout")?;
        let settle = decode_duration_ms(self.settle_ms, "crawl settle")?;
        let delay = self
            .delay_ms
            .map(|delay| decode_duration_ms(delay, "crawl delay"))
            .transpose()?;
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
pub(super) struct CrawlProgressWire {
    pub(super) processed: u64,
    pub(super) emitted: u64,
    pub(super) suppressed: u64,
}

#[derive(Serialize, Deserialize)]
pub(super) struct CrawlResultWire {
    url: String,
    depth: usize,
    fetched_at_ms: u64,
    page: Option<CrawlPageWire>,
    error: Option<WorkerErrorWire>,
}

#[derive(Serialize, Deserialize)]
struct CrawlPageWire {
    title: Option<String>,
    content: String,
    links_found: usize,
}

impl CrawlResultWire {
    pub(super) fn from_result(result: CrawlResult) -> Self {
        let fetched_at_ms = result
            .fetched_at
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX));
        match result.outcome {
            Ok(page) => Self {
                url: result.url,
                depth: result.depth,
                fetched_at_ms,
                page: Some(CrawlPageWire {
                    title: page.title,
                    content: page.content,
                    links_found: page.links_found,
                }),
                error: None,
            },
            Err(error) => Self {
                url: result.url,
                depth: result.depth,
                fetched_at_ms,
                page: None,
                error: Some(WorkerErrorWire::from_error(&error)),
            },
        }
    }

    #[cfg(test)]
    pub(super) fn into_result(self) -> Result<CrawlResult> {
        let outcome = match (self.page, self.error) {
            (Some(page), None) => Ok(CrawlPage {
                title: page.title,
                content: page.content,
                links_found: page.links_found,
            }),
            (None, Some(error)) => Err(error.into_error()),
            (Some(_), Some(_)) => return Err(worker_error("worker crawl result contains both page and error")),
            (None, None) => return Err(worker_error("worker crawl result contains neither page nor error")),
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

#[cfg(test)]
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
    fn response_wires_reject_unknown_or_inconsistent_semantics() {
        let (mut page, screenshot) = PageWire::from_page(Page::default()).unwrap();
        page.visibility_bits = u32::MAX;
        assert!(page.into_page(screenshot).is_err());

        let (mut page, screenshot) = PageWire::from_page(Page::default()).unwrap();
        page.accessibility_tree = Some("not-json".into());
        assert!(page.into_page(screenshot).is_err());

        let (mut page, screenshot) = PageWire::from_page(Page::default()).unwrap();
        page.console_messages = vec![ConsoleMessageWire {
            level: "unknown".into(),
            message: "message".into(),
        }];
        assert!(page.into_page(screenshot).is_err());

        let missing = CrawlResultWire {
            url: "https://example.com".into(),
            depth: 0,
            fetched_at_ms: 0,
            page: None,
            error: None,
        };
        assert!(missing.into_result().is_err());

        let conflicting = CrawlResultWire {
            url: "https://example.com".into(),
            depth: 0,
            fetched_at_ms: 0,
            page: Some(CrawlPageWire {
                title: None,
                content: String::new(),
                links_found: 0,
            }),
            error: Some(WorkerErrorWire::failure("engine", "failed")),
        };
        assert!(conflicting.into_result().is_err());
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
    }
}
