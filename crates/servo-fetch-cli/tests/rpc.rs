//! Stdio JSON-RPC protocol round-trip tests (engine-free, plus `#[ignore]` engine-backed).

use std::io::{BufRead as _, BufReader, Write as _};
use std::process::{Command as StdCommand, Stdio};
use std::time::Duration;

use assert_cmd::Command;
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

mod common;
use common::mock_page;

fn parse_frames(stdout: Vec<u8>) -> Vec<Value> {
    String::from_utf8(stdout)
        .unwrap()
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn run_rpc(input: &str) -> Vec<Value> {
    let output = Command::cargo_bin("servo-fetch")
        .unwrap()
        .arg("rpc")
        .write_stdin(input)
        .output()
        .unwrap();
    parse_frames(output.stdout)
}

/// Run the server with loopback fetches allowed, for `MockServer`-backed engine tests.
fn run_rpc_loopback(input: &str) -> Vec<Value> {
    let output = Command::cargo_bin("servo-fetch")
        .unwrap()
        .args(["rpc", "--allow-private-addresses"])
        .write_stdin(input)
        .output()
        .unwrap();
    parse_frames(output.stdout)
}

#[test]
fn initialize_returns_server_info() {
    let out = run_rpc("{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\"}\n");
    assert_eq!(out.len(), 1, "expected one response, got: {out:?}");
    assert_eq!(out[0]["id"], 1);
    assert_eq!(out[0]["result"]["serverInfo"]["name"], "servo-fetch");
}

#[test]
fn unknown_method_returns_method_not_found() {
    let out = run_rpc("{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"nope\"}\n");
    assert_eq!(out[0]["id"], 2);
    assert_eq!(out[0]["error"]["code"], -32601);
    assert_eq!(out[0]["error"]["data"]["kind"], "methodNotFound");
}

#[test]
fn malformed_line_returns_parse_error_with_null_id() {
    let out = run_rpc("not json\n");
    assert_eq!(out[0]["error"]["code"], -32700);
    assert!(out[0]["id"].is_null());
    assert_eq!(out[0]["error"]["data"]["kind"], "parseError");
}

#[test]
fn exit_notification_stops_the_server() {
    let out = run_rpc("{\"jsonrpc\":\"2.0\",\"method\":\"exit\"}\n");
    assert!(out.is_empty(), "exit must produce no response, got: {out:?}");
}

#[test]
fn responses_are_correlated_by_id_across_frames() {
    let out = run_rpc(concat!(
        "{\"jsonrpc\":\"2.0\",\"id\":10,\"method\":\"initialize\"}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":11,\"method\":\"initialize\"}\n",
    ));
    assert_eq!(out.len(), 2, "expected two responses, got: {out:?}");
    let ids: Vec<_> = out.iter().map(|m| m["id"].clone()).collect();
    assert!(
        ids.contains(&serde_json::json!(10)) && ids.contains(&serde_json::json!(11)),
        "ids: {ids:?}"
    );
}

#[test]
fn cancel_of_unknown_id_is_ignored() {
    let out = run_rpc(concat!(
        "{\"jsonrpc\":\"2.0\",\"method\":\"$/cancelRequest\",\"params\":{\"id\":999}}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\"}\n",
    ));
    assert_eq!(out.len(), 1, "cancel must not respond; initialize must, got: {out:?}");
    assert_eq!(out[0]["id"], 1);
}

#[test]
fn missing_required_param_is_invalid_params() {
    let out = run_rpc("{\"jsonrpc\":\"2.0\",\"id\":5,\"method\":\"fetch\",\"params\":{}}\n");
    assert_eq!(out[0]["error"]["code"], -32602);
    assert_eq!(out[0]["error"]["data"]["kind"], "invalidParams");
}

#[test]
fn fetch_blocked_address_reports_typed_kind() {
    let out =
        run_rpc("{\"jsonrpc\":\"2.0\",\"id\":6,\"method\":\"fetch\",\"params\":{\"url\":\"http://127.0.0.1/\"}}\n");
    assert!(out[0].get("error").is_some(), "expected an error, got: {out:?}");
    assert_eq!(out[0]["error"]["data"]["kind"], "addressNotAllowed", "got: {out:?}");
}

#[test]
fn blank_lines_are_ignored() {
    let out = run_rpc("\n   \n{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\"}\n");
    assert_eq!(out.len(), 1, "blank lines must be skipped, got: {out:?}");
    assert_eq!(out[0]["id"], 1);
}

#[test]
fn malformed_cancel_is_ignored() {
    let out = run_rpc(concat!(
        "{\"jsonrpc\":\"2.0\",\"method\":\"$/cancelRequest\",\"params\":{}}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\"}\n",
    ));
    assert_eq!(out.len(), 1, "malformed cancel must not respond or crash, got: {out:?}");
    assert_eq!(out[0]["id"], 1);
}

#[tokio::test]
#[ignore = "e2e: requires Servo engine"]
async fn fetch_returns_markdown() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(mock_page(
            "<html><head><title>Test</title></head><body><p>Hello from Servo</p></body></html>",
        ))
        .mount(&server)
        .await;

    let req = json!({"jsonrpc":"2.0","id":1,"method":"fetch","params":{"url":server.uri(),"timeout":30}}).to_string();
    let out = tokio::task::spawn_blocking(move || run_rpc_loopback(&format!("{req}\n")))
        .await
        .unwrap();

    assert_eq!(out.len(), 1, "expected one response, got: {out:?}");
    let md = out[0]["result"]
        .as_str()
        .expect("fetch result should be a markdown string");
    assert!(md.contains("Hello from Servo"), "got: {md}");
}

