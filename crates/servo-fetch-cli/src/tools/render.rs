//! Shape the response text: render a page to its string, paginate, and bound oversized output.

use std::borrow::Cow;

use servo_fetch::Page;
use servo_fetch_types::FetchFormat;

use super::error::{ToolError, ToolResult};
use super::limits::MAX_JS_OUTPUT_LEN;

/// Render a fetched page to its pre-pagination string for the requested format.
pub(crate) fn render_page<'a>(
    page: &'a Page,
    url: &str,
    format: FetchFormat,
    selector: Option<&str>,
) -> ToolResult<Cow<'a, str>> {
    Ok(match format {
        FetchFormat::Html => Cow::Borrowed(page.html.as_str()),
        FetchFormat::Text => Cow::Borrowed(page.inner_text.as_str()),
        FetchFormat::AccessibilityTree => Cow::Borrowed(page.accessibility_tree.as_deref().unwrap_or_default()),
        FetchFormat::Json => {
            let data = match selector {
                Some(s) => page.article_with_selector(url, s),
                None => page.article(url),
            }
            .map_err(ToolError::from)?;
            Cow::Owned(
                serde_json::to_string_pretty(&crate::wire::article(data))
                    .map_err(|e| ToolError::internal(e.to_string()))?,
            )
        }
        FetchFormat::Markdown => Cow::Owned(
            match selector {
                Some(s) => page.markdown_with_selector(url, s),
                None => page.markdown_with_url(url),
            }
            .map_err(ToolError::from)?,
        ),
    })
}

/// Paginate by character offset/count.
pub(crate) fn paginate(content: &str, start: usize, max_len: usize) -> String {
    let max_len = max_len.max(1);
    let total = content.chars().count();
    if start >= total {
        return format!("<no content at startIndex={start}, total_length={total}>");
    }
    let byte = |char_idx: usize| content.char_indices().nth(char_idx).map_or(content.len(), |(i, _)| i);
    let end = (start + max_len).min(total);
    let chunk = &content[byte(start)..byte(end)];
    if end < total {
        format!("{chunk}\n\n<content truncated. total_length={total}, next startIndex={end}>")
    } else {
        chunk.to_string()
    }
}

/// Truncate oversized JS evaluation output to the response limit, marking the cut.
pub(crate) fn clamp_js_output(mut result: String) -> String {
    if result.len() > MAX_JS_OUTPUT_LEN {
        result.truncate(servo_fetch::sanitize::floor_char_boundary(&result, MAX_JS_OUTPUT_LEN));
        result.push_str("\n<output truncated>");
    }
    result
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
        assert!(r.contains("next startIndex=5"));
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
    fn paginate_multibyte_truncates_by_char() {
        let r = paginate("日本語", 0, 2);
        assert!(r.starts_with("日本"));
        assert!(r.contains("total_length=3"));
        assert!(r.contains("next startIndex=2"));
    }

    #[test]
    fn paginate_max_len_zero_clamped() {
        let r = paginate("hello", 0, 0);
        assert!(r.starts_with('h'));
    }

    #[test]
    fn paginate_multibyte_offset_by_char() {
        assert_eq!(paginate("日本語", 1, 100), "本語");
    }
}
