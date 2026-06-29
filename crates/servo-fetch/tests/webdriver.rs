//! `WebDriver` Phase 0 spike: end-to-end round-trip against the real Servo engine.
//!
//! This is the go/no-go gate for the `WebDriver` path. It proves, at runtime, the
//! four unknowns the spike set out to validate:
//!   1. `execute_webdriver_command` works from embedding without registering any
//!      `WebDriver` channels with the constellation up front,
//!   2. the top-level `BrowsingContextId` can be derived from a fresh `WebViewId`,
//!   3. the constellation reply channels (`servo_base::generic_channel`) can be
//!      constructed and round-tripped from embedding, and
//!   4. a full navigate -> get-title -> find-element -> click -> read-result
//!      sequence works on a single persistent webview.
#![cfg(feature = "webdriver")]

use std::time::{Duration, Instant};

use servo_fetch::NetworkPolicy;
use servo_fetch::webdriver::{Locator, WebDriverEngine};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const PAGE: &str = r#"<!DOCTYPE html>
<html>
<head><title>WD Spike</title></head>
<body>
  <button id="btn"
    onclick="document.getElementById('out').textContent = 'clicked'; document.title = 'clicked!';">
    Click me
  </button>
  <div id="out">initial</div>
</body>
</html>"#;

#[tokio::test(flavor = "multi_thread")]
#[ignore = "e2e: spawns the Servo engine; gate test for the WebDriver spike"]
async fn webdriver_round_trip() {
    servo_fetch::init(NetworkPolicy::PERMISSIVE);

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(PAGE.as_bytes(), "text/html; charset=utf-8"))
        .mount(&server)
        .await;
    let url = format!("{}/", server.uri());

    tokio::task::spawn_blocking(move || {
        let engine = WebDriverEngine::new();

        let session = engine.new_session().expect("new_session");

        engine.navigate(&session, &url).expect("navigate");

        // Unknown #4: ScriptCommand(GetTitle) round-trip via a constellation channel.
        let title = engine.title(&session).expect("title");
        assert_eq!(title, "WD Spike", "GetTitle round-trip should return the page title");

        // ScriptCommand(FindElementsCSSSelector) round-trip + element reference flow.
        let elements = engine
            .find_elements(&session, Locator::Css, "#btn")
            .expect("find_elements");
        assert_eq!(elements.len(), 1, "expected exactly one #btn, got {elements:?}");

        // ScriptCommand(ElementClick) + native input-event click.
        engine.click_element(&session, &elements[0]).expect("click_element");

        // The click handler updates #out and the document title. Input events are
        // dispatched asynchronously, so poll the DOM until it reflects the click.
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut out = String::new();
        while Instant::now() < deadline {
            out = engine
                .execute_script(&session, "return document.getElementById('out').textContent", &[])
                .expect("execute_script")
                .as_str()
                .unwrap_or_default()
                .to_string();
            if out.contains("clicked") {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(out.contains("clicked"), "click did not update the DOM (got {out:?})");

        let title = engine.title(&session).expect("title after click");
        assert_eq!(title, "clicked!", "click handler should have updated the title");

        engine.delete_session(&session).expect("delete_session");
    })
    .await
    .expect("spawn_blocking");
}
