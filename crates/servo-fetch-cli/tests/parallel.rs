//! Cross-contamination and concurrent rendering integration tests.

use rmcp::ServiceExt;
use rmcp::transport::TokioChildProcess;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer};

mod common;
use common::mock_page;

async fn connect() -> rmcp::service::RunningService<rmcp::RoleClient, impl rmcp::service::Service<rmcp::RoleClient>> {
    let mut cmd = tokio::process::Command::new(env!("CARGO_BIN_EXE_servo-fetch"));
    cmd.args(["mcp", "--allow-private-addresses"]);
    let transport = TokioChildProcess::new(cmd).unwrap();
    ().serve(transport).await.expect("MCP handshake failed")
}

fn call_params(name: &str, args: &serde_json::Value) -> rmcp::model::CallToolRequestParams {
    let mut params = rmcp::model::CallToolRequestParams::default();
    params.name = String::from(name).into();
    params.arguments = Some(args.as_object().unwrap().clone());
    params
}

#[tokio::test]
#[ignore = "e2e: requires Servo engine"]
async fn parallel_fetches_do_not_cross_contaminate() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/alpha"))
        .respond_with(mock_page(
            "<html><head><title>Alpha</title></head><body><h1>Alpha Marker</h1></body></html>",
        ))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/beta"))
        .respond_with(mock_page(
            "<html><head><title>Beta</title></head><body><h1>Beta Marker</h1></body></html>",
        ))
        .mount(&server)
        .await;

    let cases = [
        (format!("{}/alpha", server.uri()), "Alpha Marker", "Beta Marker"),
        (format!("{}/beta", server.uri()), "Beta Marker", "Alpha Marker"),
    ];

    let client = connect().await;
    let calls = cases.iter().map(|(url, _, _)| {
        client.call_tool(call_params(
            "fetch",
            &serde_json::json!({ "url": url, "max_length": 2000, "timeout": 30 }),
        ))
    });
    let results = futures_util::future::join_all(calls).await;

    for ((url, expected, forbidden), result) in cases.iter().zip(results) {
        let r = result.unwrap_or_else(|e| panic!("fetch {url} failed: {e}"));
        let text = r
            .content
            .iter()
            .filter_map(|c| c.as_text())
            .map(|t| t.text.as_str())
            .collect::<String>();
        assert!(
            text.contains(expected),
            "response for {url} missing {expected:?}; got: {text}"
        );
        assert!(
            !text.contains(forbidden),
            "response for {url} leaked {forbidden:?} from a sibling request"
        );
    }
}

#[tokio::test]
#[ignore = "e2e: requires Servo engine"]
async fn concurrent_full_page_screenshots_are_distinct() {
    let server = MockServer::start().await;
    let pages = [
        (
            "/red",
            "<html><body style=\"background:#ff0000;height:800px\"><h1>RED</h1></body></html>",
        ),
        (
            "/green",
            "<html><body style=\"background:#00ff00;height:800px\"><h1>GREEN</h1></body></html>",
        ),
        (
            "/blue",
            "<html><body style=\"background:#0000ff;height:800px\"><h1>BLUE</h1></body></html>",
        ),
    ];
    for (p, html) in &pages {
        Mock::given(method("GET"))
            .and(path(*p))
            .respond_with(mock_page(*html))
            .mount(&server)
            .await;
    }

    let urls: Vec<String> = pages.iter().map(|(p, _)| format!("{}{p}", server.uri())).collect();

    let client = connect().await;
    let calls = urls.iter().map(|url| {
        client.call_tool(call_params(
            "screenshot",
            &serde_json::json!({ "url": url, "full_page": true, "timeout": 30 }),
        ))
    });
    let results = futures_util::future::join_all(calls).await;

    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (url, result) in urls.iter().zip(results) {
        let r = result.unwrap_or_else(|e| panic!("screenshot {url} failed: {e}"));
        let png_b64 = r
            .content
            .iter()
            .find_map(|c| c.as_image().map(|i| i.data.clone()))
            .unwrap_or_else(|| panic!("no image content for {url}"));
        assert!(
            seen.insert(png_b64),
            "duplicate screenshot for {url}; framebuffer likely leaked across WebViews"
        );
    }
}
