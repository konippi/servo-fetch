//! MCP-only response-size policies and bounded content construction.

use std::fmt;

use rmcp::model::{CallToolResult, ContentBlock};

use super::tools::ToolError;

/// Maximum serialized size of an MCP text tool result.
pub(super) const MAX_MCP_SERIALIZED_TEXT_RESULT_BYTES: usize = 1_000_000;
/// Maximum base64 payload size of an MCP screenshot result.
pub(super) const MAX_MCP_SCREENSHOT_BASE64_BYTES: usize = 8_000_000;

// Reserve private space for CallToolResult/ContentBlock JSON overhead and a truncation marker.
const MAX_MCP_SERIALIZED_RESULT_HEADROOM_BYTES: usize = 1_024;
// Worst-case UTF-8 character budget used only to bound intermediate text construction.
const MAX_MCP_PREBUILD_TEXT_CHARS: usize =
    (MAX_MCP_SERIALIZED_TEXT_RESULT_BYTES - MAX_MCP_SERIALIZED_RESULT_HEADROOM_BYTES) / 4;

pub(super) struct TextOutput {
    blocks: Vec<(ContentBlock, usize)>,
    serialized_blocks: usize,
    omitted: usize,
    is_error: bool,
    byte_limit: usize,
}

impl TextOutput {
    pub(super) fn success() -> Self {
        Self::new(false, MAX_MCP_SERIALIZED_TEXT_RESULT_BYTES)
    }

    pub(super) fn error() -> Self {
        Self::new(true, MAX_MCP_SERIALIZED_TEXT_RESULT_BYTES)
    }

    fn new(is_error: bool, byte_limit: usize) -> Self {
        Self {
            blocks: Vec::new(),
            serialized_blocks: 0,
            omitted: 0,
            is_error,
            byte_limit,
        }
    }

    #[cfg(test)]
    fn with_limit(is_error: bool, limit: usize) -> Self {
        Self::new(is_error, limit)
    }

    pub(super) fn push(&mut self, text: impl fmt::Display) -> bool {
        let block = ContentBlock::text(bounded_text(text));
        let serialized_len = serde_json::to_vec(&block).map_or(usize::MAX, |bytes| bytes.len());
        if self.fits(serialized_len) {
            self.serialized_blocks = self.serialized_blocks.saturating_add(serialized_len);
            self.blocks.push((block, serialized_len));
            true
        } else {
            self.omitted = self.omitted.saturating_add(1);
            false
        }
    }

    pub(super) fn finish(mut self) -> CallToolResult {
        if self.omitted > 0 {
            loop {
                let summary = ContentBlock::text(format!(
                    "<response truncated: omitted {} content {}>",
                    self.omitted,
                    if self.omitted == 1 { "block" } else { "blocks" }
                ));
                let summary_len = serde_json::to_vec(&summary).map_or(usize::MAX, |bytes| bytes.len());
                if self.fits(summary_len) {
                    self.serialized_blocks = self.serialized_blocks.saturating_add(summary_len);
                    self.blocks.push((summary, summary_len));
                    break;
                }
                if let Some((_block, len)) = self.blocks.pop() {
                    self.serialized_blocks = self.serialized_blocks.saturating_sub(len);
                    self.omitted = self.omitted.saturating_add(1);
                } else {
                    break;
                }
            }
        }

        let content = self.blocks.into_iter().map(|(block, _)| block).collect();
        if self.is_error {
            CallToolResult::error(content)
        } else {
            CallToolResult::success(content)
        }
    }

    fn fits(&self, candidate_len: usize) -> bool {
        let empty = if self.is_error {
            CallToolResult::error(Vec::new())
        } else {
            CallToolResult::success(Vec::new())
        };
        let base_len = serde_json::to_vec(&empty).map_or(usize::MAX, |bytes| bytes.len());
        let block_count = self.blocks.len().saturating_add(1);
        base_len
            .checked_add(self.serialized_blocks)
            .and_then(|size| size.checked_add(candidate_len))
            .and_then(|size| size.checked_add(block_count.saturating_sub(1)))
            .is_some_and(|size| size <= self.byte_limit)
    }
}

pub(super) fn effective_item_length(requested: usize, item_count: usize) -> usize {
    requested.min(MAX_MCP_PREBUILD_TEXT_CHARS / item_count.max(1)).max(1)
}

pub(super) fn bounded_text(value: impl fmt::Display) -> String {
    bounded_format(format_args!("{value}"), MAX_MCP_PREBUILD_TEXT_CHARS)
}

