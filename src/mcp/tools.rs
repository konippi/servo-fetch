//! Shared helpers for MCP tool implementations.

use base64::Engine as _;
use rmcp::ErrorData;
use rmcp::model::{CallToolResult, Content};

use crate::bridge;
use crate::net;
use servo_fetch::extract::{self, ExtractInput};

pub(super) fn validated_url(url: &str) -> Result<String, ErrorData> {
    net::validate_url(url)
        .map(|u| u.to_string())
        .map_err(|e| ErrorData::invalid_params(format!("{e:#}"), None))
}

pub(super) async fn fetch_page(
    url: &str,
    timeout: u64,
    screenshot: bool,
    js: Option<&str>,
) -> Result<bridge::ServoPage, ErrorData> {
    let url = url.to_string();
    let js = js.map(String::from);
    tokio::task::spawn_blocking(move || bridge::fetch_page(&url, timeout, screenshot, js.as_deref()))
        .await
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?
        .map_err(|e| ErrorData::internal_error(format!("{e:#}"), None))
}

pub(super) fn extract(page: &bridge::ServoPage, url: &str, json: bool, selector: Option<&str>) -> Result<String, ErrorData> {
    let mut input = ExtractInput::new(&page.html, url);
    input.layout_json = page.layout_json.as_deref();
    input.inner_text = page.inner_text.as_deref();
    input.selector = selector;
    if json {
        extract::extract_json(&input)
    } else {
        extract::extract_text(&input)
    }
    .map_err(|e| ErrorData::internal_error(e.to_string(), None))
}

pub(super) async fn take_screenshot(url: &str, timeout: u64) -> Result<CallToolResult, ErrorData> {
    let page = fetch_page(url, timeout, true, None).await?;
    let img = page
        .screenshot
        .ok_or_else(|| ErrorData::internal_error("screenshot capture failed", None))?;

    let mut buf = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

    Ok(CallToolResult::success(vec![Content::image(
        base64::engine::general_purpose::STANDARD.encode(&buf),
        "image/png",
    )]))
}

pub(super) fn paginate(content: &str, start: usize, max_len: usize) -> String {
    let total = content.len();
    let start = floor_char_boundary(content, start);
    if start >= total {
        return format!("<no content at start_index={start}, total_length={total}>");
    }
    let end = floor_char_boundary(content, (start + max_len).min(total));
    let chunk = &content[start..end];
    if end < total {
        format!("{chunk}\n\n<content truncated. total_length={total}, next start_index={end}>")
    } else {
        chunk.to_string()
    }
}

pub(super) fn floor_char_boundary(s: &str, index: usize) -> usize {
    if index >= s.len() {
        return s.len();
    }
    let mut i = index;
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paginate_full() {
        assert_eq!(paginate("hello", 0, 100), "hello");
    }

    #[test]
    fn paginate_truncates() {
        let r = paginate("hello world", 0, 5);
        assert!(r.starts_with("hello"));
        assert!(r.contains("next start_index=5"));
    }

    #[test]
    fn paginate_offset() {
        assert_eq!(paginate("hello world", 6, 100), "world");
    }

    #[test]
    fn paginate_out_of_bounds() {
        assert!(paginate("hello", 100, 10).contains("no content"));
    }

    #[test]
    fn paginate_multibyte_boundary() {
        // 2-byte chars: "ĵƥ" = [0xC4 0xB5][0xC6 0xA5] = 4 bytes
        assert_eq!(floor_char_boundary("ĵƥ", 0), 0); // start of ĵ
        assert_eq!(floor_char_boundary("ĵƥ", 1), 0); // inside ĵ
        assert_eq!(floor_char_boundary("ĵƥ", 2), 2); // start of ƥ
        assert_eq!(floor_char_boundary("ĵƥ", 3), 2); // inside ƥ
        assert_eq!(floor_char_boundary("ĵƥ", 4), 4); // end

        // 3-byte chars: "日本語" = [E6 97 A5][E6 9C AC][E8 AA 9E] = 9 bytes
        assert_eq!(floor_char_boundary("日本語", 0), 0); // start of 日
        assert_eq!(floor_char_boundary("日本語", 1), 0); // inside 日
        assert_eq!(floor_char_boundary("日本語", 2), 0); // inside 日
        assert_eq!(floor_char_boundary("日本語", 3), 3); // start of 本
        assert_eq!(floor_char_boundary("日本語", 4), 3); // inside 本
        assert_eq!(floor_char_boundary("日本語", 5), 3); // inside 本
        assert_eq!(floor_char_boundary("日本語", 6), 6); // start of 語
        assert_eq!(floor_char_boundary("日本語", 7), 6); // inside 語
        assert_eq!(floor_char_boundary("日本語", 8), 6); // inside 語
        assert_eq!(floor_char_boundary("日本語", 9), 9); // end

        // 4-byte chars: "🦀" = [F0 9F A6 80] = 4 bytes
        assert_eq!(floor_char_boundary("🦀", 0), 0); // start of 🦀
        assert_eq!(floor_char_boundary("🦀", 1), 0); // inside 🦀
        assert_eq!(floor_char_boundary("🦀", 2), 0); // inside 🦀
        assert_eq!(floor_char_boundary("🦀", 3), 0); // inside 🦀
        assert_eq!(floor_char_boundary("🦀", 4), 4); // end

        // paginate must produce valid UTF-8 at the boundary
        let result = paginate("日本語", 0, 4);
        assert!(result.starts_with("日"));
    }

    #[test]
    fn rejects_private_url() {
        assert!(validated_url("http://127.0.0.1/").is_err());
    }

    #[test]
    fn accepts_public_url() {
        assert!(validated_url("https://example.com").is_ok());
    }
}
