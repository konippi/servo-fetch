//! Validate and resolve wire request inputs (URL, options) into engine values.

use std::collections::BTreeMap;
use std::time::Duration;

use servo_fetch::{CookieSpec, FetchOptions, HeaderMap, VisibilityPolicy};
use servo_fetch_types::{RequestOptions, Visibility};

use super::error::{ToolError, ToolResult};
use super::limits::{MAX_SELECTOR_LEN, MAX_SETTLE_MS, MAX_TIMEOUT_SECS};

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
    let mut opts = opts
        .timeout(resolve_timeout(options.timeout))
        .settle(resolve_settle(options.settle_ms));
    if let Some(ua) = options.user_agent {
        opts = opts.user_agent(ua);
    }
    if let Some(path) = options.cookies_file {
        opts = opts.cookies(load_cookies(&path)?);
    }
    Ok(opts.headers(build_headers(options.headers)?))
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