pub(super) fn labeled_content(url: &str, content: &str) -> String {
    bounded_format(format_args!("URL: {url}\n\n{content}"), MAX_MCP_PREBUILD_TEXT_CHARS)
}

struct BoundedWriter {
    output: String,
    remaining_chars: usize,
    truncated: bool,
}

impl fmt::Write for BoundedWriter {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        let (prefix, complete) = bounded_char_prefix(value, self.remaining_chars);
        self.output.push_str(prefix);
        self.remaining_chars = self.remaining_chars.saturating_sub(prefix.chars().count());
        if complete {
            Ok(())
        } else {
            self.truncated = true;
            Err(fmt::Error)
        }
    }
}

fn bounded_format(args: fmt::Arguments<'_>, char_limit: usize) -> String {
    let mut writer = BoundedWriter {
        output: String::new(),
        remaining_chars: char_limit,
        truncated: false,
    };
    let _ = fmt::write(&mut writer, args);
    let mut output = servo_fetch::sanitize::sanitize(&writer.output).into_owned();
    if writer.truncated {
        append_with_trim(&mut output, "\n<output truncated>", char_limit);
    }
    output
}

pub(super) fn map_text<I>(urls: I) -> String
where
    I: IntoIterator<Item = String>,
{
    map_text_with_limit(urls, MAX_MCP_PREBUILD_TEXT_CHARS)
}

fn bounded_char_prefix(value: &str, max_chars: usize) -> (&str, bool) {
    value
        .char_indices()
        .nth(max_chars)
        .map_or((value, true), |(byte, _)| (&value[..byte], false))
}

fn map_text_with_limit<I>(urls: I, char_limit: usize) -> String
where
    I: IntoIterator<Item = String>,
{
    let mut output = String::new();
    let mut line_boundaries = Vec::new();
    let mut returned = 0usize;
    let mut omitted = 0usize;
    let mut chars = 0usize;

    for url in urls {
        let available = char_limit.saturating_sub(chars).saturating_sub(1);
        let (url, full_url) = bounded_char_prefix(&url, available);
        if !full_url {
            omitted = omitted.saturating_add(1);
            continue;
        }
        let url = servo_fetch::sanitize::sanitize(url);
        let line_chars = url.chars().count().saturating_add(1);
        if chars.saturating_add(line_chars) <= char_limit {
            line_boundaries.push(output.len());
            output.push_str(&url);
            output.push('\n');
            chars += line_chars;
            returned += 1;
        } else {
            omitted = omitted.saturating_add(1);
        }
    }

    if omitted > 0 {
        loop {
            let summary = format!("<map truncated: returned {returned} URLs, omitted {omitted} URLs>");
            let summary_chars = summary.chars().count();
            if chars.saturating_add(summary_chars) <= char_limit {
                output.push_str(&summary);
                break;
            }
            if let Some(boundary) = line_boundaries.pop() {
                output.truncate(boundary);
                chars = output.chars().count();
                returned = returned.saturating_sub(1);
                omitted = omitted.saturating_add(1);
            } else {
                append_with_trim(&mut output, &summary, char_limit);
                break;
            }
        }
    }
    output
}

