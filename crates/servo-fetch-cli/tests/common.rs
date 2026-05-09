//! Shared helpers for integration tests.

#![allow(dead_code, unreachable_pub)]

use wiremock::ResponseTemplate;

pub fn mock_page(html: impl Into<String>) -> ResponseTemplate {
    ResponseTemplate::new(200)
        .insert_header("content-type", "text/html")
        .set_body_string(html.into())
}
