//! Validate and resolve wire request inputs (URL, options) into engine values.

use std::collections::BTreeMap;
use std::time::Duration;

use servo_fetch::{BrowserSessionConfig, CookieSpec, FetchOptions, HeaderMap, VisibilityPolicy};
use servo_fetch_types::{FetchFormat, RequestOptions, Visibility};

use super::error::{ToolError, ToolResult};
use super::limits::{MAX_SELECTOR_LEN, MAX_SETTLE_MS, MAX_TIMEOUT_SECS};

/// Base options for a content fetch; captures the accessibility tree only when the format needs it.
pub(crate) fn content_options(url: &str, format: FetchFormat, visibility: VisibilityPolicy) -> FetchOptions {
    FetchOptions::new(url)
        .visibility(visibility)
        .accessibility(matches!(format, FetchFormat::AccessibilityTree))
}

/// Map the wire visibility policy onto the engine policy (default: moderate).
pub(crate) fn visibility_policy(v: Option<Visibility>) -> VisibilityPolicy {
    match v {
        Some(Visibility::Strict) => VisibilityPolicy::strict(),
        Some(Visibility::Off) => VisibilityPolicy::off(),
        Some(Visibility::Moderate) | None => VisibilityPolicy::moderate(),
    }
}

/// Resolve the page-load timeout (default 30s, clamped to the surface limit).
pub(crate) fn resolve_timeout(secs: Option<u64>) -> Duration {
    Duration::from_secs(secs.unwrap_or(30).clamp(1, MAX_TIMEOUT_SECS))
}

/// Resolve the post-load settle wait (default 0, clamped to the surface limit).
pub(crate) fn resolve_settle(ms: Option<u64>) -> Duration {
    Duration::from_millis(ms.unwrap_or(0).min(MAX_SETTLE_MS))
}

/// Apply the common request options (timeout, settle, UA, cookies, headers).
pub(crate) fn apply_options(opts: FetchOptions, options: RequestOptions) -> ToolResult<FetchOptions> {
    Ok(ResolvedRequestOptions::try_from(options)?.apply(opts))
}

/// Shared request settings resolved once per call and applied to every URL.
#[derive(Clone, Debug)]
pub(crate) struct ResolvedRequestOptions {
    timeout: Duration,
    settle: Duration,
    user_agent: Option<String>,
    cookies: Vec<CookieSpec>,
    headers: HeaderMap,
}

impl TryFrom<RequestOptions> for ResolvedRequestOptions {
    type Error = ToolError;

    fn try_from(options: RequestOptions) -> ToolResult<Self> {
        let RequestOptions {
            timeout,
            settle_ms,
            user_agent,
            cookies_file,
            headers,
        } = options;
        Ok(Self {
            timeout: resolve_timeout(timeout),
            settle: resolve_settle(settle_ms),
            user_agent,
            cookies: match cookies_file {
                Some(path) => load_cookies(&path)?,
                None => Vec::new(),
            },
            headers: build_headers(headers)?,
        })
    }
}

impl ResolvedRequestOptions {
    /// Split into one-use session identity (UA, cookies) and per-fetch settings.
    pub(crate) fn into_session(self, url: &str, opts: FetchOptions) -> (BrowserSessionConfig, FetchOptions) {
        let mut config = BrowserSessionConfig::new();
        if let Some(user_agent) = self.user_agent {
            config = config.user_agent(user_agent);
        }
        if !self.cookies.is_empty() {
            config = config.cookies(url, self.cookies);
        }
        (
            config,
            opts.timeout(self.timeout).settle(self.settle).headers(self.headers),
        )
    }

    /// Apply the settings to one fetch.
    pub(crate) fn apply(&self, opts: FetchOptions) -> FetchOptions {
        let mut opts = opts
            .timeout(self.timeout)
            .settle(self.settle)
            .cookies(self.cookies.clone())
            .headers(self.headers.clone());
        if let Some(user_agent) = &self.user_agent {
            opts = opts.user_agent(user_agent.clone());
        }
        opts
    }
}

/// Load and validate a Netscape-format cookies.txt file.
pub(crate) fn load_cookies(path: &str) -> ToolResult<Vec<CookieSpec>> {
    servo_fetch::load_cookies(path).map_err(ToolError::from)
}

/// Validate and build a `HeaderMap` from raw name/value pairs.
pub(crate) fn build_headers(headers: Option<BTreeMap<String, String>>) -> ToolResult<HeaderMap> {
    match headers {
        Some(map) => servo_fetch::headers::from_pairs(&map).map_err(ToolError::from),
        None => Ok(HeaderMap::new()),
    }
}

/// Validate a request URL and return its canonical form.
pub(crate) fn validated_url(url: &str) -> ToolResult<String> {
    servo_fetch::validate_url(url)
        .map(|u| u.to_string())
        .map_err(ToolError::from)
}

/// Borrow a wire glob list as `&str` slices for the engine builders.
pub(crate) fn glob_refs(globs: &[String]) -> Vec<&str> {
    globs.iter().map(String::as_str).collect()
}

/// Validate that a CSS selector is within the length limit.
pub(crate) fn validate_selector(selector: Option<&str>) -> ToolResult<()> {
    if selector.is_some_and(|s| s.len() > MAX_SELECTOR_LEN) {
        return Err(ToolError::invalid_params(format!(
            "selector exceeds {MAX_SELECTOR_LEN} character limit"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use tempfile::NamedTempFile;

    use super::*;

    #[test]
    fn settings_resolve_every_shared_setting_once() {
        let mut cookies = NamedTempFile::new().expect("cookie file");
        writeln!(cookies, ".example.com\tTRUE\t/\tFALSE\t0\tsession\tsecret").expect("cookie fixture");
        let mut headers = BTreeMap::new();
        headers.insert("X-Test".to_string(), "kept".to_string());

        let settings = ResolvedRequestOptions::try_from(RequestOptions {
            timeout: Some(7),
            settle_ms: Some(11),
            user_agent: Some("test-agent".to_string()),
            cookies_file: Some(cookies.path().to_string_lossy().into_owned()),
            headers: Some(headers),
        })
        .expect("options are valid");

        assert_eq!(settings.timeout, Duration::from_secs(7));
        assert_eq!(settings.settle, Duration::from_millis(11));
        assert_eq!(settings.user_agent.as_deref(), Some("test-agent"));
        assert_eq!(settings.cookies.len(), 1);
        assert_eq!(settings.headers["x-test"], "kept");
    }

    #[test]
    fn settings_from_a_missing_cookie_file_fail() {
        let error = ResolvedRequestOptions::try_from(RequestOptions {
            timeout: None,
            settle_ms: None,
            user_agent: None,
            cookies_file: Some("/nonexistent/cookies.txt".to_string()),
            headers: None,
        })
        .unwrap_err();

        assert!(error.to_string().contains("cookie"));
    }
}