#[tokio::test]
#[ignore = "e2e: requires Servo engine"]
async fn crawl_streams_progress_then_returns_stats() {
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
            "<html><head><title>Page 2</title></head><body>second</body></html>",
        ))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/robots.txt"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let req =
        json!({"jsonrpc":"2.0","id":2,"method":"crawl","params":{"url":server.uri(),"limit":3,"maxDepth":1,"timeout":30}})
            .to_string();
    let out = tokio::task::spawn_blocking(move || run_rpc_loopback(&format!("{req}\n")))
        .await
        .unwrap();

    let progress: Vec<_> = out.iter().filter(|m| m["method"] == "$/progress").collect();
    assert!(!progress.is_empty(), "expected $/progress notifications, got: {out:?}");
    assert!(
        progress.iter().any(|m| m["params"]["value"]["type"] == "page"),
        "expected at least one page event, got: {progress:?}"
    );
    let response = out.iter().find(|m| m["id"] == 2).expect("final crawl response");
    assert!(
        response["result"]["crawled"].as_u64().unwrap_or(0) >= 1,
        "expected crawled>=1 in CrawlStats, got: {response:?}"
    );
}

#[tokio::test]
#[ignore = "e2e: requires Servo engine"]
async fn cancel_aborts_in_flight_crawl() {
    let server = MockServer::start().await;
    let anchor = format!(r#"<a href="{}/slow">next</a>"#, server.uri());
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(mock_page(format!(
            "<html><head><title>Root</title></head><body>{anchor}</body></html>"
        )))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/slow"))
        .respond_with(mock_page("<html><body>slow</body></html>").set_delay(Duration::from_secs(10)))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/robots.txt"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let uri = server.uri();
    let code = tokio::task::spawn_blocking(move || {
        let mut child = StdCommand::new(env!("CARGO_BIN_EXE_servo-fetch"))
            .args(["rpc", "--allow-private-addresses"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        let mut stdout = BufReader::new(child.stdout.take().unwrap());

        let crawl =
            json!({"jsonrpc":"2.0","id":7,"method":"crawl","params":{"url":uri,"limit":5,"maxDepth":2,"timeout":30}})
                .to_string();
        writeln!(stdin, "{crawl}").unwrap();
        stdin.flush().unwrap();

        // Wait for the root page's progress event, then cancel while /slow is in flight.
        let mut line = String::new();
        loop {
            line.clear();
            assert!(
                stdout.read_line(&mut line).unwrap() > 0,
                "server closed before any progress"
            );
            let frame: Value = serde_json::from_str(line.trim()).unwrap();
            if frame["method"] == "$/progress" {
                break;
            }
        }
        let cancel = json!({"jsonrpc":"2.0","method":"$/cancelRequest","params":{"id":7}}).to_string();
        writeln!(stdin, "{cancel}").unwrap();
        stdin.flush().unwrap();
        drop(stdin);

        let code = loop {
            line.clear();
            if stdout.read_line(&mut line).unwrap() == 0 {
                break None;
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let frame: Value = serde_json::from_str(trimmed).unwrap();
            if frame["id"] == 7 {
                break frame["error"]["code"].as_i64();
            }
        };
        let _ = child.kill();
        let _ = child.wait();
        code
    })
    .await
    .unwrap();

    assert_eq!(code, Some(-32800), "expected RequestCancelled (-32800)");
}
