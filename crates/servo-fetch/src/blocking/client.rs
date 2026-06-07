//! Blocking client mirror of [`crate::Client`].

use std::sync::Arc;
use std::time::Duration;

use crate::error::{Error, Result};
use crate::fetch::{FetchOptions, Page, fetch_blocking};
use crate::visibility::VisibilityPolicy;
use crate::{ScreenshotOptions, client};

/// Blocking client carrying reusable fetch defaults.
#[derive(Debug, Clone)]
pub struct Client {
    inner: Arc<client::ClientInner>,
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
    pub fn fetch(&self, url: &str) -> Result<Page> {
        fetch_blocking(&self.inner.options(url))
    }

    /// Fetch a page with explicit options.
    pub fn fetch_with(&self, opts: &FetchOptions) -> Result<Page> {
        fetch_blocking(&self.inner.apply_defaults(opts.clone()))
    }

    /// Fetch and extract readable Markdown.
    pub fn markdown(&self, url: &str) -> Result<String> {
        self.fetch(url)?.markdown_with_url(url)
    }

    /// Fetch and extract plain text (`document.body.innerText`).
    pub fn text(&self, url: &str) -> Result<String> {
        Ok(self.fetch(url)?.inner_text)
    }

    /// Fetch and extract structured JSON.
    pub fn extract_json(&self, url: &str) -> Result<String> {
        self.fetch(url)?.extract_json_with_url(url)
    }

    /// Capture a PNG screenshot of the page.
    pub fn screenshot(&self, url: &str, opts: &ScreenshotOptions) -> Result<Vec<u8>> {
        let fopts = self.inner.apply_defaults(FetchOptions::screenshot(url, opts.full_page));
        let page = fetch_blocking(&fopts)?;
        page.screenshot_png()
            .map(<[u8]>::to_vec)
            .ok_or_else(|| Error::screenshot(anyhow::anyhow!("screenshot returned no data"), Some(url.to_string())))
    }

    /// Execute a JavaScript expression after page load and return the result.
    pub fn execute_js(&self, url: &str, expression: impl Into<String>) -> Result<String> {
        let fopts = self.inner.apply_defaults(FetchOptions::javascript(url, expression));
        let page = fetch_blocking(&fopts)?;
        page.js_result
            .ok_or_else(|| Error::javascript(anyhow::anyhow!("execute_js returned no result"), Some(url.to_string())))
    }
}

/// Builder for [`Client`].
#[must_use = "ClientBuilder does nothing until .build() is called"]
#[derive(Debug, Default)]
pub struct ClientBuilder {
    inner: client::ClientBuilder,
}

impl ClientBuilder {
    /// Default page-load timeout (default 30s).
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.inner = self.inner.timeout(timeout);
        self
    }

    /// Default settle wait after the load event (default 0).
    pub fn settle(mut self, settle: Duration) -> Self {
        self.inner = self.inner.settle(settle);
        self
    }

    /// Default User-Agent header.
    pub fn user_agent(mut self, ua: impl Into<String>) -> Self {
        self.inner = self.inner.user_agent(ua);
        self
    }

    /// Default visibility policy applied during extraction.
    pub fn visibility(mut self, policy: VisibilityPolicy) -> Self {
        self.inner = self.inner.visibility(policy);
        self
    }

    /// Build the [`Client`].
    #[must_use]
    pub fn build(self) -> Client {
        Client {
            inner: Arc::new(self.inner.build_inner()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_default_uses_30s_timeout() {
        assert_eq!(Client::default().inner.timeout, Duration::from_secs(30));
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
    fn client_builder_sets_visibility() {
        let client = Client::builder().visibility(VisibilityPolicy::off()).build();
        assert_eq!(client.inner.visibility, VisibilityPolicy::off());
    }

    #[test]
    fn client_builder_sanitizes_user_agent() {
        let client = Client::builder().user_agent("Bot\r\nX-Evil: yes").build();
        assert_eq!(client.inner.user_agent.as_deref(), Some("Bot  X-Evil: yes"));
    }

    #[test]
    fn client_clone_shares_inner() {
        let client = Client::new();
        assert!(Arc::ptr_eq(&client.inner, &client.clone().inner));
    }

    #[test]
    fn assert_send_sync() {
        fn check<T: Send + Sync>() {}
        check::<Client>();
        check::<ClientBuilder>();
    }

    #[test]
    fn fetch_invalid_url_returns_invalid_url_error() {
        let client = Client::new();
        let err = client.fetch("not a url").unwrap_err();
        assert!(matches!(err, Error::InvalidUrl { .. }));
    }

    #[test]
    fn fetch_private_address_is_rejected() {
        let client = Client::new();
        let err = client.fetch("http://127.0.0.1/").unwrap_err();
        assert!(err.is_network(), "got: {err:?}");
    }
}
