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

    mod integration {
        use crate::pdf::probe;
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        async fn run_probe(url: String, timeout: u64) -> Option<Vec<u8>> {
            tokio::task::spawn_blocking(move || probe(&url, timeout)).await.unwrap()
        }

        #[tokio::test]
        async fn returns_bytes_when_head_advertises_pdf() {
            let server = MockServer::start().await;
            Mock::given(method("HEAD"))
                .and(path("/doc.pdf"))
                .respond_with(ResponseTemplate::new(200).insert_header("content-type", "application/pdf"))
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .and(path("/doc.pdf"))
                .respond_with(ResponseTemplate::new(200).set_body_raw(b"%PDF-1.4 minimal".to_vec(), "application/pdf"))
                .mount(&server)
                .await;

            let bytes = run_probe(format!("{}/doc.pdf", server.uri()), 5).await;
            assert_eq!(bytes.as_deref(), Some(&b"%PDF-1.4 minimal"[..]));
        }

        #[tokio::test]
        async fn returns_none_when_head_says_html() {
            let server = MockServer::start().await;
            Mock::given(method("HEAD"))
                .and(path("/lying.pdf"))
                .respond_with(ResponseTemplate::new(200).insert_header("content-type", "text/html"))
                .mount(&server)
                .await;

            assert!(run_probe(format!("{}/lying.pdf", server.uri()), 5).await.is_none());
        }

        #[tokio::test]
        async fn returns_none_when_head_lacks_content_type() {
            let server = MockServer::start().await;
            Mock::given(method("HEAD"))
                .and(path("/no-ct.pdf"))
                .respond_with(ResponseTemplate::new(200))
                .mount(&server)
                .await;

            assert!(run_probe(format!("{}/no-ct.pdf", server.uri()), 5).await.is_none());
        }

        #[tokio::test]
        async fn returns_none_when_head_returns_404() {
            let server = MockServer::start().await;
            Mock::given(method("HEAD"))
                .and(path("/missing.pdf"))
                .respond_with(ResponseTemplate::new(404))
                .mount(&server)
                .await;

            assert!(run_probe(format!("{}/missing.pdf", server.uri()), 5).await.is_none());
        }

        #[tokio::test]
        async fn skips_non_pdf_url_without_any_request() {
            let server = MockServer::start().await;
            Mock::given(method("HEAD"))
                .respond_with(ResponseTemplate::new(500))
                .expect(0)
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .respond_with(ResponseTemplate::new(500))
                .expect(0)
                .mount(&server)
                .await;

            assert!(run_probe(format!("{}/page.html", server.uri()), 5).await.is_none());
        }

        #[tokio::test]
        async fn accepts_content_type_with_parameter() {
            let server = MockServer::start().await;
            Mock::given(method("HEAD"))
                .and(path("/x.pdf"))
                .respond_with(
                    ResponseTemplate::new(200).insert_header("content-type", "application/pdf; charset=binary"),
                )
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .and(path("/x.pdf"))
                .respond_with(ResponseTemplate::new(200).set_body_raw(b"PDF".to_vec(), "application/pdf"))
                .mount(&server)
                .await;

            assert_eq!(
                run_probe(format!("{}/x.pdf", server.uri()), 5).await.as_deref(),
                Some(&b"PDF"[..]),
            );
        }

        #[tokio::test]
        async fn does_not_follow_redirects() {
            // PDF probe is configured with `max_redirects(0)` to avoid SSRF bypass via
            // redirect to a private IP.
            let server = MockServer::start().await;
            Mock::given(method("HEAD"))
                .and(path("/redirect.pdf"))
                .respond_with(ResponseTemplate::new(302).insert_header("location", "https://example.com/"))
                .mount(&server)
                .await;

            assert!(
                run_probe(format!("{}/redirect.pdf", server.uri()), 5).await.is_none(),
                "302 must not be followed (SSRF gate)",
            );
        }

        #[tokio::test]
        async fn head_request_actually_uses_head_method() {
            let server = MockServer::start().await;
            Mock::given(method("HEAD"))
                .and(path("/big.pdf"))
                .respond_with(ResponseTemplate::new(200).insert_header("content-type", "application/pdf"))
                .expect(1)
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .and(path("/big.pdf"))
                .respond_with(ResponseTemplate::new(200).set_body_raw(b"PDF".to_vec(), "application/pdf"))
                .expect(1)
                .mount(&server)
                .await;

            let _ = run_probe(format!("{}/big.pdf", server.uri()), 5).await;
            // Mock expectations are verified on `MockServer` drop.
        }
    }
}
