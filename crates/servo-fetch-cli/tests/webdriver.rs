//! End-to-end test for the `servo-fetch webdriver` server.
//!
//! Spawns the real binary, then drives it over HTTP with W3C `WebDriver` JSON
//! (New Session → navigate → find → click → send keys → execute script →
//! screenshot → Delete Session) against a local wiremock page.
#![cfg(feature = "webdriver")]

use std::net::TcpListener;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// W3C element identifier key.
const ELEMENT_KEY: &str = "element-6066-11e4-a52e-4f735466cecf";

const PAGE: &str = r#"<!DOCTYPE html>
<html>
<head><title>WD Spike</title></head>
<body>
  <button id="btn" onclick="document.getElementById('out').textContent = 'clicked';">Click me</button>
  <div id="out">initial</div>
  <input id="inp" type="text">
</body>
</html>"#;

/// Kills the spawned server process on drop.
struct ServerGuard(Child);

impl Drop for ServerGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

fn agent() -> ureq::Agent {
    ureq::Agent::new_with_config(
        ureq::config::Config::builder()
            .timeout_global(Some(Duration::from_secs(90)))
            .build(),
    )
}

fn post(agent: &ureq::Agent, url: &str, body: &Value) -> Value {
    let bytes = serde_json::to_vec(body).expect("serialize request");
    let response = agent
        .post(url)
        .header("Content-Type", "application/json")
        .send(bytes.as_slice())
        .unwrap_or_else(|e| panic!("POST {url}: {e}"));
    let text = response.into_body().read_to_string().expect("read body");
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {url} response: {e}"))
}

fn get(agent: &ureq::Agent, url: &str) -> Value {
    let response = agent.get(url).call().unwrap_or_else(|e| panic!("GET {url}: {e}"));
    let text = response.into_body().read_to_string().expect("read body");
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {url} response: {e}"))
}

fn wait_ready(agent: &ureq::Agent, base: &str) {
    let url = format!("{base}/status");
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if agent.get(&url).call().is_ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    panic!("WebDriver server did not become ready");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "e2e: spawns the servo-fetch WebDriver server (Servo engine)"]
async fn webdriver_server_round_trip() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(PAGE.as_bytes(), "text/html; charset=utf-8"))
        .mount(&server)
        .await;
    let page_url = format!("{}/", server.uri());

    let port = free_port();
    let child = Command::new(env!("CARGO_BIN_EXE_servo-fetch"))
        .args(["webdriver", "--port", &port.to_string(), "--allow-private-addresses"])
        .spawn()
        .expect("spawn servo-fetch webdriver");
    let _guard = ServerGuard(child);
    let base = format!("http://127.0.0.1:{port}");

    // The W3C client calls are blocking; run them off the async runtime.
    tokio::task::spawn_blocking(move || {
        let agent = agent();
        wait_ready(&agent, &base);

        // New Session.
        let session = post(
            &agent,
            &format!("{base}/session"),
            &json!({"capabilities": {"alwaysMatch": {}}}),
        );
        let sid = session["value"]["sessionId"].as_str().expect("sessionId").to_string();

        // Navigate.
        post(
            &agent,
            &format!("{base}/session/{sid}/url"),
            &json!({ "url": page_url }),
        );

        // Title.
        let title = get(&agent, &format!("{base}/session/{sid}/title"));
        assert_eq!(title["value"], "WD Spike", "GetTitle: {title}");

        // Current URL.
        let url = get(&agent, &format!("{base}/session/{sid}/url"));
        assert_eq!(url["value"], page_url, "GetCurrentUrl: {url}");

        // Find element.
        let found = post(
            &agent,
            &format!("{base}/session/{sid}/element"),
            &json!({"using": "css selector", "value": "#btn"}),
        );
        let element_id = found["value"][ELEMENT_KEY].as_str().expect("element id").to_string();

        // Click, then poll the DOM (input events are async) for the handler effect.
        post(
            &agent,
            &format!("{base}/session/{sid}/element/{element_id}/click"),
            &json!({}),
        );
        let mut out = Value::Null;
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            out = post(
                &agent,
                &format!("{base}/session/{sid}/execute/sync"),
                &json!({"script": "return document.getElementById('out').textContent", "args": []}),
            );
            if out["value"] == "clicked" {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        assert_eq!(out["value"], "clicked", "click effect not observed: {out}");

        // Send keys to the text input, then read it back.
        let input = post(
            &agent,
            &format!("{base}/session/{sid}/element"),
            &json!({"using": "css selector", "value": "#inp"}),
        );
        let input_id = input["value"][ELEMENT_KEY].as_str().expect("input id").to_string();
        post(
            &agent,
            &format!("{base}/session/{sid}/element/{input_id}/value"),
            &json!({"text": "hello"}),
        );
        let typed = post(
            &agent,
            &format!("{base}/session/{sid}/execute/sync"),
            &json!({"script": "return document.getElementById('inp').value", "args": []}),
        );
        assert_eq!(typed["value"], "hello", "send keys not reflected: {typed}");

        // Screenshot (base64 PNG).
        let shot = get(&agent, &format!("{base}/session/{sid}/screenshot"));
        assert!(
            shot["value"].as_str().is_some_and(|s| !s.is_empty()),
            "screenshot should be a non-empty base64 string"
        );

        // Delete Session.
        agent
            .delete(&format!("{base}/session/{sid}"))
            .call()
            .expect("delete session");
    })
    .await
    .expect("spawn_blocking");
}
