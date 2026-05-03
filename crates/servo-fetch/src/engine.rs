//! Servo browser engine facade.

use std::time::Duration;

use crate::error::Error;

/// Rendered page returned by [`fetch`].
#[derive(Debug, Clone, Default, serde::Serialize)]
#[non_exhaustive]
pub struct Page {
    /// Fully rendered HTML after JavaScript execution.
    pub html: String,
    /// Plain text content (`document.body.innerText`).
    pub inner_text: String,
    /// Page title extracted from `<title>` tag.
    pub title: Option<String>,
    /// Parsed layout data from the injected CSS heuristics script.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layout_json: Option<String>,
    /// Result of JavaScript evaluation, if [`FetchOptions::javascript`] was used.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub js_result: Option<String>,
    /// Browser console messages captured during page load.
    pub console_messages: Vec<ConsoleMessage>,
    /// Accessibility tree (AccessKit), if requested.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accessibility_tree: Option<String>,
    #[serde(skip)]
    screenshot_png: Option<Vec<u8>>,
}

impl Page {
    /// Extract readable Markdown from this page.
    pub fn markdown(&self) -> crate::error::Result<String> {
        self.markdown_with_url("")
    }

    /// Extract readable Markdown, using the original URL for link resolution.
    pub fn markdown_with_url(&self, url: &str) -> crate::error::Result<String> {
        let input = crate::extract::ExtractInput::new(&self.html, url)
            .with_layout_json(self.layout_json.as_deref())
            .with_inner_text(Some(&self.inner_text));
        Ok(crate::extract::extract_text(&input)?)
    }

    /// Extract structured JSON from this page.
    pub fn extract_json(&self) -> crate::error::Result<String> {
        self.extract_json_with_url("")
    }

    /// Extract structured JSON, using the original URL for link resolution.
    pub fn extract_json_with_url(&self, url: &str) -> crate::error::Result<String> {
        let input = crate::extract::ExtractInput::new(&self.html, url)
            .with_layout_json(self.layout_json.as_deref())
            .with_inner_text(Some(&self.inner_text));
        Ok(crate::extract::extract_json(&input)?)
    }

    /// PNG screenshot bytes, if captured via [`FetchOptions::screenshot`].
    #[must_use]
    pub fn screenshot_png(&self) -> Option<&[u8]> {
        self.screenshot_png.as_deref()
    }

    pub(crate) fn from_servo(page: crate::bridge::ServoPage) -> Self {
        let title = {
            let doc = dom_query::Document::from(page.html.as_str());
            let t = doc.select("title").text().to_string();
            if t.is_empty() { None } else { Some(t) }
        };
        let screenshot_png = page.screenshot.and_then(|img| {
            let mut buf = std::io::Cursor::new(Vec::new());
            img.write_to(&mut buf, image::ImageFormat::Png).ok()?;
            Some(buf.into_inner())
        });
        Self {
            html: page.html,
            inner_text: page.inner_text.unwrap_or_default(),
            title,
            layout_json: page.layout_json,
            js_result: page.js_result,
            console_messages: page
                .console_messages
                .into_iter()
                .map(|m| ConsoleMessage {
                    level: match m.level {
                        crate::bridge::ConsoleLevel::Log => ConsoleLevel::Log,
                        crate::bridge::ConsoleLevel::Debug => ConsoleLevel::Debug,
                        crate::bridge::ConsoleLevel::Info => ConsoleLevel::Info,
                        crate::bridge::ConsoleLevel::Warn => ConsoleLevel::Warn,
                        crate::bridge::ConsoleLevel::Error => ConsoleLevel::Error,
                        crate::bridge::ConsoleLevel::Trace => ConsoleLevel::Trace,
                    },
                    message: m.message,
                })
                .collect(),
            screenshot_png,
            accessibility_tree: page.accessibility_tree,
        }
    }
}

/// Browser console message captured during page load.
#[derive(Debug, Clone, serde::Serialize)]
#[non_exhaustive]
pub struct ConsoleMessage {
    /// Severity level.
    pub level: ConsoleLevel,
    /// Message text.
    pub message: String,
}

/// Console message severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum ConsoleLevel {
    /// General log message.
    Log,
    /// Debug-level message.
    Debug,
    /// Informational message.
    Info,
    /// Warning message.
    Warn,
    /// Error message.
    Error,
    /// Trace-level message.
    Trace,
}

