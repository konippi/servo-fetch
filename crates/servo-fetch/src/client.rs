//! Async client with reusable defaults.

use std::sync::Arc;
use std::time::Duration;

use crate::error::{Error, Result};
use crate::fetch::{self, FetchOptions, Page};
use crate::net::sanitize_user_agent;
use crate::visibility::VisibilityPolicy;

/// Async client carrying reusable fetch defaults.
#[derive(Debug, Clone)]
pub struct Client {
    inner: Arc<ClientInner>,
}

#[derive(Debug)]
pub(crate) struct ClientInner {
    pub(crate) timeout: Duration,
    pub(crate) settle: Duration,
    pub(crate) user_agent: Option<String>,
    pub(crate) visibility: VisibilityPolicy,
}

impl Default for Client {
    fn default() -> Self {
        Self::new()
    }
}

impl Client {
    /// Construct a client with default options.
    #[must_use]
    pub fn new() -> Self {
        Self::builder().build()
    }

    /// Begin building a client with custom options.
    pub fn builder() -> ClientBuilder {
        ClientBuilder::default()
    }

    /// Fetch a page using the client's default options.
    pub async fn fetch(&self, url: &str) -> Result<Page> {
        fetch::fetch(&self.options(url)).await
    }

    /// Fetch a page with explicit options.
    ///
    /// Unset fields on `opts` fall back to the client's defaults; explicit
    /// values on `opts` always win.
    pub async fn fetch_with(&self, opts: &FetchOptions) -> Result<Page> {
        fetch::fetch(&self.apply_defaults(opts)).await
    }

    /// Fetch and extract readable Markdown.
    pub async fn markdown(&self, url: &str) -> Result<String> {
        self.fetch(url).await?.markdown_with_url(url)
    }

    /// Fetch and extract plain text (`document.body.innerText`).
    pub async fn text(&self, url: &str) -> Result<String> {
        Ok(self.fetch(url).await?.inner_text)
    }

    /// Fetch and extract structured JSON.
    pub async fn extract_json(&self, url: &str) -> Result<String> {
        self.fetch(url).await?.extract_json_with_url(url)
    }

    /// Capture a PNG screenshot of the page.
    pub async fn screenshot(&self, url: &str, opts: &ScreenshotOptions) -> Result<Vec<u8>> {
        let fopts = self.apply_defaults(&FetchOptions::screenshot(url, opts.full_page));
        let page = fetch::fetch(&fopts).await?;
        page.screenshot_png()
            .map(<[u8]>::to_vec)
            .ok_or_else(|| Error::screenshot(anyhow::anyhow!("screenshot returned no data"), Some(url.to_string())))
    }

    /// Execute a JavaScript expression after page load and return the result.
    pub async fn execute_js(&self, url: &str, expression: impl Into<String>) -> Result<String> {
        let fopts = self.apply_defaults(&FetchOptions::javascript(url, expression));
        let page = fetch::fetch(&fopts).await?;
        page.js_result
            .ok_or_else(|| Error::javascript(anyhow::anyhow!("execute_js returned no result"), Some(url.to_string())))
    }

    fn apply_defaults(&self, opts: &FetchOptions) -> FetchOptions {
        let mut opts = opts.clone();
        if opts.timeout.is_none() {
            opts.timeout = Some(self.inner.timeout);
        }
        if opts.settle.is_none() {
            opts.settle = Some(self.inner.settle);
        }
        if opts.visibility.is_none() {
            opts.visibility = Some(self.inner.visibility);
        }
        if opts.user_agent.is_none()
            && let Some(ua) = self.inner.user_agent.as_deref()
        {
            opts.user_agent = Some(ua.to_owned());
        }
        opts
    }

    fn options(&self, url: &str) -> FetchOptions {
        self.apply_defaults(&FetchOptions::new(url))
    }
}

/// Options for [`Client::screenshot`].
#[derive(Debug, Default, Clone)]
#[non_exhaustive]
pub struct ScreenshotOptions {
    /// Capture the full scrollable page (rather than only the viewport).
    pub full_page: bool,
}

/// Builder for [`Client`].
#[must_use = "ClientBuilder does nothing until .build() is called"]
#[derive(Debug, Default)]
pub struct ClientBuilder {
    timeout: Option<Duration>,
    settle: Option<Duration>,
    user_agent: Option<String>,
    visibility: Option<VisibilityPolicy>,
}

impl ClientBuilder {
    /// Default page-load timeout (default 30s).
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Default settle wait after the load event (default 0).
    pub fn settle(mut self, settle: Duration) -> Self {
        self.settle = Some(settle);
        self
    }

