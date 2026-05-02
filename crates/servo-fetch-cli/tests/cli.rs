//! CLI integration tests.

use assert_cmd::Command;
use predicates::prelude::*;

fn servo_fetch() -> Command {
    Command::cargo_bin("servo-fetch").expect("binary exists")
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

const TIMEOUT: &str = "--timeout=60";

#[test]
#[ignore = "requires Servo + network"]
fn default_produces_markdown() {
    servo_fetch()
        .args([TIMEOUT, "https://example.com"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Example Domain"));
}

#[test]
#[ignore = "requires Servo + network"]
fn json_produces_valid_json() {
    let output = servo_fetch()
        .args(["--json", TIMEOUT, "https://example.com"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let parsed: serde_json::Value = serde_json::from_slice(&output).expect("valid JSON");
    assert!(parsed.get("title").is_some());
}

#[test]
#[ignore = "requires Servo + network"]
fn js_eval_returns_result() {
    servo_fetch()
        .args(["--js", "document.title", TIMEOUT, "https://example.com"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Example Domain"));
}

#[test]
#[ignore = "requires Servo + network"]
fn screenshot_creates_file() {
    let dir = std::env::temp_dir().join("servo-fetch-test");
    std::fs::create_dir_all(&dir).ok();
    let path = dir.join("test.png");
    servo_fetch()
        .args(["--screenshot", path.to_str().unwrap(), TIMEOUT, "https://example.com"])
        .assert()
        .success();
    assert!(path.exists(), "screenshot file should be created");
    assert!(path.metadata().unwrap().len() > 0, "screenshot should not be empty");
    std::fs::remove_file(&path).ok();
}

#[test]
#[ignore = "requires Servo + network"]
fn timeout_produces_error() {
    servo_fetch()
        .args(["--timeout=0", "https://example.com"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid value '0'"));
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
#[ignore = "requires Servo + network"]
fn crawl_produces_ndjson() {
    let output = servo_fetch()
        .args(["crawl", "https://example.com", "--limit", "2", "--timeout", "60"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let first_line = std::str::from_utf8(&output)
        .expect("valid utf8")
        .lines()
        .next()
        .expect("at least one line");
    let parsed: serde_json::Value = serde_json::from_str(first_line).expect("valid NDJSON");
    assert_eq!(parsed["status"], "ok");
    assert!(parsed["url"].as_str().is_some());
    assert!(parsed["content"].as_str().is_some());
}