pub(super) fn javascript_text<'a, I>(result: &str, console: I) -> String
where
    I: ExactSizeIterator<Item = (String, &'a str)>,
{
    javascript_text_with_limit(result, console, MAX_MCP_PREBUILD_TEXT_CHARS)
}

fn javascript_text_with_limit<'a, I>(result: &str, mut console: I, char_limit: usize) -> String
where
    I: ExactSizeIterator<Item = (String, &'a str)>,
{
    let (result, full_result) = bounded_char_prefix(result, char_limit);
    let result = servo_fetch::sanitize::sanitize(result);
    let mut output = String::new();
    let _ = push_chars(&mut output, &result, char_limit);
    if !full_result {
        append_with_trim(&mut output, "\n<output truncated>", char_limit);
    }

    if console.len() == 0 {
        return output;
    }

    let console_start = output.len();
    let header = "\n\n--- console output ---\n";
    let mut line_boundaries = Vec::new();
    let mut omitted = 0usize;
    let mut chars = output.chars().count();

    while let Some((level, message)) = console.next() {
        let header_chars = if line_boundaries.is_empty() {
            header.chars().count()
        } else {
            0
        };
        let level_chars = level.chars().count();
        let available = char_limit.saturating_sub(
            chars
                .saturating_add(header_chars)
                .saturating_add(level_chars)
                .saturating_add(4),
        );
        let (message, full_message) = bounded_char_prefix(message, available);
        if !full_message {
            omitted = 1usize.saturating_add(console.len());
            break;
        }
        let message = servo_fetch::sanitize::sanitize(message);
        let line_chars = level_chars.saturating_add(message.chars().count()).saturating_add(4);
        if chars.saturating_add(header_chars).saturating_add(line_chars) > char_limit {
            omitted = 1usize.saturating_add(console.len());
            break;
        }
        if line_boundaries.is_empty() {
            output.push_str(header);
            chars += header_chars;
        }
        line_boundaries.push(output.len());
        let _ = fmt::Write::write_fmt(&mut output, format_args!("[{level}] {message}\n"));
        chars += line_chars;
    }

    if omitted > 0 {
        loop {
            let summary = format!("\n<console output truncated: omitted {omitted} console messages>");
            let summary_chars = summary.chars().count();
            if chars.saturating_add(summary_chars) <= char_limit {
                output.push_str(&summary);
                break;
            }
            if let Some(boundary) = line_boundaries.pop() {
                output.truncate(boundary);
                chars = output.chars().count();
                omitted = omitted.saturating_add(1);
                if line_boundaries.is_empty() {
                    output.truncate(console_start);
                    chars = output.chars().count();
                }
            } else {
                let combined_summary = format!("\n<output truncated>{summary}");
                append_with_trim(&mut output, &combined_summary, char_limit);
                break;
            }
        }
    }
    output
}

pub(super) fn checked_base64_length(raw_len: usize, encoded_limit: usize) -> Result<usize, ToolError> {
    let encoded_len = raw_len
        .checked_add(2)
        .map(|len| len / 3)
        .and_then(|groups| groups.checked_mul(4))
        .ok_or_else(|| ToolError::internal("screenshot base64 size overflow"))?;
    if encoded_len > encoded_limit {
        return Err(ToolError::internal(format!(
            "screenshot base64 data exceeds {encoded_limit} byte MCP response limit"
        )));
    }
    Ok(encoded_len)
}

fn push_chars(output: &mut String, value: &str, char_limit: usize) -> bool {
    let available = char_limit.saturating_sub(output.chars().count());
    let mut chars = value.chars();
    output.extend(chars.by_ref().take(available));
    chars.next().is_none()
}

fn append_with_trim(output: &mut String, suffix: &str, char_limit: usize) {
    let (suffix, _) = bounded_char_prefix(suffix, char_limit);
    let suffix_chars = suffix.chars().count();
    truncate_to_chars(output, char_limit.saturating_sub(suffix_chars));
    output.push_str(suffix);
}

fn truncate_to_chars(output: &mut String, max_chars: usize) {
    if let Some((byte, _)) = output.char_indices().nth(max_chars) {
        output.truncate(byte);
    }
}

#[cfg(test)]
mod tests {
    use rmcp::model::{CallToolResult, ContentBlock};

    use super::*;

    fn serialized_len(result: &CallToolResult) -> usize {
        serde_json::to_vec(result).expect("serialize CallToolResult").len()
    }

    fn text_blocks(result: &CallToolResult) -> Vec<&str> {
        result
            .content
            .iter()
            .map(|block| block.as_text().expect("text block").text.as_str())
            .collect()
    }

    #[test]
    fn text_output_accepts_exact_serialized_boundary_and_omits_over_boundary() {
        let text = "x".repeat(200);
        let exact = serialized_len(&CallToolResult::success(vec![ContentBlock::text(&text)]));

        let mut accepted = TextOutput::with_limit(false, exact);
        assert!(accepted.push(&text));
        assert_eq!(text_blocks(&accepted.finish()), vec![text.as_str()]);

        let mut omitted = TextOutput::with_limit(false, exact - 1);
        assert!(!omitted.push(&text));
        let result = omitted.finish();
        assert!(serialized_len(&result) < exact);
        assert!(text_blocks(&result)[0].contains("omitted 1 content block"));
    }

    #[test]
    fn text_output_measures_utf8_and_json_escaping_and_stays_under_final_cap() {
        let escaped = "é\"\\".repeat(16_666);
        let mut output = TextOutput::success();
        assert!(output.push(escaped));
        let result = output.finish();
        assert!(serialized_len(&result) <= MAX_MCP_SERIALIZED_TEXT_RESULT_BYTES);

        let mut four_byte = TextOutput::success();
        assert!(four_byte.push("😀".repeat(MAX_MCP_PREBUILD_TEXT_CHARS + 1)));
        let result = four_byte.finish();
        let text = text_blocks(&result)[0];
        assert!(text.starts_with('😀'));
        assert!(text.ends_with("<output truncated>"));
        assert!(!text.contains("omitted 1 content block"));
        assert!(serialized_len(&result) <= MAX_MCP_SERIALIZED_TEXT_RESULT_BYTES);
    }

    #[test]
    fn bounded_text_and_labeled_content_bound_before_sanitizing_controls() {
        let controls = "visible\x1b[31m red\x1b[0m\x00\u{009b}32m\u{202e} text\x1b]0;title\x07";
        assert_eq!(bounded_text(controls), "visible red text");

        let label = labeled_content(
            "https://example.com/\x1b[31mred\x1b[0m\u{202e}",
            "body\x00\u{0085}\x1b]0;title\x07 text",
        );
        assert_eq!(label, "URL: https://example.com/red\n\nbody text");

        let oversized = format!("{}unreachable", "\x01".repeat(MAX_MCP_PREBUILD_TEXT_CHARS + 100));
        let bounded = bounded_text(&oversized);
        assert!(bounded.chars().count() <= MAX_MCP_PREBUILD_TEXT_CHARS);
        assert!(!bounded.contains("unreachable"));
        assert!(!bounded.contains('\x01'));
        assert!(bounded.ends_with("<output truncated>"));
    }

    #[test]
    fn text_output_push_defensively_bounds_and_sanitizes_raw_callers() {
        struct GuardedDisplay<'a>(&'a str);

        impl fmt::Display for GuardedDisplay<'_> {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Write::write_str(formatter, self.0)?;
                panic!("formatter continued after the output bound");
            }
        }

        let raw = format!(
            "safe\x1b[31m red\x1b[0m\u{202e}{}unreachable",
            "x".repeat(MAX_MCP_PREBUILD_TEXT_CHARS)
        );
        let mut guarded = TextOutput::success();
        assert!(guarded.push(GuardedDisplay(&raw)));
        assert!(serialized_len(&guarded.finish()) <= MAX_MCP_SERIALIZED_TEXT_RESULT_BYTES);

        let mut output = TextOutput::success();
        assert!(output.push(raw));

        let result = output.finish();
        let text = text_blocks(&result)[0];
        assert!(text.starts_with("safe red"));
        assert!(!text.contains('\x1b'));
        assert!(!text.contains('\u{202e}'));
        assert!(!text.contains("unreachable"));
        assert!(text.ends_with("<output truncated>"));
        assert!(serialized_len(&result) <= MAX_MCP_SERIALIZED_TEXT_RESULT_BYTES);
    }

    #[test]
    fn tiny_helper_limits_are_never_exceeded() {
        let marker = "\n<output truncated>";
        for limit in 0..marker.chars().count() {
            let mut appended = "existing".to_string();
            append_with_trim(&mut appended, marker, limit);
            assert!(appended.chars().count() <= limit, "append limit {limit}");

            let map = map_text_with_limit(["https://example.com/long".to_string()], limit);
            assert!(map.chars().count() <= limit, "map limit {limit}");

            let javascript = javascript_text_with_limit("result", [("Log".to_string(), "message")].into_iter(), limit);
            assert!(javascript.chars().count() <= limit, "JavaScript limit {limit}");
        }
    }

    #[test]
    fn truncation_summary_replaces_previously_accepted_block_when_needed() {
        let first = "x".repeat(200);
        let one_block = CallToolResult::success(vec![ContentBlock::text(&first)]);
        let limit = serialized_len(&one_block);
        let mut output = TextOutput::with_limit(false, limit);
        assert!(output.push(first));
        assert!(!output.push("y".repeat(200)));

        let result = output.finish();
        let blocks = text_blocks(&result);
        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].contains("omitted 2 content blocks"));
        assert!(serialized_len(&result) <= limit);
    }

    #[test]
    fn error_output_is_a_bounded_single_text_result() {
        let mut output = TextOutput::error();
        output.push("\"\\".repeat(MAX_MCP_SERIALIZED_TEXT_RESULT_BYTES / 2));
        let result = output.finish();
        assert_eq!(result.is_error, Some(true));
        assert_eq!(result.content.len(), 1);
        assert!(result.content[0].as_text().is_some());
        assert!(serialized_len(&result) <= MAX_MCP_SERIALIZED_TEXT_RESULT_BYTES);
    }

    #[test]
    fn per_item_guard_is_derived_from_final_cap_and_item_count() {
        let derived_chars = MAX_MCP_PREBUILD_TEXT_CHARS;
        assert_eq!(effective_item_length(usize::MAX, 1), derived_chars);
        assert_eq!(effective_item_length(usize::MAX, 2), derived_chars / 2);
        assert_eq!(effective_item_length(usize::MAX, 20), derived_chars / 20);
        assert_eq!(effective_item_length(5_000, 20), 5_000);
        assert_eq!(effective_item_length(usize::MAX, usize::MAX), 1);
    }

    #[test]
    fn batch_and_crawl_blocks_have_bounded_url_labels_and_exact_omitted_count() {
        let block = labeled_content("https://example.com/a", "alpha");
        assert_eq!(block, "URL: https://example.com/a\n\nalpha");

        let oversized_url = format!("https://example.com/{}", "x".repeat(MAX_MCP_PREBUILD_TEXT_CHARS));
        let block = labeled_content(&oversized_url, "unreachable content");
        assert!(block.chars().count() <= MAX_MCP_PREBUILD_TEXT_CHARS);
        assert!(block.starts_with("URL: https://example.com/"));
        assert!(!block.contains("unreachable content"));
        assert!(block.ends_with("<output truncated>"));

        let summary = CallToolResult::success(vec![ContentBlock::text(
            "<response truncated: omitted 2 content blocks>",
        )]);
        let mut output = TextOutput::with_limit(false, serialized_len(&summary));
        assert!(!output.push(labeled_content("https://example.com/a", &"x".repeat(200))));
        assert!(!output.push(labeled_content("https://example.com/b", &"y".repeat(200))));
        let result = output.finish();
        assert_eq!(
            text_blocks(&result),
            vec!["<response truncated: omitted 2 content blocks>"]
        );
    }

    #[test]
    fn map_appends_only_complete_utf8_url_lines_and_reports_counts() {
        let urls = vec![
            "https://example.com/café".to_string(),
            format!("https://example.com/{}", "😀".repeat(100)),
        ];
        let first_chars = urls[0].chars().count();
        let text = map_text_with_limit(urls, first_chars + 70);
        assert!(text.starts_with("https://example.com/café\n"));
        assert!(!text.contains('😀'));
        assert!(text.contains("returned 1 URLs, omitted 1 URLs"));
        assert!(text.chars().count() <= first_chars + 70);
    }

    #[test]
    fn javascript_result_and_console_share_derived_guard_with_exact_omitted_count() {
        let messages = [
            ("Log".to_string(), "first"),
            ("Warn".to_string(), "second"),
            ("Error".to_string(), "third"),
        ];
        let text = javascript_text_with_limit(&"é".repeat(70), messages.into_iter(), 120);
        assert!(text.chars().count() <= 120);
        assert!(text.contains("omitted 3 console messages"));
        assert!(!text.contains("[Log] first"));
    }

    #[test]
    fn javascript_console_summary_never_silently_truncates_the_result() {
        let complete_result = "x".repeat(99);
        let text = javascript_text_with_limit(&complete_result, [("Log".to_string(), "message")].into_iter(), 100);
        assert!(text.chars().count() <= 100);
        assert!(text.contains("<output truncated>"));
        assert!(text.contains("<console output truncated: omitted 1 console messages>"));

        let already_truncated = "y".repeat(101);
        let text = javascript_text_with_limit(&already_truncated, [("Warn".to_string(), "message")].into_iter(), 100);
        assert!(text.chars().count() <= 100);
        assert!(text.contains("<output truncated>"));
        assert!(text.contains("<console output truncated: omitted 1 console messages>"));
    }

    #[test]
    fn javascript_bounds_control_containing_result_before_sanitizing() {
        let result = format!("{}unreachable", "\x01".repeat(10_000));
        let text = javascript_text_with_limit(&result, std::iter::empty(), 100);

        assert!(text.chars().count() <= 100);
        assert!(!text.contains("unreachable"));
        assert!(text.contains("<output truncated>"));
    }

    #[test]
    fn map_and_javascript_console_bound_items_before_sanitizing() {
        let oversized = format!("{}unreachable", "\x01".repeat(10_000));

        let map = map_text_with_limit([oversized.clone()], 100);
        assert!(map.chars().count() <= 100);
        assert!(!map.contains("unreachable"));
        assert!(map.contains("returned 0 URLs, omitted 1 URLs"));

        let javascript =
            javascript_text_with_limit("result", [("Log".to_string(), oversized.as_str())].into_iter(), 100);
        assert!(javascript.chars().count() <= 100);
        assert!(!javascript.contains("unreachable"));
        assert!(javascript.contains("omitted 1 console messages"));
    }

    #[test]
    fn checked_base64_length_handles_cap_and_arithmetic_overflow_without_allocation() {
        assert_eq!(checked_base64_length(6, 8).unwrap(), 8);
        assert!(checked_base64_length(7, 8).is_err());
        assert!(checked_base64_length(usize::MAX, usize::MAX).is_err());
    }
}
