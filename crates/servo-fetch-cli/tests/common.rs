//! Shared helpers for integration tests.

#![allow(dead_code, unreachable_pub)]

use wiremock::ResponseTemplate;

pub fn mock_page(html: impl Into<String>) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_raw(html.into().into_bytes(), "text/html; charset=utf-8")
}
