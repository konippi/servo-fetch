//! Fetch and batch-fetch tool helpers.

use crate::bridge;
use servo_fetch::extract::{self, ExtractInput};

use super::common::{fetch_semaphore, paginate};

pub(crate) async fn fetch_page(
    url: &str,
    timeout: u64,
    settle_ms: u64,
    mode: bridge::FetchMode,
) -> Result<bridge::ServoPage, String> {
    let _permit = fetch_semaphore()
        .acquire()
        .await
        .map_err(|e| format!("fetch semaphore closed: {e}"))?;

    let url = url.to_string();
    tokio::task::spawn_blocking(move || {
        bridge::fetch_page(bridge::FetchOptions {
            url: &url,
            timeout_secs: timeout,
            settle_ms,
            mode,
        })
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| format!("{e:#}"))
}

pub(crate) async fn probe_pdf(url: &str, timeout: u64) -> Option<Vec<u8>> {
    let url = url.to_string();
    tokio::task::spawn_blocking(move || crate::pdf::probe(&url, timeout))
        .await
        .ok()
        .flatten()
}

pub(crate) async fn batch_fetch_pages(
    urls: &[String],
    timeout: u64,
    settle_ms: u64,
    json: bool,
    selector: Option<&str>,
    max_len: usize,
) -> Vec<(String, String)> {
    let (tx, mut rx) = tokio::sync::mpsc::channel(urls.len().max(1));

    for url in urls {
        let permit = fetch_semaphore().acquire().await.ok();
        let tx = tx.clone();
        let url = url.clone();
        let selector = selector.map(String::from);
        tokio::task::spawn_blocking(move || {
            let text = fetch_and_render(&url, timeout, settle_ms, json, selector.as_deref(), max_len);
            let _ = tx.blocking_send((url, text));
            drop(permit);
        });
    }
    drop(tx);

    let mut results = Vec::with_capacity(urls.len());
    while let Some(pair) = rx.recv().await {
        results.push(pair);
    }
    results
}

fn fetch_and_render(
    url: &str,
    timeout: u64,
    settle_ms: u64,
    json: bool,
    selector: Option<&str>,
    max_len: usize,
) -> String {
    let page = match bridge::fetch_page(bridge::FetchOptions {
        url,
        timeout_secs: timeout,
        settle_ms,
        mode: bridge::FetchMode::Content { include_a11y: false },
    }) {
        Ok(p) => p,
        Err(e) => return format!("[error] {e:#}"),
    };

    let input = ExtractInput::new(&page.html, url)
        .with_layout_json(page.layout_json.as_deref())
        .with_inner_text(page.inner_text.as_deref())
        .with_selector(selector);

    let full = if json {
        extract::extract_json(&input).unwrap_or_default()
    } else {
        extract::extract_text(&input).unwrap_or_default()
    };

    paginate(&servo_fetch::sanitize::sanitize(&full), 0, max_len)
}
