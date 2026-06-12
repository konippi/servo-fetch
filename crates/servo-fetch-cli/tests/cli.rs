//! CLI integration tests.

use std::fs;
use std::future::Future;
use std::str::from_utf8;

use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;
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
            .args(["--format", "json", "--allow-private-addresses", TIMEOUT, &s.uri()])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let parsed: Value = serde_json::from_slice(&output).expect("valid JSON");
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
fn format_png_creates_file() {
    block_on(async {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/"))
            .respond_with(mock_page("<html><body><h1>Screenshot</h1></body></html>"))
            .mount(&s)
            .await;
        let dir = std::env::temp_dir().join("servo-fetch-e2e");
        fs::create_dir_all(&dir).ok();
        let file = dir.join("test.png");
        servo_fetch()
            .args([
                "--format",
                "png",
                "-o",
                file.to_str().unwrap(),
                "--allow-private-addresses",
                TIMEOUT,
                &s.uri(),
            ])
            .assert()
            .success();
        assert!(file.exists());
        assert!(file.metadata().unwrap().len() > 0);
        fs::remove_file(&file).ok();
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
                "--format",
                "json",
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
        let lines: Vec<&str> = from_utf8(&output).unwrap().lines().collect();
        assert!(lines.len() >= 2, "expected page + stats record, got: {lines:?}");
        let page: Value = serde_json::from_str(lines[0]).expect("valid NDJSON");
        assert_eq!(page["type"], "page");
        assert!(page["fetchedAt"].is_string(), "fetchedAt must be present");
        let stats: Value = serde_json::from_str(lines.last().unwrap()).expect("valid stats NDJSON");
        assert_eq!(stats["type"], "stats");
        assert!(stats["crawled"].as_u64().is_some_and(|n| n >= 1));
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
        let text = from_utf8(&output).unwrap();
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
                "--format",
                "json",
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
        let line = from_utf8(&output)
            .unwrap()
            .lines()
            .next()
            .expect("expected NDJSON line");
        let outer: Value = serde_json::from_str(line).expect("outer NDJSON must parse");
        let content_str = outer["content"].as_str().expect("content must be a string");
        let inner: Value = serde_json::from_str(content_str).expect("content must be structured JSON, not markdown");
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
#[ignore = "e2e: requires Servo engine"]
fn output_writes_single_file() {
    block_on(async {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/"))
            .respond_with(mock_page(
                "<html><head><title>OutputFile</title></head><body><h1>Direct</h1></body></html>",
            ))
            .mount(&s)
            .await;
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("page.md");
        servo_fetch()
            .args([
                "-o",
                file.to_str().unwrap(),
                "--allow-private-addresses",
                TIMEOUT,
                &s.uri(),
            ])
            .assert()
            .success();
        let body = fs::read_to_string(&file).expect("file written");
        assert!(body.contains("Direct"), "got: {body}");
    });
}

#[test]
#[ignore = "e2e: requires Servo engine"]
fn output_dir_writes_single_file() {
    block_on(async {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/"))
            .respond_with(mock_page(
                "<html><head><title>OutputDir</title></head><body><h1>Saved</h1></body></html>",
            ))
            .mount(&s)
            .await;
        let dir = tempfile::tempdir().expect("tempdir");
        servo_fetch()
            .args([
                "--output-dir",
                dir.path().to_str().unwrap(),
                "--allow-private-addresses",
                TIMEOUT,
                &s.uri(),
            ])
            .assert()
            .success();
        let entries: Vec<_> = fs::read_dir(dir.path())
            .expect("dir exists")
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "md"))
            .collect();
        assert_eq!(entries.len(), 1, "expected exactly 1 .md file");
        let body = fs::read_to_string(entries[0].path()).unwrap();
        assert!(body.contains("Saved"), "got: {body}");
    });
}

#[test]
#[ignore = "e2e: requires Servo engine"]
fn crawl_output_dir_writes_per_page_files() {
    block_on(async {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/"))
            .respond_with(mock_page(
                "<html><head><title>CrawlDir</title></head><body><p>One page</p></body></html>",
            ))
            .mount(&s)
            .await;
        Mock::given(method("GET"))
            .and(path("/robots.txt"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&s)
            .await;
        let dir = tempfile::tempdir().expect("tempdir");
        let output = servo_fetch()
            .args([
                "crawl",
                &s.uri(),
                "--format",
                "json",
                "--output-dir",
                dir.path().to_str().unwrap(),
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
        assert!(output.is_empty(), "stdout should be silent in --output-dir mode");
        let entries: Vec<_> = fs::read_dir(dir.path())
            .expect("dir exists")
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
            .collect();
        assert_eq!(entries.len(), 1, "expected exactly 1 .json file");
        let line = fs::read_to_string(entries[0].path()).unwrap();
        let record: Value = serde_json::from_str(line.trim()).expect("valid JSON");
        assert_eq!(record["type"], "page");
    });
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
        .stdout(predicate::str::contains("--exclude"))
        .stdout(predicate::str::contains("--output-dir"));
}

#[test]
fn fetch_help_shows_output_dir() {
    servo_fetch()
        .args(["--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--output-dir"));
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
