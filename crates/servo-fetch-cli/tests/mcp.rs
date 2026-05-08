//! MCP server E2E tests.

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
async fn initialize_returns_server_info() {
    let client = connect().await;
    let info = client.peer_info().unwrap();
    assert!(info.server_info.name.contains("servo-fetch"));
    assert!(!info.server_info.version.is_empty());
    assert!(info.instructions.as_deref().unwrap_or("").contains("Servo"));
}

#[tokio::test]
async fn list_tools_returns_expected_tools() {
    let client = connect().await;
    let tools = client.list_tools(None).await.unwrap();

    assert_eq!(tools.tools.len(), 6);
    let names: Vec<&str> = tools.tools.iter().map(|t| t.name.as_ref()).collect();
    assert!(names.contains(&"fetch"));
    assert!(names.contains(&"batch_fetch"));
    assert!(names.contains(&"screenshot"));
    assert!(names.contains(&"execute_js"));
    assert!(names.contains(&"crawl"));
    assert!(names.contains(&"map"));
}

#[tokio::test]
async fn fetch_rejects_private_ip() {
    let client = connect().await;
    let result = client
        .call_tool(call_params("fetch", &serde_json::json!({"url": "http://127.0.0.1/"})))
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn fetch_rejects_missing_url() {
    let client = connect().await;
    let result = client.call_tool(call_params("fetch", &serde_json::json!({}))).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn screenshot_rejects_private_ip() {
    let client = connect().await;
    let result = client
        .call_tool(call_params(
            "screenshot",
            &serde_json::json!({"url": "http://127.0.0.1/"}),
        ))
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn execute_js_rejects_private_ip() {
    let client = connect().await;
    let result = client
        .call_tool(call_params(
            "execute_js",
            &serde_json::json!({"url": "http://127.0.0.1/", "expression": "1+1"}),
        ))
        .await;
    assert!(result.is_err());
}

#[tokio::test]
#[ignore = "requires Servo + network"]
async fn fetch_returns_content() {
    let client = connect().await;
    let result = client
        .call_tool(call_params(
            "fetch",
            &serde_json::json!({"url": "https://example.com", "max_length": 500, "timeout": 60}),
        ))
        .await
        .unwrap();
    assert!(!result.content.is_empty());
}

#[tokio::test]
#[ignore = "requires Servo + network"]
async fn execute_js_returns_title() {
    let client = connect().await;
    let result = client
        .call_tool(call_params(
            "execute_js",
            &serde_json::json!({"url": "https://example.com", "expression": "document.title", "timeout": 60}),
        ))
        .await
        .unwrap();
    assert!(!result.content.is_empty());
}

#[tokio::test]
async fn fetch_rejects_metadata_ip_in_pdf_probe() {
    let client = connect().await;
    let result = client
        .call_tool(call_params(
            "fetch",
            &serde_json::json!({"url": "http://169.254.169.254/latest/meta-data/foo.pdf"}),
        ))
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn batch_fetch_rejects_empty_urls() {
    let client = connect().await;
    let result = client
        .call_tool(call_params("batch_fetch", &serde_json::json!({"urls": []})))
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn batch_fetch_rejects_private_ip() {
    let client = connect().await;
    let result = client
        .call_tool(call_params(
            "batch_fetch",
            &serde_json::json!({"urls": ["http://127.0.0.1/"]}),
        ))
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn crawl_rejects_private_ip() {
    let client = connect().await;
    let result = client
        .call_tool(call_params("crawl", &serde_json::json!({"url": "http://127.0.0.1/"})))
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn crawl_rejects_missing_url() {
    let client = connect().await;
    let result = client.call_tool(call_params("crawl", &serde_json::json!({}))).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn crawl_rejects_file_scheme() {
    let client = connect().await;
    let result = client
        .call_tool(call_params("crawl", &serde_json::json!({"url": "file:///etc/passwd"})))
        .await;
    assert!(result.is_err());
}

#[tokio::test]
#[ignore = "requires Servo + network"]
async fn crawl_returns_multiple_pages() {
    let client = connect().await;
    let result = client
        .call_tool(call_params(
            "crawl",
            &serde_json::json!({
                "url": "https://example.com",
                "limit": 3,
                "max_depth": 1,
                "timeout": 60
            }),
        ))
        .await
        .unwrap();
    assert!(!result.content.is_empty(), "crawl should return at least the seed page");
}

#[tokio::test]
#[ignore = "requires Servo + network"]
async fn batch_fetch_returns_multiple_results() {
    let client = connect().await;
    let result = client
        .call_tool(call_params(
            "batch_fetch",
            &serde_json::json!({
                "urls": ["https://example.com", "https://www.iana.org/help/example-domains"],
                "max_length": 500,
                "timeout": 60
            }),
        ))
        .await
        .unwrap();
    assert_eq!(result.content.len(), 2, "should return one content entry per URL");
}