    /// Default User-Agent header.
    pub fn user_agent(mut self, ua: impl Into<String>) -> Self {
        self.user_agent = Some(sanitize_user_agent(ua.into()));
        self
    }

    /// Default visibility policy applied during extraction.
    pub fn visibility(mut self, policy: VisibilityPolicy) -> Self {
        self.visibility = Some(policy);
        self
    }

    /// Build the [`Client`].
    #[must_use]
    pub fn build(self) -> Client {
        Client {
            inner: Arc::new(self.build_inner()),
        }
    }

    pub(crate) fn build_inner(self) -> ClientInner {
        ClientInner {
            timeout: self.timeout.unwrap_or(FetchOptions::DEFAULT_TIMEOUT),
            settle: self.settle.unwrap_or_default(),
            user_agent: self.user_agent,
            visibility: self.visibility.unwrap_or_default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_default_uses_30s_timeout() {
        assert_eq!(Client::default().inner.timeout, FetchOptions::DEFAULT_TIMEOUT);
    }

    #[test]
    fn client_builder_sets_timeout() {
        let client = Client::builder().timeout(Duration::from_secs(60)).build();
        assert_eq!(client.inner.timeout, Duration::from_secs(60));
    }

    #[test]
    fn client_builder_sets_settle() {
        let client = Client::builder().settle(Duration::from_millis(500)).build();
        assert_eq!(client.inner.settle, Duration::from_millis(500));
    }

    #[test]
    fn client_builder_sanitizes_user_agent() {
        let client = Client::builder().user_agent("Bot\r\nX-Evil: yes").build();
        assert_eq!(client.inner.user_agent.as_deref(), Some("Bot  X-Evil: yes"));
    }

    #[test]
    fn client_options_propagates_defaults() {
        let client = Client::builder()
            .timeout(Duration::from_secs(60))
            .settle(Duration::from_millis(500))
            .user_agent("MyBot")
            .build();
        let opts = client.options("https://example.com");
        assert_eq!(opts.timeout, Some(Duration::from_secs(60)));
        assert_eq!(opts.settle, Some(Duration::from_millis(500)));
        assert_eq!(opts.user_agent.as_deref(), Some("MyBot"));
    }

    #[test]
    fn client_apply_defaults_caller_value_wins() {
        let client = Client::builder()
            .timeout(Duration::from_secs(60))
            .user_agent("ClientBot")
            .build();
        let user_opts = FetchOptions::new("https://example.com")
            .timeout(Duration::from_secs(10))
            .user_agent("UserBot");
        let merged = client.apply_defaults(&user_opts);
        // Caller's explicit values win
        assert_eq!(merged.timeout, Some(Duration::from_secs(10)));
        assert_eq!(merged.user_agent.as_deref(), Some("UserBot"));
    }

    #[test]
    fn client_apply_defaults_fills_unset_fields() {
        let client = Client::builder()
            .timeout(Duration::from_secs(60))
            .settle(Duration::from_millis(750))
            .user_agent("ClientBot")
            .visibility(VisibilityPolicy::off())
            .build();
        let user_opts = FetchOptions::new("https://example.com");
        let merged = client.apply_defaults(&user_opts);
        // None fields filled with client defaults
        assert_eq!(merged.timeout, Some(Duration::from_secs(60)));
        assert_eq!(merged.settle, Some(Duration::from_millis(750)));
        assert_eq!(merged.user_agent.as_deref(), Some("ClientBot"));
        assert_eq!(merged.visibility, Some(VisibilityPolicy::off()));
    }

    #[test]
    fn client_clone_shares_inner() {
        let client = Client::new();
        assert!(Arc::ptr_eq(&client.inner, &client.clone().inner));
    }

    #[test]
    fn screenshot_options_default_is_viewport() {
        assert!(!ScreenshotOptions::default().full_page);
    }

    #[test]
    fn assert_send_sync() {
        fn check<T: Send + Sync>() {}
        check::<Client>();
        check::<ClientBuilder>();
        check::<ScreenshotOptions>();
    }

    #[test]
    fn client_builder_sets_visibility() {
        let client = Client::builder().visibility(VisibilityPolicy::off()).build();
        assert_eq!(client.inner.visibility, VisibilityPolicy::off());
    }

    #[tokio::test]
    async fn client_fetch_invalid_url_returns_invalid_url_error() {
        let client = Client::new();
        let err = client.fetch("not a url").await.unwrap_err();
        assert!(matches!(err, Error::InvalidUrl { .. }), "got: {err:?}");
    }

    #[tokio::test]
    async fn client_fetch_private_address_is_rejected() {
        let client = Client::new();
        let err = client.fetch("http://127.0.0.1/").await.unwrap_err();
        assert!(err.is_network(), "got: {err:?}");
    }
}
