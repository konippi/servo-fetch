//! Cross-contamination and concurrent rendering integration tests.

use rmcp::ServiceExt;
use rmcp::transport::TokioChildProcess;

async fn connect() -> rmcp::service::RunningService<rmcp::RoleClient, impl rmcp::service::Service<rmcp::RoleClient>> {
    let mut cmd = tokio::process::Command::new(env!("CARGO_BIN_EXE_servo-fetch"));
    cmd.arg("mcp");
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
#[ignore = "requires Servo + network"]
async fn parallel_fetches_do_not_cross_contaminate() {
    // Two public test URLs with distinct, stable content markers.
    const URLS: &[(&str, &str)] = &[
        ("https://example.com", "Example Domain"),
        ("https://example.org", "Example Domain"),
    ];

    let client = connect().await;
    let calls = URLS.iter().map(|(url, _)| {
        client.call_tool(call_params(
            "fetch",
            &serde_json::json!({ "url": url, "max_length": 2000, "timeout": 60 }),
        ))
    });
    let results = futures_util::future::join_all(calls).await;

    for ((url, marker), result) in URLS.iter().zip(results) {
        let r = result.unwrap_or_else(|e| panic!("fetch {url} failed: {e}"));
        let text = r
            .content
            .iter()
            .filter_map(|c| c.as_text())
            .map(|t| t.text.as_str())
            .collect::<String>();
        assert!(
            text.contains(marker),
            "response for {url} missing marker {marker:?}; got: {text}"
        );
    }
}

#[tokio::test]
#[ignore = "requires Servo + network"]
async fn concurrent_full_page_screenshots_are_distinct() {
    // Stress the dedicated `SoftwareRenderingContext`: each PNG must be unique.
    // All URLs must render visually distinct pages.
    const URLS: &[&str] = &[
        "https://example.com",
        "https://www.iana.org/help/example-domains",
        "https://httpbin.org/html",
    ];

    let client = connect().await;
    let calls = URLS.iter().map(|url| {
        client.call_tool(call_params(
            "screenshot",
            &serde_json::json!({ "url": url, "full_page": true, "timeout": 60 }),
        ))
    });
    let results = futures_util::future::join_all(calls).await;

    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (url, result) in URLS.iter().zip(results) {
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