impl ConsoleLevel {
    /// Returns the string representation of this level.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Log => "log",
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
            Self::Trace => "trace",
        }
    }
}

impl std::fmt::Display for ConsoleLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.pad(self.as_str())
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) enum FetchMode {
    #[default]
    Content,
    Screenshot {
        full_page: bool,
    },
    JavaScript(String),
}

/// Options for a single page fetch.
///
/// # Thread Safety
///
/// [`fetch`] is safe to call from multiple threads. Each call queues a request
/// to the shared Servo engine thread, which processes them sequentially.
#[must_use = "options do nothing until passed to fetch()"]
#[derive(Debug, Clone)]
pub struct FetchOptions {
    pub(crate) url: String,
    pub(crate) timeout: Duration,
    pub(crate) settle: Duration,
    pub(crate) mode: FetchMode,
}

impl FetchOptions {
    /// Fetch rendered content (default mode).
    pub fn new(url: &str) -> Self {
        Self {
            url: url.into(),
            timeout: Duration::from_secs(30),
            settle: Duration::ZERO,
            mode: FetchMode::Content,
        }
    }

    /// Capture a PNG screenshot.
    pub fn screenshot(url: &str, full_page: bool) -> Self {
        Self {
            mode: FetchMode::Screenshot { full_page },
            ..Self::new(url)
        }
    }

    /// Execute a JavaScript expression and return the result.
    pub fn javascript(url: &str, expression: impl Into<String>) -> Self {
        Self {
            mode: FetchMode::JavaScript(expression.into()),
            ..Self::new(url)
        }
    }

    /// Page load timeout (default: 30s).
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Extra wait after load event for SPA hydration (default: 0).
    pub fn settle(mut self, settle: Duration) -> Self {
        self.settle = settle;
        self
    }
}

/// Fetch a single page via the embedded Servo engine.
///
/// The first call spawns a persistent engine thread that lives for the process
/// lifetime. If the engine thread panics, this returns [`Error::Engine`].
#[allow(clippy::needless_pass_by_value)]
pub fn fetch(opts: FetchOptions) -> crate::error::Result<Page> {
    ensure_crypto_provider();

    crate::net::validate_url(&opts.url).map_err(|e| Error::InvalidUrl {
        url: opts.url.clone(),
        reason: e.to_string(),
    })?;

    if matches!(opts.mode, FetchMode::Content)
        && let Some(bytes) = crate::pdf::probe(&opts.url, opts.timeout.as_secs().max(1))
    {
        let text = crate::extract::extract_pdf(&bytes);
        return Ok(Page {
            html: String::new(),
            inner_text: text,
            ..Page::default()
        });
    }

    let bridge_opts = crate::bridge::FetchOptions {
        url: &opts.url,
        timeout_secs: opts.timeout.as_secs().max(1),
        settle_ms: u64::try_from(opts.settle.as_millis()).unwrap_or(u64::MAX),
        mode: match opts.mode {
            FetchMode::Content => crate::bridge::FetchMode::Content { include_a11y: false },
            FetchMode::Screenshot { full_page } => crate::bridge::FetchMode::Screenshot { full_page },
            FetchMode::JavaScript(ref expr) => crate::bridge::FetchMode::ExecuteJs {
                expression: expr.clone(),
            },
        },
    };

    let servo_page = crate::bridge::fetch_page(bridge_opts).map_err(|e| {
        let msg = format!("{e:#}");
        if msg.contains("timed out") {
            Error::Timeout {
                url: opts.url.clone(),
                timeout: opts.timeout,
            }
        } else {
            Error::Engine(msg)
        }
    })?;

    Ok(Page::from_servo(servo_page))
}

/// Options for crawling a site.
#[must_use = "options do nothing until passed to crawl() or crawl_each()"]
#[derive(Debug, Clone)]
pub struct CrawlOptions {
    pub(crate) url: String,
    pub(crate) limit: usize,
    pub(crate) max_depth: usize,
    pub(crate) timeout: Duration,
    pub(crate) settle: Duration,
    pub(crate) include: Vec<String>,
    pub(crate) exclude: Vec<String>,
    pub(crate) selector: Option<String>,
    pub(crate) json: bool,
}

impl CrawlOptions {
    /// Create crawl options for the given seed URL.
    pub fn new(url: &str) -> Self {
        Self {
            url: url.into(),
            limit: 50,
            max_depth: 3,
            timeout: Duration::from_secs(30),
            settle: Duration::ZERO,
            include: Vec::new(),
            exclude: Vec::new(),
            selector: None,
            json: false,
        }
    }

