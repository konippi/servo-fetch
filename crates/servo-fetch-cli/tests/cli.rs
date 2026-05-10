//! CLI integration tests.

use std::future::Future;

use assert_cmd::Command;
use predicates::prelude::*;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

mod common;
use common::mock_page;

fn servo_fetch() -> Command {
    Command::cargo_bin("servo-fetch").expect("binary exists")
}

fn block_on<F: Future>(f: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build current-thread runtime")
        .block_on(f)
}

#[test]
fn no_args_shows_error() {
    servo_fetch()
        .assert()
        .failure()
        .stderr(predicate::str::contains("URL is required"));
}

#[test]
fn invalid_url_shows_error() {
    servo_fetch()
        .arg("not-a-url")
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid URL"));
}

#[test]
fn file_scheme_rejected() {
    servo_fetch()
        .arg("file:///etc/passwd")
        .assert()
        .failure()
        .stderr(predicate::str::contains("not allowed"));
}

#[test]
fn javascript_scheme_rejected() {
    servo_fetch()
        .arg("javascript:alert(1)")
        .assert()
        .failure()
        .stderr(predicate::str::contains("not allowed"));
}

#[test]
fn version_flag() {
    servo_fetch()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("servo-fetch"));
}

#[test]
fn help_flag() {
    servo_fetch()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("browser engine in a binary"));
}

const TIMEOUT: &str = "--timeout=30";

#[test]
#[ignore = "e2e: requires Servo engine"]
fn default_produces_markdown() {
    block_on(async {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/"))
            .respond_with(mock_page(
                "<html><head><title>Test</title></head><body><h1>Hello Servo</h1></body></html>",
            ))
            .mount(&s)
            .await;
        servo_fetch()
            .args(["--allow-private-addresses", TIMEOUT, &s.uri()])
            .assert()
            .success()
            .stdout(predicate::str::contains("Hello Servo"));
    });
}

#[test]
#[ignore = "e2e: requires Servo engine"]
fn json_produces_valid_json() {
    block_on(async {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/"))
            .respond_with(mock_page(
                "<html><head><title>JSON Test</title></head><body>content</body></html>",
            ))
            .mount(&s)
            .await;
        let output = servo_fetch()
            .args(["--json", "--allow-private-addresses", TIMEOUT, &s.uri()])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let parsed: serde_json::Value = serde_json::from_slice(&output).expect("valid JSON");
        assert!(parsed.get("title").is_some());
    });
}

#[test]
#[ignore = "e2e: requires Servo engine"]
fn js_eval_returns_result() {
    block_on(async {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/"))
            .respond_with(mock_page(
                "<html><head><title>JS Eval</title></head><body></body></html>",
            ))
            .mount(&s)
            .await;
        servo_fetch()
            .args(["--js", "document.title", "--allow-private-addresses", TIMEOUT, &s.uri()])
            .assert()
            .success();
    });
}

#[test]
#[ignore = "e2e: requires Servo engine"]
fn screenshot_creates_file() {
    block_on(async {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/"))
            .respond_with(mock_page("<html><body><h1>Screenshot</h1></body></html>"))
            .mount(&s)
            .await;
        let dir = std::env::temp_dir().join("servo-fetch-e2e");
        std::fs::create_dir_all(&dir).ok();
        let file = dir.join("test.png");
        servo_fetch()
            .args([
                "--screenshot",
                file.to_str().unwrap(),
                "--allow-private-addresses",
                TIMEOUT,
                &s.uri(),
            ])
            .assert()
            .success();
        assert!(file.exists());
        assert!(file.metadata().unwrap().len() > 0);
        std::fs::remove_file(&file).ok();
    });
}

#[test]
#[ignore = "e2e: requires Servo engine"]
fn crawl_produces_ndjson() {
    block_on(async {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/"))
            .respond_with(mock_page(
                "<html><head><title>Crawl</title></head><body><p>Root</p></body></html>",
            ))
            .mount(&s)
            .await;
        Mock::given(method("GET"))
            .and(path("/robots.txt"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&s)
            .await;
        let output = servo_fetch()
            .args([
                "crawl",
                &s.uri(),
                "--json",
                "--limit",
                "1",
                "--timeout",
                "30",
                "--allow-private-addresses",
            ])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let first_line = std::str::from_utf8(&output)
            .unwrap()
            .lines()
            .next()
            .expect("expected NDJSON output");
        let parsed: serde_json::Value = serde_json::from_str(first_line).expect("valid NDJSON");
        assert_eq!(parsed["status"], "ok");
    });
}

#[test]
#[ignore = "e2e: requires Servo engine"]
fn crawl_selector_scopes_content() {
    block_on(async {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/"))
            .respond_with(mock_page(
                "<html><head><title>T</title></head><body><h1>Kept</h1><p>Dropped paragraph</p></body></html>",
            ))
            .mount(&s)
            .await;
        Mock::given(method("GET"))
            .and(path("/robots.txt"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&s)
            .await;
        let output = servo_fetch()
            .args([
                "crawl",
                &s.uri(),
                "--selector",
                "h1",
                "--limit",
                "1",
                "--max-depth",
                "0",
                "--timeout",
                "30",
                "--allow-private-addresses",
            ])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let text = std::str::from_utf8(&output).unwrap();
        assert!(text.contains("Kept"), "selector should keep h1 text, got: {text}");
        assert!(
            !text.contains("Dropped paragraph"),
            "selector should scope away from <p>, got: {text}"
        );
    });
}

#[test]
#[ignore = "e2e: requires Servo engine"]
fn crawl_json_embeds_structured_content() {
    block_on(async {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/"))
            .respond_with(mock_page(
                "<html><head><title>JSONCrawl</title></head><body><article>Structured body content for extraction.</article></body></html>",
            ))
            .mount(&s)
            .await;
        Mock::given(method("GET"))
            .and(path("/robots.txt"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&s)
            .await;
        let output = servo_fetch()
            .args([
                "crawl",
                &s.uri(),
                "--json",
                "--limit",
                "1",
                "--max-depth",
                "0",
                "--timeout",
                "30",
                "--allow-private-addresses",
            ])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let line = std::str::from_utf8(&output)
            .unwrap()
            .lines()
            .next()
            .expect("expected NDJSON line");
        let outer: serde_json::Value = serde_json::from_str(line).expect("outer NDJSON must parse");
        let content_str = outer["content"].as_str().expect("content must be a string");
        let inner: serde_json::Value =
            serde_json::from_str(content_str).expect("content must be structured JSON, not markdown");
        assert!(
            inner.get("title").is_some(),
            "inner JSON should have a 'title' field, got: {content_str}"
        );
    });
}

#[test]
fn crawl_rejects_private_ip() {
    servo_fetch()
        .args(["crawl", "http://127.0.0.1/"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not allowed"));
}

#[test]
fn crawl_rejects_file_scheme() {
    servo_fetch()
        .args(["crawl", "file:///etc/passwd"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not allowed"));
}

#[test]
fn crawl_rejects_invalid_url() {
    servo_fetch()
        .args(["crawl", "not-a-url"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid URL"));
}

#[test]
fn crawl_help_shows_options() {
    servo_fetch()
        .args(["crawl", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--limit"))
        .stdout(predicate::str::contains("--max-depth"))
        .stdout(predicate::str::contains("--include"))
        .stdout(predicate::str::contains("--exclude"));
}

#[test]
fn mcp_help_shows_options() {
    servo_fetch()
        .args(["mcp", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("MCP"))
        .stdout(predicate::str::contains("--port"));
}
