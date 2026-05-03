//! PDF detection and retrieval, run before Servo so PDF handling never
//! blocks concurrent `WebView`s in the engine queue.

use std::time::Duration;

use anyhow::{Context as _, Result};

const MAX_PDF_BYTES: u64 = 50 * 1024 * 1024;
const HEAD_TIMEOUT: Duration = Duration::from_secs(5);

/// Return PDF bytes when the resource is a PDF, `None` otherwise.
pub(crate) fn probe(url: &str, timeout_secs: u64) -> Option<Vec<u8>> {
    if !looks_like_pdf_url(url) {
        return None;
    }
    let overall = Duration::from_secs(timeout_secs);
    if !head_is_pdf(url, HEAD_TIMEOUT.min(overall)) {
        return None;
    }
    fetch_bytes(url, overall).ok()
}

fn looks_like_pdf_url(url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(url) else {
        return false;
    };
    parsed.path().rsplit('/').next().is_some_and(|last| {
        last.rsplit_once('.')
            .is_some_and(|(_, ext)| ext.eq_ignore_ascii_case("pdf"))
    })
}

fn head_is_pdf(url: &str, timeout: Duration) -> bool {
    let Ok(resp) = build_agent(timeout).head(url).call() else {
        return false;
    };
    resp.headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|ct| ct.to_ascii_lowercase().starts_with("application/pdf"))
}

fn fetch_bytes(url: &str, timeout: Duration) -> Result<Vec<u8>> {
    build_agent(timeout)
        .get(url)
        .call()
        .with_context(|| format!("PDF GET failed for {url}"))?
        .into_body()
        .with_config()
        .limit(MAX_PDF_BYTES)
        .read_to_vec()
        .context("PDF body read failed")
}

fn build_agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::new_with_config(
        ureq::config::Config::builder()
            .max_redirects(0)
            .timeout_global(Some(timeout))
            .build(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_suffix_detection() {
        assert!(looks_like_pdf_url("https://example.com/foo.pdf"));
        assert!(looks_like_pdf_url("https://example.com/FOO.PDF"));
        assert!(looks_like_pdf_url("https://example.com/a/b/c.pdf?x=1#anchor"));
        assert!(!looks_like_pdf_url("https://example.com/"));
        assert!(!looks_like_pdf_url("https://example.com/page.html"));
        assert!(!looks_like_pdf_url("https://example.com/download?id=123"));
        assert!(!looks_like_pdf_url("not a url"));
    }

    #[test]
    fn probe_skips_non_pdf_urls_without_network() {
        // No network traffic because suffix check fails first.
        assert!(probe("https://example.com/page.html", 1).is_none());
    }

    #[test]
    fn probe_returns_none_for_unresolvable_host() {
        // Suffix matches, HEAD fails quickly.
        assert!(probe("http://invalid.invalid/foo.pdf", 1).is_none());
    }
}
