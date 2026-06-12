//! Single-page fetching and rendered content extraction.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use servo::accesskit::{Node, NodeId};

use crate::error::Error;
use crate::net::sanitize_user_agent;

/// Rendered page returned by [`crate::fetch()`].
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
    /// Per-node visibility flags from the visibility-aware extraction pass.
    #[serde(skip)]
    visibility_json: Option<String>,
    /// Result of JavaScript evaluation, if [`FetchOptions::javascript`] was used.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub js_result: Option<String>,
    /// Browser console messages captured during page load.
    pub console_messages: Vec<ConsoleMessage>,
    /// Accessibility tree (AccessKit), serialized as JSON, if requested.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accessibility_tree: Option<String>,
    /// Structured data extracted via [`FetchOptions::schema`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extracted: Option<Value>,
    /// PNG-encoded screenshot bytes — read via [`Page::screenshot_png`].
    #[serde(skip)]
    screenshot_png: Option<Vec<u8>>,
    /// Typed AccessKit tree, shared cheaply across [`Page`] clones.
    #[serde(skip)]
    a11y: Option<Arc<HashMap<NodeId, Node>>>,
    /// Visibility policy that was active when this page was fetched.
    #[serde(skip)]
    visibility_policy: crate::visibility::VisibilityPolicy,
}

impl Page {
    /// Extract readable Markdown from this page.
    pub fn markdown(&self) -> crate::error::Result<String> {
        self.markdown_with_url("")
    }

    /// Extract readable Markdown, using the original URL for link resolution.
    pub fn markdown_with_url(&self, url: &str) -> crate::error::Result<String> {
        Ok(crate::extract::extract_text(&self.extract_input(url, None))?)
    }

    /// Extract structured JSON from this page.
    pub fn extract_json(&self) -> crate::error::Result<String> {
        self.extract_json_with_url("")
    }

    /// Extract structured JSON, using the original URL for link resolution.
    pub fn extract_json_with_url(&self, url: &str) -> crate::error::Result<String> {
        Ok(crate::extract::extract_json(&self.extract_input(url, None))?)
    }

    /// Extract structured article data, using the original URL for link resolution.
    pub fn article(&self, url: &str) -> crate::error::Result<crate::extract::ArticleData> {
        Ok(crate::extract::extract_article(&self.extract_input(url, None))?)
    }

    /// Extract structured article data from the subtree matched by a CSS selector.
    pub fn article_with_selector(
        &self,
        url: &str,
        selector: &str,
    ) -> crate::error::Result<crate::extract::ArticleData> {
        Ok(crate::extract::extract_article(
            &self.extract_input(url, Some(selector)),
        )?)
    }

    /// Extract readable Markdown from the subtree matched by a CSS selector.
    pub fn markdown_with_selector(&self, url: &str, selector: &str) -> crate::error::Result<String> {
        Ok(crate::extract::extract_text(&self.extract_input(url, Some(selector)))?)
    }

    /// Extract structured JSON from the subtree matched by a CSS selector.
    pub fn extract_json_with_selector(&self, url: &str, selector: &str) -> crate::error::Result<String> {
        Ok(crate::extract::extract_json(&self.extract_input(url, Some(selector)))?)
    }

    /// PNG screenshot bytes, if captured via [`FetchOptions::screenshot`].
    #[must_use]
    pub fn screenshot_png(&self) -> Option<&[u8]> {
        self.screenshot_png.as_deref()
    }

    fn extract_input<'a>(&'a self, url: &'a str, selector: Option<&'a str>) -> crate::extract::ExtractInput<'a> {
        crate::extract::ExtractInput::new(&self.html, url)
            .with_layout_json(self.layout_json.as_deref())
            .with_visibility_json(self.visibility_json.as_deref())
            .with_a11y(self.a11y.as_deref())
            .with_inner_text(Some(&self.inner_text))
            .with_selector(selector)
            .with_visibility(self.visibility_policy)
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
            visibility_json: page.visibility_json,
            js_result: page.js_result,
            console_messages: page.console_messages.into_iter().map(ConsoleMessage::from).collect(),
            screenshot_png,
            accessibility_tree: page.accessibility_tree,
            a11y: page.a11y.map(Arc::new),
            extracted: None,
            visibility_policy: crate::visibility::VisibilityPolicy::default(),
        }
    }
}

