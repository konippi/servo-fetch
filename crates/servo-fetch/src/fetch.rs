//! Single-page fetching and rendered content extraction.

use std::time::Duration;

use crate::error::Error;
use crate::net::sanitize_user_agent;

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

    /// Extract readable Markdown from the subtree matched by a CSS selector.
    pub fn markdown_with_selector(&self, url: &str, selector: &str) -> crate::error::Result<String> {
        let input = crate::extract::ExtractInput::new(&self.html, url)
            .with_layout_json(self.layout_json.as_deref())
            .with_inner_text(Some(&self.inner_text))
            .with_selector(Some(selector));
        Ok(crate::extract::extract_text(&input)?)
    }

    /// Extract structured JSON from the subtree matched by a CSS selector.
    pub fn extract_json_with_selector(&self, url: &str, selector: &str) -> crate::error::Result<String> {
        let input = crate::extract::ExtractInput::new(&self.html, url)
            .with_layout_json(self.layout_json.as_deref())
            .with_inner_text(Some(&self.inner_text))
            .with_selector(Some(selector));
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
#[must_use = "options do nothing until passed to fetch()"]
#[derive(Debug, Clone)]
pub struct FetchOptions {
    pub(crate) url: String,
    pub(crate) timeout: Duration,
    pub(crate) settle: Duration,
    pub(crate) mode: FetchMode,
    pub(crate) user_agent: Option<String>,
}

impl FetchOptions {
    /// Fetch rendered content (default mode).
    pub fn new(url: &str) -> Self {
        Self {
            url: url.into(),
            timeout: Duration::from_secs(30),
            settle: Duration::ZERO,
            mode: FetchMode::Content,
            user_agent: None,
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

    /// Override the User-Agent string for this request.
    pub fn user_agent(mut self, ua: impl Into<String>) -> Self {
        self.user_agent = Some(sanitize_user_agent(ua.into()));
        self
    }
}

/// Fetch a single page via the embedded Servo engine.
#[allow(clippy::needless_pass_by_value)]
pub fn fetch(opts: FetchOptions) -> crate::error::Result<Page> {
    crate::net::ensure_crypto_provider();

    crate::net::validate_url(&opts.url)?;

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
        user_agent: opts.user_agent.as_deref(),
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
        let _: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
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
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(parsed["url"].as_str(), Some("https://example.com/page"));
        assert!(parsed["text_content"].as_str().unwrap().contains("scoped"));
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
