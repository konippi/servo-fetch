//! MCP server E2E tests.

use rmcp::ServiceExt;
use rmcp::transport::TokioChildProcess;
use tokio::process::Command;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

mod common;
use common::{mock_page, slow_page};

async fn connect() -> rmcp::service::RunningService<rmcp::RoleClient, impl rmcp::service::Service<rmcp::RoleClient>> {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_servo-fetch"));
    cmd.arg("mcp");
    let transport = TokioChildProcess::new(cmd).unwrap();
    ().serve(transport).await.expect("MCP handshake failed")
}

async fn connect_loopback()
-> rmcp::service::RunningService<rmcp::RoleClient, impl rmcp::service::Service<rmcp::RoleClient>> {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_servo-fetch"));
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

fn assert_tool_error<E: std::fmt::Debug>(result: Result<rmcp::model::CallToolResult, E>) {
    let result = result.expect("expected an isError tool result, not a protocol error");
    assert_eq!(result.is_error, Some(true), "expected isError tool result");
}

/// Direct `__worker` children of the MCP server process.
#[cfg(unix)]
fn worker_children(server: u32) -> Vec<i32> {
    let output = std::process::Command::new("pgrep")
        .args(["-P", &server.to_string(), "-f", "__worker"])
        .output()
        .expect("inspect worker processes");
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.trim().parse().ok())
        .collect()
}

/// Wait until the PID is fully gone (reaped, not merely killed), like the library session tests.
#[cfg(unix)]
async fn wait_for_exit(pid: i32) {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        // SAFETY: signal 0 only probes the PID captured from our own MCP server's child.
        #[allow(unsafe_code)]
        let alive = unsafe { libc::kill(pid, 0) } == 0;
        if !alive {
            assert_eq!(
                std::io::Error::last_os_error().raw_os_error(),
                Some(libc::ESRCH),
                "probing worker {pid} must fail only because it no longer exists"
            );
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "worker {pid} survived cancellation"
        );
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}

#[tokio::test]
async fn initialize_returns_server_info() {
    let client = connect().await;
    let info = client.peer_info().unwrap();
    let server_info = info
        .server_info
        .as_ref()
        .expect("server should advertise its implementation info");
    assert!(server_info.name.contains("servo-fetch"));
    assert!(!server_info.version.is_empty());
    assert!(info.instructions.as_deref().unwrap_or("").contains("Servo"));
}

#[tokio::test]
async fn list_tools_preserves_required_and_representative_camel_case_fields() {
    let client = connect().await;
    let listed = client.list_tools(None).await.unwrap();

    assert_eq!(listed.tools.len(), 6);
    let expectations = [
        ("fetch", &["url"][..], &["maxLength", "startIndex"][..]),
        ("batch_fetch", &["urls"][..], &["maxLength", "settleMs"][..]),
        ("screenshot", &["url"][..], &["fullPage", "userAgent"][..]),
        ("execute_js", &["url", "expression"][..], &["settleMs"][..]),
        ("crawl", &["url"][..], &["maxDepth", "delayMs", "maxLength"][..]),
        ("map", &["url"][..], &["noFallback", "userAgent"][..]),
    ];

    for (name, required, properties) in expectations {
        let tool = listed
            .tools
            .iter()
            .find(|tool| tool.name == name)
            .unwrap_or_else(|| panic!("missing {name} tool"));
        let actual_required = tool.input_schema["required"]
            .as_array()
            .expect("schema required fields")
            .iter()
            .map(|field| field.as_str().expect("required field name"))
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            actual_required,
            required.iter().copied().collect(),
            "{name} required fields"
        );
        let actual_properties = tool.input_schema["properties"].as_object().expect("schema properties");
        for property in properties {
            assert!(actual_properties.contains_key(*property), "{name} missing {property}");
        }
    }
}

#[tokio::test]
async fn fetch_rejects_private_ip() {
    let client = connect().await;
    let result = client
        .call_tool(call_params("fetch", &serde_json::json!({"url": "http://127.0.0.1/"})))
        .await;
    assert_tool_error(result);
}

#[tokio::test]
async fn unknown_tool_remains_invalid_params_protocol_error() {
    let client = connect().await;
    let error = client
        .call_tool(call_params("unknown", &serde_json::json!({})))
        .await
        .expect_err("unknown tool should be a protocol error");

    match error {
        rmcp::ServiceError::McpError(error) => {
            assert_eq!(error.code, rmcp::model::ErrorCode::INVALID_PARAMS);
        }
        other => panic!("expected MCP protocol error, got {other:?}"),
    }
}