    /// Maximum number of pages to crawl (default: 50).
    pub fn limit(mut self, n: usize) -> Self {
        self.limit = n;
        self
    }

    /// Maximum link depth from the seed URL (default: 3).
    pub fn max_depth(mut self, n: usize) -> Self {
        self.max_depth = n;
        self
    }

    /// Page load timeout per page (default: 30s).
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Extra wait after load event per page (default: 0).
    pub fn settle(mut self, settle: Duration) -> Self {
        self.settle = settle;
        self
    }

    /// URL path glob patterns to include (e.g. `"/docs/**"`).
    pub fn include(mut self, patterns: &[&str]) -> Self {
        self.include = patterns.iter().map(|s| (*s).to_string()).collect();
        self
    }

    /// URL path glob patterns to exclude (e.g. `"/docs/archive/**"`).
    pub fn exclude(mut self, patterns: &[&str]) -> Self {
        self.exclude = patterns.iter().map(|s| (*s).to_string()).collect();
        self
    }

    /// Output crawled content as JSON instead of Markdown.
    pub fn json(mut self, json: bool) -> Self {
        self.json = json;
        self
    }

    /// CSS selector to extract a specific section per page.
    pub fn selector(mut self, selector: impl Into<String>) -> Self {
        self.selector = Some(selector.into());
        self
    }
}

/// Result for a single crawled page.
#[derive(Debug, Clone, serde::Serialize)]
#[non_exhaustive]
pub struct CrawlResult {
    /// URL of the crawled page.
    pub url: String,
    /// Link depth from the seed URL.
    pub depth: usize,
    /// Whether the page was fetched successfully.
    pub status: CrawlStatus,
    /// Page title, if extraction succeeded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Extracted content (Markdown or JSON depending on options).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Error message, if the page failed to load.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Number of links discovered on this page.
    pub links_found: usize,
}

impl CrawlResult {
    fn from_internal(r: &crate::crawl::CrawlPageResult) -> Self {
        Self {
            url: r.url.clone(),
            depth: r.depth,
            status: match r.status {
                crate::crawl::CrawlStatus::Ok => CrawlStatus::Ok,
                crate::crawl::CrawlStatus::Error => CrawlStatus::Error,
            },
            title: r.title.clone(),
            content: r.content.clone(),
            error: r.error.clone(),
            links_found: r.links_found,
        }
    }
}

/// Status of a crawled page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum CrawlStatus {
    /// Page fetched and extracted successfully.
    Ok,
    /// Page failed to load or extract.
    Error,
}

/// Crawl a site, invoking `on_page` for each result as it arrives.
#[allow(clippy::needless_pass_by_value)]
pub fn crawl_each(opts: CrawlOptions, mut on_page: impl FnMut(&CrawlResult)) -> crate::error::Result<()> {
    let internal_opts = build_crawl_options(&opts)?;
    crate::runtime::block_on(crate::crawl::run(internal_opts, |r| {
        on_page(&CrawlResult::from_internal(r));
    }))
    .map_err(|e| Error::Engine(e.to_string()))?;
    Ok(())
}

/// Crawl a site and collect all results.
#[allow(clippy::needless_pass_by_value)]
pub fn crawl(opts: CrawlOptions) -> crate::error::Result<Vec<CrawlResult>> {
    let mut results = Vec::new();
    crawl_each(opts, |r| results.push(r.clone()))?;
    Ok(results)
}

/// Fetch a URL and return readable Markdown.
pub fn markdown(url: &str) -> crate::error::Result<String> {
    fetch(FetchOptions::new(url))?.markdown_with_url(url)
}

/// Fetch a URL and return structured JSON.
pub fn extract_json(url: &str) -> crate::error::Result<String> {
    fetch(FetchOptions::new(url))?.extract_json_with_url(url)
}

/// Fetch a URL and return plain text (`document.body.innerText`).
pub fn text(url: &str) -> crate::error::Result<String> {
    Ok(fetch(FetchOptions::new(url))?.inner_text)
}

/// Validate a URL for fetching. Rejects disallowed schemes and private addresses.
pub fn validate_url(url: &str) -> crate::error::Result<url::Url> {
    crate::net::validate_url(url).map_err(|e| Error::InvalidUrl {
        url: url.into(),
        reason: e.to_string(),
    })
}

fn ensure_crypto_provider() {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
}