/// Browser console message captured during page load.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
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

impl fmt::Display for ConsoleLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(self.as_str())
    }
}

impl From<crate::bridge::ConsoleLevel> for ConsoleLevel {
    fn from(level: crate::bridge::ConsoleLevel) -> Self {
        use crate::bridge::ConsoleLevel as Bridge;
        match level {
            Bridge::Log => Self::Log,
            Bridge::Debug => Self::Debug,
            Bridge::Info => Self::Info,
            Bridge::Warn => Self::Warn,
            Bridge::Error => Self::Error,
            Bridge::Trace => Self::Trace,
        }
    }
}

impl From<crate::bridge::ConsoleMessage> for ConsoleMessage {
    fn from(msg: crate::bridge::ConsoleMessage) -> Self {
        Self {
            level: msg.level.into(),
            message: msg.message,
        }
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
#[must_use = "options do nothing until passed to fetch()"]
#[derive(Debug, Clone)]
pub struct FetchOptions {
    pub(crate) url: String,
    pub(crate) timeout: Option<Duration>,
    pub(crate) settle: Option<Duration>,
    pub(crate) mode: FetchMode,
    pub(crate) user_agent: Option<String>,
    pub(crate) extract_schema: Option<crate::schema::ExtractSchema>,
    pub(crate) visibility: Option<crate::visibility::VisibilityPolicy>,
    pub(crate) cookies: Vec<crate::cookies::CookieSpec>,
}

impl FetchOptions {
    /// Default page-load timeout used when neither `FetchOptions::timeout` nor
    /// a [`crate::Client`] override is set.
    pub(crate) const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

    /// Default settle wait used when neither `FetchOptions::settle` nor a
    /// [`crate::Client`] override is set.
    pub(crate) const DEFAULT_SETTLE: Duration = Duration::ZERO;

    /// Fetch rendered content (default mode).
    pub fn new(url: &str) -> Self {
        Self {
            url: url.into(),
            timeout: None,
            settle: None,
            mode: FetchMode::Content,
            user_agent: None,
            extract_schema: None,
            visibility: None,
            cookies: Vec::new(),
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
        self.timeout = Some(timeout);
        self
    }

    /// Extra wait after load event for SPA hydration (default: 0).
    pub fn settle(mut self, settle: Duration) -> Self {
        self.settle = Some(settle);
        self
    }

    /// Override the User-Agent string for this request.
    pub fn user_agent(mut self, ua: impl Into<String>) -> Self {
        self.user_agent = Some(sanitize_user_agent(ua.into()));
        self
    }

    /// Extract structured data from the rendered page using the given schema.
    pub fn schema(mut self, schema: crate::schema::ExtractSchema) -> Self {
        self.extract_schema = Some(schema);
        self
    }

    /// Visibility-filtering policy applied during extraction.
    pub fn visibility(mut self, policy: crate::visibility::VisibilityPolicy) -> Self {
        self.visibility = Some(policy);
        self
    }

    /// Seed session cookies before navigation, scoped to the target's site.
    pub fn cookies(mut self, cookies: Vec<crate::cookies::CookieSpec>) -> Self {
        self.cookies = cookies;
        self
    }

    /// Resolve the effective timeout, falling back to [`Self::DEFAULT_TIMEOUT`].
    pub(crate) fn effective_timeout(&self) -> Duration {
        self.timeout.unwrap_or(Self::DEFAULT_TIMEOUT)
    }

    /// Resolve the effective settle wait, falling back to [`Self::DEFAULT_SETTLE`].
    pub(crate) fn effective_settle(&self) -> Duration {
        self.settle.unwrap_or(Self::DEFAULT_SETTLE)
    }

    /// Resolve the effective visibility policy, falling back to its default.
    pub(crate) fn effective_visibility(&self) -> crate::visibility::VisibilityPolicy {
        self.visibility.unwrap_or_default()
    }
}

/// Fetch a single page via the embedded Servo engine (blocking).
pub fn fetch_blocking(opts: &FetchOptions) -> crate::error::Result<Page> {
    if let Some(pdf_page) = pre_fetch(opts)? {
        return Ok(pdf_page);
    }
    let bridge_opts = build_bridge_options(opts);
    let servo_page = crate::bridge::fetch_page(bridge_opts).map_err(|e| map_engine_error(e, opts))?;
    Ok(finalize_page(servo_page, opts))
}

/// Fetch a single page via the embedded Servo engine.
pub async fn fetch(opts: &FetchOptions) -> crate::error::Result<Page> {
    if let Some(pdf_page) = pre_fetch_async(opts).await? {
        return Ok(pdf_page);
    }
    let bridge_opts = build_bridge_options(opts);
    let servo_page = crate::bridge::fetch_page_async(bridge_opts)
        .await
        .map_err(|e| map_engine_error(e, opts))?;
    Ok(finalize_page(servo_page, opts))
}

/// Fetch a URL and return readable Markdown (blocking).
pub fn markdown_blocking(url: &str) -> crate::error::Result<String> {
    fetch_blocking(&FetchOptions::new(url))?.markdown_with_url(url)
}

/// Fetch a URL and return readable Markdown.
pub async fn markdown(url: &str) -> crate::error::Result<String> {
    fetch(&FetchOptions::new(url)).await?.markdown_with_url(url)
}

/// Fetch a URL and return structured JSON (blocking).
pub fn extract_json_blocking(url: &str) -> crate::error::Result<String> {
    fetch_blocking(&FetchOptions::new(url))?.extract_json_with_url(url)
}

/// Fetch a URL and return structured JSON.
pub async fn extract_json(url: &str) -> crate::error::Result<String> {
    fetch(&FetchOptions::new(url)).await?.extract_json_with_url(url)
}

/// Fetch a URL and return plain text (`document.body.innerText`) (blocking).
pub fn text_blocking(url: &str) -> crate::error::Result<String> {
    Ok(fetch_blocking(&FetchOptions::new(url))?.inner_text)
}

/// Fetch a URL and return plain text (`document.body.innerText`).
pub async fn text(url: &str) -> crate::error::Result<String> {
    Ok(fetch(&FetchOptions::new(url)).await?.inner_text)
}

fn pre_fetch(opts: &FetchOptions) -> crate::error::Result<Option<Page>> {
    crate::net::ensure_crypto_provider();
    crate::net::validate_url(&opts.url)?;

    if matches!(opts.mode, FetchMode::Content)
        && let Some(bytes) = crate::pdf::probe(&opts.url, opts.effective_timeout().as_secs().max(1))
    {
        return Ok(Some(pdf_page(&bytes)));
    }

    Ok(None)
}

async fn pre_fetch_async(opts: &FetchOptions) -> crate::error::Result<Option<Page>> {
    crate::net::ensure_crypto_provider();
    crate::net::validate_url(&opts.url)?;

    if matches!(opts.mode, FetchMode::Content) {
        let url = opts.url.clone();
        let timeout_secs = opts.effective_timeout().as_secs().max(1);
        let probe = tokio::task::spawn_blocking(move || crate::pdf::probe(&url, timeout_secs))
            .await
            .map_err(|e| Error::engine(anyhow::anyhow!("pdf probe task panicked: {e}"), Some(opts.url.clone())))?;
        if let Some(bytes) = probe {
            return Ok(Some(pdf_page(&bytes)));
        }
    }

    Ok(None)
}

fn pdf_page(bytes: &[u8]) -> Page {
    let text = crate::extract::extract_pdf(bytes);
    Page {
        html: String::new(),
        inner_text: text,
        ..Page::default()
    }
}

fn build_bridge_options(opts: &FetchOptions) -> crate::bridge::FetchOptions<'_> {
    crate::bridge::FetchOptions {
        url: &opts.url,
        timeout_secs: opts.effective_timeout().as_secs().max(1),
        settle_ms: u64::try_from(opts.effective_settle().as_millis()).unwrap_or(u64::MAX),
        user_agent: opts.user_agent.as_deref(),
        cookies: &opts.cookies,
        mode: match opts.mode {
            FetchMode::Content => crate::bridge::FetchMode::Content { include_a11y: false },
            FetchMode::Screenshot { full_page } => crate::bridge::FetchMode::Screenshot { full_page },
            FetchMode::JavaScript(ref expr) => crate::bridge::FetchMode::ExecuteJs {
                expression: expr.clone(),
            },
        },
    }
}

fn finalize_page(servo_page: crate::bridge::ServoPage, opts: &FetchOptions) -> Page {
    let mut page = Page::from_servo(servo_page);
    page.visibility_policy = opts.effective_visibility();
    if let Some(schema) = opts.extract_schema.as_ref() {
        page.extracted = Some(schema.extract_from(&page.html));
    }
    page
}

fn map_engine_error(e: crate::bridge::EngineError, opts: &FetchOptions) -> Error {
    match e {
        crate::bridge::EngineError::Timeout(_) => Error::Timeout {
            url: opts.url.clone(),
            timeout: opts.effective_timeout(),
        },
        crate::bridge::EngineError::Other(e) => Error::engine(e, Some(opts.url.clone())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fetch_options_defaults() {
        let opts = FetchOptions::new("https://example.com");
        assert_eq!(opts.url, "https://example.com");
        assert_eq!(opts.timeout, None);
        assert_eq!(opts.settle, None);
        assert_eq!(opts.visibility, None);
        assert!(matches!(opts.mode, FetchMode::Content));
    }

    #[test]
    fn fetch_options_effective_defaults() {
        let opts = FetchOptions::new("https://example.com");
        assert_eq!(opts.effective_timeout(), Duration::from_secs(30));
        assert_eq!(opts.effective_settle(), Duration::ZERO);
    }

    #[test]
    fn fetch_options_caller_value_preserved() {
        let opts = FetchOptions::new("https://example.com")
            .timeout(Duration::from_secs(45))
            .settle(Duration::from_millis(250));
        assert_eq!(opts.timeout, Some(Duration::from_secs(45)));
        assert_eq!(opts.settle, Some(Duration::from_millis(250)));
        assert_eq!(opts.effective_timeout(), Duration::from_secs(45));
        assert_eq!(opts.effective_settle(), Duration::from_millis(250));
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
        assert_eq!(opts.timeout, Some(Duration::from_secs(60)));
        assert_eq!(opts.settle, Some(Duration::from_millis(500)));
    }

    #[test]
    fn fetch_user_agent_set() {
        let opts = FetchOptions::new("https://example.com").user_agent("MyBot/1.0");
        assert_eq!(opts.user_agent.as_deref(), Some("MyBot/1.0"));
    }

    #[test]
    fn fetch_user_agent_default_is_none() {
        let opts = FetchOptions::new("https://example.com");
        assert!(opts.user_agent.is_none());
    }

    #[test]
    fn fetch_user_agent_sanitizes_crlf() {
        let opts = FetchOptions::new("https://example.com").user_agent("Bot\r\nX-Evil: yes");
        assert_eq!(opts.user_agent.as_deref(), Some("Bot  X-Evil: yes"));
    }

    #[test]
    fn fetch_user_agent_sanitizes_null() {
        let opts = FetchOptions::new("https://example.com").user_agent("Bot\0/1.0");
        assert_eq!(opts.user_agent.as_deref(), Some("Bot /1.0"));
    }

    #[test]
    fn fetch_user_agent_empty_string() {
        let opts = FetchOptions::new("https://example.com").user_agent("");
        assert_eq!(opts.user_agent.as_deref(), Some(""));
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
        let _: Value = serde_json::from_str(&json).expect("valid JSON");
    }

    #[test]
    fn page_screenshot_png_none_by_default() {
        let page = Page::default();
        assert!(page.screenshot_png().is_none());
    }

    #[test]
    fn page_markdown_with_selector_scopes_to_subtree() {
        let page = Page {
            html: "<html><body><article>keep</article><aside>drop</aside></body></html>".into(),
            ..Page::default()
        };
        let md = page.markdown_with_selector("https://example.com", "article").unwrap();
        assert!(md.contains("keep"));
        assert!(!md.contains("drop"));
    }

    #[test]
    fn page_extract_json_with_selector_includes_url() {
        let page = Page {
            html: "<html><body><article>scoped</article></body></html>".into(),
            ..Page::default()
        };
        let json = page
            .extract_json_with_selector("https://example.com/page", "article")
            .unwrap();
        let parsed: Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(parsed["url"].as_str(), Some("https://example.com/page"));
        assert!(parsed["textContent"].as_str().unwrap().contains("scoped"));
    }

    #[test]
    fn page_markdown_with_selector_no_match_returns_empty() {
        let page = Page {
            html: "<html><body><article>x</article></body></html>".into(),
            ..Page::default()
        };
        let md = page.markdown_with_selector("", ".nonexistent").unwrap();
        assert!(md.is_empty());
    }

    #[test]
    fn page_markdown_with_invalid_selector_returns_error() {
        let page = Page {
            html: "<html><body><p>x</p></body></html>".into(),
            ..Page::default()
        };
        let err = page.markdown_with_selector("", "###invalid[[[").unwrap_err();
        assert!(err.to_string().contains("invalid CSS selector"));
    }

    #[test]
    fn page_markdown_with_empty_selector_returns_error() {
        let page = Page {
            html: "<html><body><p>x</p></body></html>".into(),
            ..Page::default()
        };
        assert!(page.markdown_with_selector("", "").is_err());
    }

    #[test]
    fn fetch_rejects_invalid_url() {
        let result = fetch_blocking(&FetchOptions::new("not a url"));
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, Error::InvalidUrl { .. }));
    }

    #[test]
    fn fetch_rejects_private_ip() {
        let result = fetch_blocking(&FetchOptions::new("http://127.0.0.1/"));
        assert!(result.is_err());
    }

    #[test]
    fn fetch_rejects_file_scheme() {
        let result = fetch_blocking(&FetchOptions::new("file:///etc/passwd"));
        assert!(result.is_err());
    }

    mod page_from_servo {
        use crate::bridge;
        use crate::fetch::{ConsoleLevel, Page};

        fn synthetic_image(w: u32, h: u32) -> image::RgbaImage {
            image::RgbaImage::from_pixel(w, h, image::Rgba([255, 0, 0, 255]))
        }

        fn empty_servo_page() -> bridge::ServoPage {
            bridge::ServoPage::default()
        }

        #[test]
        fn extracts_title_from_html() {
            let mut sp = empty_servo_page();
            sp.html = "<html><head><title>Hello World</title></head></html>".into();
            let page = Page::from_servo(sp);
            assert_eq!(page.title.as_deref(), Some("Hello World"));
        }

        #[test]
        fn title_is_none_when_tag_missing() {
            let mut sp = empty_servo_page();
            sp.html = "<html><body>no title here</body></html>".into();
            let page = Page::from_servo(sp);
            assert!(page.title.is_none());
        }

        #[test]
        fn title_is_none_when_tag_empty() {
            let mut sp = empty_servo_page();
            sp.html = "<html><head><title></title></head></html>".into();
            let page = Page::from_servo(sp);
            assert!(page.title.is_none());
        }

        #[test]
        fn title_is_none_for_empty_html() {
            let page = Page::from_servo(empty_servo_page());
            assert!(page.title.is_none());
        }

        #[test]
        fn inner_text_none_becomes_empty_string() {
            let sp = empty_servo_page();
            assert!(sp.inner_text.is_none());
            let page = Page::from_servo(sp);
            assert_eq!(page.inner_text, "");
        }

        #[test]
        fn screenshot_is_encoded_as_png() {
            let mut sp = empty_servo_page();
            sp.screenshot = Some(synthetic_image(8, 8));
            let page = Page::from_servo(sp);
            let bytes = page.screenshot_png().expect("screenshot encoded");
            assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "PNG magic bytes");
        }

        #[test]
        fn console_messages_empty_by_default() {
            let page = Page::from_servo(empty_servo_page());
            assert!(page.console_messages.is_empty());
        }

        #[test]
        fn console_messages_preserve_all_six_levels() {
            let cases = [
                (bridge::ConsoleLevel::Log, ConsoleLevel::Log),
                (bridge::ConsoleLevel::Debug, ConsoleLevel::Debug),
                (bridge::ConsoleLevel::Info, ConsoleLevel::Info),
                (bridge::ConsoleLevel::Warn, ConsoleLevel::Warn),
                (bridge::ConsoleLevel::Error, ConsoleLevel::Error),
                (bridge::ConsoleLevel::Trace, ConsoleLevel::Trace),
            ];
            for (src, expected) in cases {
                let mut sp = empty_servo_page();
                sp.console_messages = vec![bridge::ConsoleMessage {
                    level: src,
                    message: "msg".into(),
                }];
                let page = Page::from_servo(sp);
                assert_eq!(
                    page.console_messages.len(),
                    1,
                    "console message lost for source level {src:?}",
                );
                assert_eq!(
                    page.console_messages[0].level, expected,
                    "level mapping wrong for source {src:?}",
                );
            }
        }

        #[test]
        fn console_messages_preserve_ordering_across_levels() {
            let mut sp = empty_servo_page();
            sp.console_messages = vec![
                bridge::ConsoleMessage {
                    level: bridge::ConsoleLevel::Info,
                    message: "first".into(),
                },
                bridge::ConsoleMessage {
                    level: bridge::ConsoleLevel::Error,
                    message: "second".into(),
                },
                bridge::ConsoleMessage {
                    level: bridge::ConsoleLevel::Warn,
                    message: "third".into(),
                },
            ];
            let page = Page::from_servo(sp);
            assert_eq!(page.console_messages.len(), 3);
            assert_eq!(page.console_messages[0].message, "first");
            assert_eq!(page.console_messages[1].message, "second");
            assert_eq!(page.console_messages[2].message, "third");
            assert_eq!(page.console_messages[0].level, ConsoleLevel::Info);
            assert_eq!(page.console_messages[1].level, ConsoleLevel::Error);
            assert_eq!(page.console_messages[2].level, ConsoleLevel::Warn);
        }

        #[test]
        fn extracted_starts_as_none_until_schema_applied() {
            let page = Page::from_servo(empty_servo_page());
            assert!(page.extracted.is_none());
        }

        #[test]
        fn full_round_trip_preserves_every_field() {
            let sp = bridge::ServoPage {
                html: "<html><head><title>T</title></head><body>B</body></html>".into(),
                inner_text: Some("B".into()),
                layout_json: Some("[]".into()),
                visibility_json: Some("[]".into()),
                screenshot: Some(synthetic_image(2, 2)),
                js_result: Some("42".into()),
                accessibility_tree: Some("{}".into()),
                a11y: None,
                console_messages: vec![bridge::ConsoleMessage {
                    level: bridge::ConsoleLevel::Log,
                    message: "x".into(),
                }],
            };
            let page = Page::from_servo(sp);
            assert_eq!(page.html, "<html><head><title>T</title></head><body>B</body></html>");
            assert_eq!(page.inner_text, "B");
            assert_eq!(page.title.as_deref(), Some("T"));
            assert_eq!(page.layout_json.as_deref(), Some("[]"));
            assert_eq!(page.js_result.as_deref(), Some("42"));
            assert_eq!(page.accessibility_tree.as_deref(), Some("{}"));
            assert_eq!(page.console_messages.len(), 1);
            assert!(page.screenshot_png().is_some());
            assert!(page.extracted.is_none());
        }
    }
}