#[tokio::test]
async fn malformed_arguments_for_all_tools_are_bounded_sanitized_tool_errors() {
    let cases = [
        ("fetch", serde_json::json!({"url": "https://example.com", "format": 1})),
        (
            "screenshot",
            serde_json::json!({"url": "https://example.com", "fullPage": "yes"}),
        ),
        (
            "execute_js",
            serde_json::json!({"url": "https://example.com", "expression": 1}),
        ),
        ("batch_fetch", serde_json::json!({"urls": "https://example.com"})),
        (
            "crawl",
            serde_json::json!({"url": "https://example.com", "limit": "many"}),
        ),
        (
            "map",
            serde_json::json!({"url": "https://example.com", "limit": "many"}),
        ),
    ];

    let client = connect().await;
    for (tool, arguments) in cases {
        let result = client
            .call_tool(call_params(tool, &arguments))
            .await
            .unwrap_or_else(|error| panic!("{tool} malformed arguments returned a protocol error: {error:?}"));
        assert_eq!(result.is_error, Some(true), "{tool}");
        assert!(serde_json::to_vec(&result).unwrap().len() <= 1_000_000, "{tool}");
    }

    let malformed = format!("bad\u{1b}[31mvisible{}", "x".repeat(1_100_000));
    let result = client
        .call_tool(call_params(
            "fetch",
            &serde_json::json!({"url": "https://example.com", "format": malformed}),
        ))
        .await
        .expect("malformed fetch arguments should be an isError tool result");
    let serialized = serde_json::to_vec(&result).unwrap();
    let text = result.content[0].as_text().expect("text error block");
    assert_eq!(result.is_error, Some(true));
    assert!(serialized.len() <= 1_000_000);
    assert!(!text.text.contains('\u{1b}'));
    assert!(text.text.contains("visible"));
    assert!(text.text.ends_with("<output truncated>"));
}

#[tokio::test]
async fn fetch_rejects_missing_url() {
    let client = connect().await;
    let result = client.call_tool(call_params("fetch", &serde_json::json!({}))).await;
    assert_tool_error(result);
}

#[tokio::test]
async fn fetch_sanitizes_controls_in_malformed_url_error() {
    let client = connect().await;
    let result = client
        .call_tool(call_params(
            "fetch",
            &serde_json::json!({
                "url": "not a \u{1b}[31mURL\u{1b}[0m/visible\u{202e}\u{0}"
            }),
        ))
        .await
        .expect("expected an isError tool result, not a protocol error");

    assert_eq!(result.is_error, Some(true));
    let text = result.content[0].as_text().expect("text error block");
    assert!(text.text.starts_with("invalid URL 'not a URL/visible"));
    assert!(!text.text.contains('\u{1b}'));
    assert!(!text.text.contains('\u{202e}'));
    assert!(!text.text.contains('\u{0}'));
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
    assert_tool_error(result);
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
    assert_tool_error(result);
}

#[tokio::test]
#[ignore = "e2e: requires Servo engine"]
async fn execute_js_returns_title() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(mock_page(
            "<html><head><title>JS Title</title></head><body></body></html>",
        ))
        .mount(&server)
        .await;

    let client = connect_loopback().await;
    let result = client
        .call_tool(call_params(
            "execute_js",
            &serde_json::json!({"url": server.uri(), "expression": "document.title", "timeout": 30}),
        ))
        .await
        .unwrap();
    assert!(!result.content.is_empty());
}

#[tokio::test]
async fn batch_and_crawl_invalid_shared_options_remain_tool_errors() {
    let cases = [
        (
            "batch_fetch",
            serde_json::json!({
                "urls": ["https://example.com"],
                "headers": {"bad\nname": "value"}
            }),
        ),
        (
            "crawl",
            serde_json::json!({
                "url": "https://example.com",
                "headers": {"bad\nname": "value"}
            }),
        ),
    ];

    let client = connect().await;
    for (tool, arguments) in cases {
        let result = client
            .call_tool(call_params(tool, &arguments))
            .await
            .unwrap_or_else(|error| panic!("{tool} invalid input returned a protocol error: {error:?}"));
        assert_eq!(result.is_error, Some(true), "{tool}");
    }
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
    assert_tool_error(result);
}

#[tokio::test]
async fn batch_fetch_rejects_empty_urls() {
    let client = connect().await;
    let result = client
        .call_tool(call_params("batch_fetch", &serde_json::json!({"urls": []})))
        .await;
    assert_tool_error(result);
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
    assert_tool_error(result);
}

#[tokio::test]
async fn crawl_rejects_private_ip() {
    let client = connect().await;
    let result = client
        .call_tool(call_params("crawl", &serde_json::json!({"url": "http://127.0.0.1/"})))
        .await;
    assert_tool_error(result);
}

