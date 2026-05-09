//! Shared helpers for servo-fetch tool operations.

use std::sync::OnceLock;

use tokio::sync::Semaphore;

use servo_fetch::Page;
use servo_fetch::extract::{self, ExtractInput};

use super::error::{ToolError, ToolResult};

const DEFAULT_MAX_CONCURRENT_FETCHES: usize = 4;
const MAX_ALLOWED_CONCURRENCY: usize = 16;

pub(crate) fn fetch_semaphore() -> &'static Semaphore {
    static SEMAPHORE: OnceLock<Semaphore> = OnceLock::new();
    SEMAPHORE.get_or_init(|| {
        let limit = std::env::var("SERVO_FETCH_MAX_CONCURRENCY")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|n| *n > 0)
            .map_or(DEFAULT_MAX_CONCURRENT_FETCHES, |n| n.min(MAX_ALLOWED_CONCURRENCY));
        Semaphore::new(limit)
    })
}

pub(crate) fn validated_url(url: &str) -> ToolResult<String> {
    servo_fetch::validate_url(url)
        .map(|u| u.to_string())
        .map_err(|e| ToolError::invalid(format!("{e:#}")))
}

pub(crate) fn extract(page: &Page, url: &str, json: bool, selector: Option<&str>) -> ToolResult<String> {
    let input = ExtractInput::new(&page.html, url)
        .with_layout_json(page.layout_json.as_deref())
        .with_inner_text(Some(&page.inner_text))
        .with_selector(selector);
    if json {
        extract::extract_json(&input)
    } else {
        extract::extract_text(&input)
    }
    .map_err(|e| ToolError::internal(e.to_string()))
}

pub(crate) fn paginate(content: &str, start: usize, max_len: usize) -> String {
    use servo_fetch::sanitize::floor_char_boundary;

    let max_len = max_len.max(1);
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
        let result = paginate("日本語", 0, 4);
        assert!(result.starts_with("日"));
    }

    #[test]
    fn paginate_max_len_zero_clamped() {
        let r = paginate("hello", 0, 0);
        assert!(r.starts_with('h'));
    }

    #[test]
    fn paginate_start_mid_multibyte() {
        let r = paginate("日本語", 1, 100);
        assert!(r.starts_with("日"));
    }
}
