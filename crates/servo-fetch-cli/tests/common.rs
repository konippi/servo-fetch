//! Shared helpers for integration tests.

#![allow(dead_code, unreachable_pub)]

use std::time::Duration;

use tokio::sync::mpsc;
use wiremock::{Request, ResponseTemplate};

pub fn mock_page(html: impl Into<String>) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_raw(html.into().into_bytes(), "text/html; charset=utf-8")
}

/// A slow page whose responder reports each arriving request, so tests can act mid-fetch.
pub fn slow_page(
    html: &'static str,
    delay: Duration,
) -> (mpsc::UnboundedReceiver<()>, impl Fn(&Request) -> ResponseTemplate) {
    let (arrived, arrivals) = mpsc::unbounded_channel();
    let responder = move |_: &Request| {
        let _ = arrived.send(());
        mock_page(html).set_delay(delay)
    };
    (arrivals, responder)
}