#[tokio::test]
async fn crawl_rejects_missing_url() {
    let client = connect().await;
    let result = client.call_tool(call_params("crawl", &serde_json::json!({}))).await;
    assert_tool_error(result);
}

#[tokio::test]
async fn crawl_rejects_file_scheme() {
    let client = connect().await;
    let result = client
        .call_tool(call_params("crawl", &serde_json::json!({"url": "file:///etc/passwd"})))
        .await;
    assert_tool_error(result);
}

#[tokio::test]
#[ignore = "e2e: requires Servo engine"]
async fn crawl_invalid_selector_is_a_tool_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(mock_page("<html><body><p>content</p></body></html>"))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/robots.txt"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let client = connect_loopback().await;
    let result = client
        .call_tool(call_params(
            "crawl",
            &serde_json::json!({
                "url": server.uri(),
                "selector": "[",
                "limit": 1,
                "delayMs": 0,
                "timeout": 30
            }),
        ))
        .await
        .expect("invalid crawl selector should be a tool result");

    assert_eq!(result.is_error, Some(true));
    let text = result.content[0].as_text().expect("text error block");
    assert!(text.text.contains("invalid CSS selector"));
}

#[tokio::test]
#[ignore = "e2e: requires Servo engine"]
async fn crawl_returns_multiple_pages() {
    let server = MockServer::start().await;
    let link = format!(r#"<a href="{}/page2">next</a>"#, server.uri());
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(mock_page(format!(
            "<html><head><title>Root</title></head><body>{link}</body></html>"
        )))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/page2"))
        .respond_with(mock_page(
            "<html><head><title>Page 2</title></head><body><p>Second page</p></body></html>",
        ))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/robots.txt"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let client = connect_loopback().await;
    let result = client
        .call_tool(call_params(
            "crawl",
            &serde_json::json!({
                "url": server.uri(),
                "limit": 3,
                "maxDepth": 1,
                "timeout": 30
            }),
        ))
        .await
        .expect("crawl tool call failed");
    assert!(!result.content.is_empty());
    for block in &result.content {
        let text = block.as_text().expect("crawl content should be text");
        assert!(
            text.text.starts_with("URL: ") || text.text.contains("response truncated"),
            "crawl page blocks should be URL-labeled"
        );
    }
}

#[tokio::test]
#[ignore = "e2e: requires Servo engine"]
async fn batch_fetch_returns_multiple_results() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/a"))
        .respond_with(mock_page(
            "<html><head><title>A</title></head><body>Page A</body></html>",
        ))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/b"))
        .respond_with(mock_page(
            "<html><head><title>B</title></head><body>Page B</body></html>",
        ))
        .mount(&server)
        .await;

    let client = connect_loopback().await;
    let result = client
        .call_tool(call_params(
            "batch_fetch",
            &serde_json::json!({
                "urls": [format!("{}/a", server.uri()), format!("{}/b", server.uri())],
                "timeout": 30
            }),
        ))
        .await
        .unwrap();
    assert_eq!(result.content.len(), 2, "should return one content entry per URL");
    let texts: Vec<&str> = result
        .content
        .iter()
        .map(|block| block.as_text().expect("batch content should be text").text.as_str())
        .collect();
    assert!(
        texts
            .iter()
            .any(|text| text.starts_with(&format!("URL: {}/a", server.uri())))
    );
    assert!(
        texts
            .iter()
            .any(|text| text.starts_with(&format!("URL: {}/b", server.uri())))
    );
}

#[tokio::test]
#[ignore = "e2e: requires Servo engine"]
async fn fetch_session_applies_user_agent_and_seeded_cookie() {
    use std::io::Write as _;

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/identity"))
        .and(header("user-agent", "McpSessionIdentity/1.0"))
        .and(header("cookie", "mcp_session=seeded"))
        .respond_with(mock_page("<html><body>session identity applied</body></html>"))
        .expect(1)
        .mount(&server)
        .await;
    let mut cookies = tempfile::NamedTempFile::new().expect("cookie file");
    writeln!(cookies, "127.0.0.1\tFALSE\t/\tFALSE\t0\tmcp_session\tseeded").expect("cookie fixture");

    let client = connect_loopback().await;
    let result = client
        .call_tool(call_params(
            "fetch",
            &serde_json::json!({
                "url": format!("{}/identity", server.uri()),
                "format": "text",
                "userAgent": "McpSessionIdentity/1.0",
                "cookiesFile": cookies.path(),
                "timeout": 30
            }),
        ))
        .await
        .expect("session fetch succeeds with configured identity");
    let text = result.content[0].as_text().expect("text content");
    assert!(text.text.contains("session identity applied"));
}

