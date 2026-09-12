//! Bounded PDF retrieval that bypasses the browser engine.

use std::time::Duration;

use crate::transfer;

const MAX_PDF_BYTES: u64 = 50 * 1024 * 1024;

/// GET a validated URL and return its bytes when the server serves a PDF, `None` otherwise.
pub(crate) async fn probe(target: &url::Url, headers: &transfer::Headers, timeout: Duration) -> Option<Vec<u8>> {
    let client = transfer::client().ok()?;
    let response = headers.get(&client, target, timeout).send().await.ok()?;
    let is_pdf = response
        .headers()
        .get(http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.to_ascii_lowercase().starts_with("application/pdf"));
    if !response.status().is_success() || !is_pdf {
        return None;
    }
    transfer::collect_bounded(response, MAX_PDF_BYTES).await
}

pub(crate) fn looks_like_pdf_url(url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(url) else {
        return false;
    };
    parsed.path().rsplit('/').next().is_some_and(|last| {
        last.rsplit_once('.')
            .is_some_and(|(_, ext)| ext.eq_ignore_ascii_case("pdf"))
    })
}

#[cfg(test)]
mod tests {
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    async fn probe_default(url: String) -> Option<Vec<u8>> {
        let headers = transfer::Headers::new(&http::HeaderMap::new(), None);
        probe(&url::Url::parse(&url).unwrap(), &headers, Duration::from_secs(5)).await
    }

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

    #[tokio::test]
    async fn sends_request_context_to_get() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/protected.pdf"))
            .and(header("user-agent", "TestBot/1.0"))
            .and(header("authorization", "Bearer test"))
            .and(header("cookie", "sid=seed"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(b"PDF".to_vec(), "application/pdf"))
            .expect(1)
            .mount(&server)
            .await;
        let mut headers = http::HeaderMap::new();
        headers.insert("authorization", "Bearer test".parse().unwrap());
        headers.insert("cookie", "sid=seed".parse().unwrap());
        let url = url::Url::parse(&format!("{}/protected.pdf", server.uri())).unwrap();
        let headers = transfer::Headers::new(&headers, Some("TestBot/1.0"));
        let bytes = probe(&url, &headers, Duration::from_secs(5)).await;
        assert_eq!(bytes.as_deref(), Some(&b"PDF"[..]));
    }

    #[tokio::test]
    async fn returns_none_for_html_missing_content_type_or_error_status() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/html.pdf"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(b"<html/>".to_vec(), "text/html"))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/untyped.pdf"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"%PDF-1.4".to_vec()))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/missing.pdf"))
            .respond_with(ResponseTemplate::new(404).set_body_raw(b"%PDF-1.4".to_vec(), "application/pdf"))
            .mount(&server)
            .await;
        for path_name in ["html.pdf", "untyped.pdf", "missing.pdf"] {
            assert!(
                probe_default(format!("{}/{path_name}", server.uri())).await.is_none(),
                "{path_name} must not be treated as a PDF"
            );
        }
    }

    #[tokio::test]
    async fn returns_none_when_the_connection_fails() {
        let closed = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap();
        assert!(probe_default(format!("http://{closed}/foo.pdf")).await.is_none());
    }

    #[tokio::test]
    async fn dropping_probe_mid_body_closes_the_socket() {
        use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
        use tokio::net::TcpListener;
        use tokio::sync::oneshot;

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind PDF fixture");
        let address = listener.local_addr().expect("PDF fixture address");
        let (partial_tx, partial_rx) = oneshot::channel();
        let (closed_tx, closed_rx) = oneshot::channel();
        let fixture = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept PDF request");
            let mut request = Vec::new();
            let mut byte = [0_u8; 1];
            while !request.ends_with(b"\r\n\r\n") {
                socket.read_exact(&mut byte).await.expect("read PDF request");
                request.push(byte[0]);
            }
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\ncontent-type: application/pdf\r\ncontent-length: 1048576\r\n\r\n%PDF-1.4\n",
                )
                .await
                .expect("write partial PDF response");
            socket.flush().await.expect("flush partial PDF response");
            partial_tx.send(()).expect("report partial PDF body");
            let observed = socket.read(&mut byte).await;
            closed_tx.send(observed).expect("report client disconnect");
        });

        let probing = tokio::spawn(probe_default(format!("http://{address}/slow.pdf")));
        partial_rx.await.expect("probe reaches the partial PDF body");
        probing.abort();
        assert!(probing.await.expect_err("probe task is cancelled").is_cancelled());

        let observed = tokio::time::timeout(Duration::from_secs(1), closed_rx)
            .await
            .expect("cancelled probe closes its socket promptly")
            .expect("fixture reports client disconnect");
        match observed {
            Ok(0) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::BrokenPipe
                ) => {}
            other => panic!("cancelled probe must close its response socket, observed {other:?}"),
        }
        fixture.await.expect("PDF fixture task completes");
    }
}