fn build_crawl_options(opts: &CrawlOptions) -> crate::error::Result<crate::crawl::CrawlOptions> {
    let seed = crate::net::validate_url(&opts.url).map_err(|e| Error::InvalidUrl {
        url: opts.url.clone(),
        reason: e.to_string(),
    })?;
    let include = if opts.include.is_empty() {
        None
    } else {
        Some(crate::crawl::build_globset(&opts.include).map_err(|e| Error::Engine(e.to_string()))?)
    };
    let exclude = if opts.exclude.is_empty() {
        None
    } else {
        Some(crate::crawl::build_globset(&opts.exclude).map_err(|e| Error::Engine(e.to_string()))?)
    };
    Ok(crate::crawl::CrawlOptions {
        seed,
        limit: opts.limit,
        max_depth: opts.max_depth,
        timeout_secs: opts.timeout.as_secs().max(1),
        settle_ms: u64::try_from(opts.settle.as_millis()).unwrap_or(u64::MAX),
        include,
        exclude,
        selector: opts.selector.clone(),
        json: opts.json,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fetch_options_defaults() {
        let opts = FetchOptions::new("https://example.com");
        assert_eq!(opts.url, "https://example.com");
        assert_eq!(opts.timeout, Duration::from_secs(30));
        assert_eq!(opts.settle, Duration::ZERO);
        assert!(matches!(opts.mode, FetchMode::Content));
    }

    #[test]
    fn fetch_options_screenshot() {
        let opts = FetchOptions::screenshot("https://example.com", true);
        assert!(matches!(opts.mode, FetchMode::Screenshot { full_page: true }));
    }

    #[test]
    fn fetch_options_javascript() {
        let opts = FetchOptions::javascript("https://example.com", "document.title");
        assert!(matches!(opts.mode, FetchMode::JavaScript(ref e) if e == "document.title"));
    }

    #[test]
    fn fetch_options_chaining() {
        let opts = FetchOptions::new("https://example.com")
            .timeout(Duration::from_secs(60))
            .settle(Duration::from_millis(500));
        assert_eq!(opts.timeout, Duration::from_secs(60));
        assert_eq!(opts.settle, Duration::from_millis(500));
    }

    #[test]
    fn crawl_options_defaults() {
        let opts = CrawlOptions::new("https://example.com");
        assert_eq!(opts.url, "https://example.com");
        assert_eq!(opts.limit, 50);
        assert_eq!(opts.max_depth, 3);
        assert_eq!(opts.timeout, Duration::from_secs(30));
        assert!(opts.include.is_empty());
        assert!(opts.exclude.is_empty());
    }

    #[test]
    fn crawl_options_chaining() {
        let opts = CrawlOptions::new("https://example.com")
            .limit(100)
            .max_depth(5)
            .timeout(Duration::from_secs(60))
            .include(&["/docs/**"])
            .exclude(&["/docs/archive/**"]);
        assert_eq!(opts.limit, 100);
        assert_eq!(opts.max_depth, 5);
        assert_eq!(opts.include, vec!["/docs/**"]);
        assert_eq!(opts.exclude, vec!["/docs/archive/**"]);
    }

    #[test]
    fn page_markdown_from_html() {
        let page = Page {
            html: "<html><head><title>Test</title></head><body><p>hello world</p></body></html>".into(),
            inner_text: "hello world".into(),
            ..Page::default()
        };
        let md = page.markdown().unwrap();
        assert!(md.contains("hello world"));
    }

    #[test]
    fn page_extract_json_produces_valid_json() {
        let page = Page {
            html: "<html><head><title>Test</title></head><body><p>content</p></body></html>".into(),
            inner_text: "content".into(),
            ..Page::default()
        };
        let json = page.extract_json().unwrap();
        let _: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
    }

    #[test]
    fn page_screenshot_png_none_by_default() {
        let page = Page::default();
        assert!(page.screenshot_png().is_none());
    }

    #[test]
    fn fetch_rejects_invalid_url() {
        let result = fetch(FetchOptions::new("not a url"));
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, Error::InvalidUrl { .. }));
    }

    #[test]
    fn fetch_rejects_private_ip() {
        let result = fetch(FetchOptions::new("http://127.0.0.1/"));
        assert!(result.is_err());
    }

    #[test]
    fn fetch_rejects_file_scheme() {
        let result = fetch(FetchOptions::new("file:///etc/passwd"));
        assert!(result.is_err());
    }
}
