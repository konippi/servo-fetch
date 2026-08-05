//! Bounded PDF retrieval for one-shot fetches.

use std::time::Duration;

use anyhow::{Context as _, Result};

const MAX_PDF_BYTES: u64 = 50 * 1024 * 1024;

/// Return PDF bytes when the resource is a PDF, `None` otherwise.
pub(crate) fn probe(
    url: &str,
    timeout_secs: u64,
    user_agent: Option<&str>,
    headers: &http::HeaderMap,
) -> Option<Vec<u8>> {
    if !looks_like_pdf_url(url) {
        return None;
    }
    fetch_bytes(url, Duration::from_secs(timeout_secs), user_agent, headers).ok()
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

fn fetch_bytes(url: &str, timeout: Duration, user_agent: Option<&str>, headers: &http::HeaderMap) -> Result<Vec<u8>> {
    let agent = build_agent(timeout, user_agent);
    let mut request = agent.get(url);
    for (name, value) in headers {
        request = request.header(name.clone(), value.clone());
    }
    let response = request.call().with_context(|| format!("PDF GET failed for {url}"))?;
    let is_pdf = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.to_ascii_lowercase().starts_with("application/pdf"));
    if !is_pdf {
        anyhow::bail!("response is not a PDF");
    }
    response
        .into_body()
        .with_config()
        .limit(MAX_PDF_BYTES)
        .read_to_vec()
        .context("PDF body read failed")
}

fn build_agent(timeout: Duration, user_agent: Option<&str>) -> ureq::Agent {
    let mut config = ureq::config::Config::builder()
        .max_redirects(0)
        .timeout_global(Some(timeout));
    if let Some(user_agent) = user_agent {
        config = config.user_agent(user_agent);
    }
    ureq::Agent::new_with_config(config.build())
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
        assert!(probe("https://example.com/page.html", 1, None, &http::HeaderMap::new()).is_none());
    }

    #[test]
    fn probe_returns_none_for_unresolvable_host() {
        // Suffix matches, but GET fails quickly.
        assert!(probe("http://invalid.invalid/foo.pdf", 1, None, &http::HeaderMap::new()).is_none());
    }

    mod integration {
        use wiremock::matchers::{header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        use crate::pdf::probe;

        async fn run_probe_with(
            url: String,
            timeout: u64,
            user_agent: Option<&'static str>,
            headers: http::HeaderMap,
        ) -> Option<Vec<u8>> {
            tokio::task::spawn_blocking(move || probe(&url, timeout, user_agent, &headers))
                .await
                .unwrap()
        }

        async fn run_probe(url: String, timeout: u64) -> Option<Vec<u8>> {
            run_probe_with(url, timeout, None, http::HeaderMap::new()).await
        }

        #[tokio::test]
        async fn returns_bytes_when_get_is_pdf() {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path("/doc.pdf"))
                .respond_with(ResponseTemplate::new(200).set_body_raw(b"%PDF-1.4 minimal".to_vec(), "application/pdf"))
                .mount(&server)
                .await;

            let bytes = run_probe(format!("{}/doc.pdf", server.uri()), 5).await;
            assert_eq!(bytes.as_deref(), Some(&b"%PDF-1.4 minimal"[..]));
        }

        #[tokio::test]
        async fn sends_request_context_to_get() {
            let server = MockServer::start().await;
            let required = || {
                Mock::given(header("user-agent", "TestBot/1.0"))
                    .and(header("authorization", "Bearer test"))
                    .and(header("cookie", "sid=seed"))
                    .and(path("/protected.pdf"))
            };
            required()
                .and(method("GET"))
                .respond_with(ResponseTemplate::new(200).set_body_raw(b"PDF".to_vec(), "application/pdf"))
                .mount(&server)
                .await;
            let mut headers = http::HeaderMap::new();
            headers.insert(
                http::header::AUTHORIZATION,
                http::HeaderValue::from_static("Bearer test"),
            );
            headers.insert(http::header::COOKIE, http::HeaderValue::from_static("sid=seed"));

            let bytes = run_probe_with(
                format!("{}/protected.pdf", server.uri()),
                5,
                Some("TestBot/1.0"),
                headers,
            )
            .await;
            assert_eq!(bytes.as_deref(), Some(&b"PDF"[..]));
        }

        #[tokio::test]
        async fn returns_none_when_get_says_html() {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path("/lying.pdf"))
                .respond_with(ResponseTemplate::new(200).insert_header("content-type", "text/html"))
                .mount(&server)
                .await;

            assert!(run_probe(format!("{}/lying.pdf", server.uri()), 5).await.is_none());
        }

        #[tokio::test]
        async fn returns_none_when_get_lacks_content_type() {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path("/no-ct.pdf"))
                .respond_with(ResponseTemplate::new(200))
                .mount(&server)
                .await;

            assert!(run_probe(format!("{}/no-ct.pdf", server.uri()), 5).await.is_none());
        }

        #[tokio::test]
        async fn returns_none_when_get_returns_404() {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path("/missing.pdf"))
                .respond_with(ResponseTemplate::new(404))
                .mount(&server)
                .await;

            assert!(run_probe(format!("{}/missing.pdf", server.uri()), 5).await.is_none());
        }

        #[tokio::test]
        async fn skips_non_pdf_url_without_any_request() {
            let server = MockServer::start().await;
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
            Mock::given(method("GET"))
                .and(path("/x.pdf"))
                .respond_with(
                    ResponseTemplate::new(200).set_body_raw(b"PDF".to_vec(), "application/pdf; charset=binary"),
                )
                .mount(&server)
                .await;

            assert_eq!(
                run_probe(format!("{}/x.pdf", server.uri()), 5).await.as_deref(),
                Some(&b"PDF"[..]),
            );
        }

        #[tokio::test]
        async fn does_not_follow_redirects() {
            // Disable redirects to prevent SSRF bypass.
            let server = MockServer::start().await;
            Mock::given(method("GET"))
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
        async fn probe_uses_single_get_request() {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path("/big.pdf"))
                .respond_with(ResponseTemplate::new(200).set_body_raw(b"PDF".to_vec(), "application/pdf"))
                .expect(1)
                .mount(&server)
                .await;

            let _ = run_probe(format!("{}/big.pdf", server.uri()), 5).await;
        }
    }
}
