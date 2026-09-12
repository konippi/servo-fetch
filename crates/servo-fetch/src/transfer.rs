//! Shared reqwest client for transfers that bypass the browser engine (robots.txt, sitemaps, PDFs).

use std::time::Duration;

use futures_util::StreamExt as _;
use reqwest::header::USER_AGENT;
use url::Url;

use crate::{bridge, net};

/// Build a client with the default transfer policy.
pub(crate) fn client() -> crate::error::Result<reqwest::Client> {
    build(reqwest::Client::builder())
}

/// Like [`client`], but bodies arrive undecoded for callers that must decompress exactly once themselves.
pub(crate) fn raw_client() -> crate::error::Result<reqwest::Client> {
    build(reqwest::Client::builder().no_gzip())
}

/// Redirects are disabled for every client; callers follow them manually so each hop is validated.
fn build(builder: reqwest::ClientBuilder) -> crate::error::Result<reqwest::Client> {
    net::ensure_crypto_provider();
    builder
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| crate::Error::engine(error, None))
}

/// Request headers guaranteed to carry a User-Agent: an explicit header wins, then the option, then the engine default.
#[derive(Clone, Debug)]
pub(crate) struct Headers(http::HeaderMap);

impl Headers {
    pub(crate) fn new(base: &http::HeaderMap, user_agent: Option<&str>) -> Self {
        let mut headers = base.clone();
        if !headers.contains_key(USER_AGENT)
            && let Ok(value) = http::HeaderValue::from_str(user_agent.unwrap_or_else(|| bridge::default_user_agent()))
        {
            headers.insert(USER_AGENT, value);
        }
        Self(headers)
    }

    /// The User-Agent these headers announce.
    pub(crate) fn user_agent(&self) -> &str {
        self.0
            .get(USER_AGENT)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_else(|| bridge::default_user_agent())
    }

    /// Start a GET with these headers and the given deadline.
    pub(crate) fn get(&self, client: &reqwest::Client, url: &Url, timeout: Duration) -> reqwest::RequestBuilder {
        client.get(url.clone()).timeout(timeout).headers(self.0.clone())
    }
}

/// Collect a body up to `max_bytes`; larger bodies yield `None` without buffering the excess.
pub(crate) async fn collect_bounded(response: reqwest::Response, max_bytes: u64) -> Option<Vec<u8>> {
    let mut body = Vec::new();
    let mut chunks = response.bytes_stream();
    while let Some(chunk) = chunks.next().await {
        let chunk = chunk.ok()?;
        let next_len = u64::try_from(body.len())
            .ok()?
            .checked_add(u64::try_from(chunk.len()).ok()?)?;
        if next_len > max_bytes {
            return None;
        }
        body.extend_from_slice(&chunk);
    }
    Some(body)
}

#[cfg(test)]
mod tests {
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    fn target(server: &MockServer, path_name: &str) -> Url {
        Url::parse(&format!("{}{path_name}", server.uri())).unwrap()
    }

    fn with_agent(base: &http::HeaderMap, user_agent: Option<&str>) -> Headers {
        Headers::new(base, user_agent)
    }

    #[tokio::test]
    async fn clients_never_follow_redirects() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/hop"))
            .respond_with(ResponseTemplate::new(302).insert_header("location", "/elsewhere"))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/elsewhere"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&server)
            .await;

        let headers = http::HeaderMap::new();
        let response = with_agent(&headers, None)
            .get(&client().unwrap(), &target(&server, "/hop"), Duration::from_secs(5))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 302, "the caller decides whether to follow");
    }

    #[tokio::test]
    async fn user_agent_falls_back_to_the_engine_default_unless_a_header_overrides_it() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/default"))
            .and(header("user-agent", bridge::default_user_agent()))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/header"))
            .and(header("user-agent", "FromHeader/1.0"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/option"))
            .and(header("user-agent", "FromOption/1.0"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;
        let client = client().unwrap();
        let none = http::HeaderMap::new();
        let mut overriding = http::HeaderMap::new();
        overriding.insert(USER_AGENT, "FromHeader/1.0".parse().unwrap());

        let by_default = with_agent(&none, None).get(&client, &target(&server, "/default"), Duration::from_secs(5));
        let by_header = with_agent(&overriding, Some("Ignored/0")).get(
            &client,
            &target(&server, "/header"),
            Duration::from_secs(5),
        );
        let by_option =
            with_agent(&none, Some("FromOption/1.0")).get(&client, &target(&server, "/option"), Duration::from_secs(5));
        assert_eq!(by_default.send().await.unwrap().status(), 200);
        assert_eq!(by_header.send().await.unwrap().status(), 200);
        assert_eq!(by_option.send().await.unwrap().status(), 200);
    }

    #[tokio::test]
    async fn bounded_collection_stops_at_the_limit() {
        async fn collect(server: &MockServer, limit: u64) -> Option<Vec<u8>> {
            let headers = http::HeaderMap::new();
            let response = with_agent(&headers, None)
                .get(&client().unwrap(), &target(server, "/body"), Duration::from_secs(5))
                .send()
                .await
                .unwrap();
            collect_bounded(response, limit).await
        }

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/body"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![b'x'; 64]))
            .mount(&server)
            .await;
        assert_eq!(collect(&server, 64).await.map(|body| body.len()), Some(64));
        assert_eq!(
            collect(&server, 63).await,
            None,
            "one byte over the limit rejects the body"
        );
    }
}