/// Cancel `tool` while `workers` isolated workers are mid-fetch; every worker must exit and the slots must free up.
#[cfg(unix)]
async fn cancel_kills_workers(tool: &str, arguments: serde_json::Value, workers: usize, server: &MockServer) {
    use std::time::Duration;

    use rmcp::model::{CallToolRequest, ClientRequest};
    use rmcp::service::PeerRequestOptions;

    let (mut arrivals, slow) = slow_page("<html><body>slow</body></html>", Duration::from_secs(30));
    Mock::given(method("GET"))
        .and(path("/slow"))
        .respond_with(slow)
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/fast"))
        .respond_with(mock_page("<html><body>fast page</body></html>"))
        .mount(server)
        .await;

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_servo-fetch"));
    cmd.args(["mcp", "--allow-private-addresses"])
        .env("SERVO_FETCH_MAX_WORKERS", workers.to_string())
        .env("SERVO_FETCH_PREWARM", "0");
    let transport = TokioChildProcess::new(cmd).unwrap();
    let server_pid = transport.id().expect("MCP server pid");
    let client = ().serve(transport).await.expect("MCP handshake failed");

    let slow = client
        .peer()
        .send_cancellable_request(
            ClientRequest::CallToolRequest(CallToolRequest::new(call_params(tool, &arguments))),
            PeerRequestOptions::no_options(),
        )
        .await
        .expect("start slow call");
    for _ in 0..workers {
        tokio::time::timeout(Duration::from_secs(15), arrivals.recv())
            .await
            .expect("every worker navigates to the slow page before cancellation");
    }
    let pids = worker_children(server_pid);
    assert_eq!(pids.len(), workers, "one in-flight worker per slot, found {pids:?}");

    slow.cancel(None).await.expect("send cancellation");
    for pid in pids {
        wait_for_exit(pid).await;
    }

    let fast = call_params(
        "fetch",
        &serde_json::json!({"url": format!("{}/fast", server.uri()), "format": "text", "timeout": 30}),
    );
    let result = tokio::time::timeout(Duration::from_secs(15), client.call_tool(fast))
        .await
        .expect("freed slots admit the follow-up fetch")
        .expect("follow-up fetch succeeds");
    let text = result.content[0].as_text().expect("text content");
    assert!(text.text.contains("fast page"), "follow-up fetch renders the fast page");
}

#[cfg(unix)]
#[tokio::test]
#[ignore = "e2e: requires Servo engine"]
async fn cancelled_fetch_kills_its_worker_and_frees_the_slot() {
    let server = MockServer::start().await;
    let args = serde_json::json!({"url": format!("{}/slow", server.uri()), "format": "text", "timeout": 30});
    cancel_kills_workers("fetch", args, 1, &server).await;
}

#[cfg(unix)]
#[tokio::test]
#[ignore = "e2e: requires Servo engine"]
async fn cancelled_batch_fetch_kills_every_worker_and_frees_the_slots() {
    let server = MockServer::start().await;
    let args = serde_json::json!({
        "urls": [format!("{}/slow?a", server.uri()), format!("{}/slow?b", server.uri())],
        "format": "text",
        "timeout": 30
    });
    cancel_kills_workers("batch_fetch", args, 2, &server).await;
}

#[cfg(unix)]
#[tokio::test]
#[ignore = "e2e: requires Servo engine"]
async fn cancelled_crawl_kills_its_worker_and_frees_the_slot() {
    let server = MockServer::start().await;
    let args = serde_json::json!({"url": format!("{}/slow", server.uri()), "limit": 1, "timeout": 30});
    cancel_kills_workers("crawl", args, 1, &server).await;
}

#[tokio::test]
#[ignore = "e2e: requires Servo engine"]
async fn pdf_suffix_serving_html_still_renders_through_the_one_shot_path() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/report.pdf"))
        .respond_with(mock_page("<html><body>rendered fallback</body></html>"))
        .mount(&server)
        .await;

    let client = connect_loopback().await;
    let result = client
        .call_tool(call_params(
            "fetch",
            &serde_json::json!({"url": format!("{}/report.pdf", server.uri()), "format": "text", "timeout": 30}),
        ))
        .await
        .expect("PDF-suffixed URL is fetched");
    assert_ne!(
        result.is_error,
        Some(true),
        "PDF URLs must not be rejected by the session path"
    );
    let text = result.content[0].as_text().expect("text content");
    assert!(text.text.contains("rendered fallback"));
}
