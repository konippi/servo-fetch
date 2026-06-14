//! Custom request-header integration test.

use servo_fetch::blocking::fetch;
use servo_fetch::{FetchOptions, HeaderMap, NetworkPolicy};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test(flavor = "multi_thread")]
#[ignore = "e2e: spawns the Servo engine; Linux CI sees SIGSEGV during destructor cleanup"]
async fn custom_header_reaches_the_target() {
    servo_fetch::init(NetworkPolicy::PERMISSIVE);

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .and(header("x-test-token", "secret-123"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            b"<!DOCTYPE html><html><body><h1>AUTHED</h1></body></html>".as_slice(),
            "text/html; charset=utf-8",
        ))
        .mount(&server)
        .await;
    let url = format!("{}/", server.uri());

    let html = tokio::task::spawn_blocking(move || {
        let mut headers = HeaderMap::new();
        headers.insert("x-test-token", "secret-123".parse().unwrap());
        fetch(&FetchOptions::new(&url).headers(headers)).expect("fetch").html
    })
    .await
    .expect("spawn_blocking");

    assert!(
        html.contains("AUTHED"),
        "header not delivered or real page not captured:\n{html}"
    );
}
