//! Content extraction — converts raw HTML into readable Markdown or structured JSON.

use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt::{self, Write as _};

use dom_query::Document;
use dom_smoothie::Readability;
use htmd::HtmlToMarkdown;
use serde::Serialize;
use servo::accesskit::{Node, NodeId};

use crate::layout::{self, LayoutElement};
use crate::visibility::{self, A11yIndex, VisibilityPolicy};

/// Errors that can occur during content extraction.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ExtractError {
    /// Failed to format Markdown output.
    #[error("markdown formatting failed")]
    Fmt(#[from] fmt::Error),
    /// Failed to serialize JSON output.
    #[error("JSON serialization failed")]
    Json(#[from] serde_json::Error),
    /// The provided CSS selector is invalid.
    #[error("invalid CSS selector")]
    InvalidSelector,
}

/// Structured article data for JSON output.
#[derive(Serialize)]
#[non_exhaustive]
pub struct ArticleData {
    /// Page title.
    pub title: String,
    /// Raw HTML content extracted by Readability.
    pub content: String,
    /// Readable text content (Markdown).
    pub text_content: String,
    /// Author or byline, if detected.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub byline: Option<String>,
    /// Short excerpt or description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub excerpt: Option<String>,
    /// Document language (e.g. "en").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
    /// Canonical URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// Extract text content from a PDF byte slice, or an empty string on failure.
#[must_use]
pub fn extract_pdf(data: &[u8]) -> String {
    match pdf_extract::extract_text_from_mem(data) {
        Ok(text) => text,
        Err(e) => {
            tracing::warn!(error = %e, "PDF text extraction failed");
            String::new()
        }
    }
}

/// Input parameters for content extraction.
#[non_exhaustive]
pub struct ExtractInput<'a> {
    /// Raw HTML of the page.
    pub html: &'a str,
    /// URL of the page (used for resolving relative links).
    pub url: &'a str,
    /// JSON-serialized layout data from the injected JS, if available.
    pub layout_json: Option<&'a str>,
    /// JSON-serialized visibility data from the injected JS, if available.
    pub visibility_json: Option<&'a str>,
    /// AccessKit accessibility tree, if available.
    pub a11y: Option<&'a HashMap<NodeId, Node>>,
    /// `document.body.innerText` fallback, if available.
    pub inner_text: Option<&'a str>,
    /// CSS selector to extract a specific section instead of using Readability.
    pub selector: Option<&'a str>,
    /// Visibility policy controlling which hidden content is stripped.
    pub visibility: VisibilityPolicy,
}

impl<'a> ExtractInput<'a> {
    /// Create a new `ExtractInput` with required fields.
    #[must_use]
    pub fn new(html: &'a str, url: &'a str) -> Self {
        Self {
            html,
            url,
            layout_json: None,
            visibility_json: None,
            a11y: None,
            inner_text: None,
            selector: None,
            visibility: VisibilityPolicy::default(),
        }
    }

    /// Set the layout JSON data.
    #[must_use]
    pub fn with_layout_json(mut self, layout_json: Option<&'a str>) -> Self {
        self.layout_json = layout_json;
        self
    }

    /// Set the visibility JSON data.
    #[must_use]
    pub fn with_visibility_json(mut self, visibility_json: Option<&'a str>) -> Self {
        self.visibility_json = visibility_json;
        self
    }

    /// Set the typed accessibility tree.
    #[must_use]
    pub fn with_a11y(mut self, a11y: Option<&'a HashMap<NodeId, Node>>) -> Self {
        self.a11y = a11y;
        self
    }

    /// Set the inner text fallback.
    #[must_use]
    pub fn with_inner_text(mut self, inner_text: Option<&'a str>) -> Self {
        self.inner_text = inner_text;
        self
    }

    /// Set the CSS selector for targeted extraction.
    #[must_use]
    pub fn with_selector(mut self, selector: Option<&'a str>) -> Self {
        self.selector = selector;
        self
    }

    /// Set the visibility policy.
    #[must_use]
    pub fn with_visibility(mut self, policy: VisibilityPolicy) -> Self {
        self.visibility = policy;
        self
    }
}

/// Extract readable content as Markdown text.
pub fn extract_text(input: &ExtractInput<'_>) -> Result<String, ExtractError> {
    if let Some(selector) = input.selector {
        return extract_by_selector(input, selector);
    }
    let article = parse_article(input);

    let mut out = String::new();
    if !article.title.is_empty() {
        writeln!(out, "# {}\n", article.title)?;
    }
    if let Some(ref byline) = article.byline {
        writeln!(out, "*{}*\n", byline.replace('*', r"\*"))?;
    }
    if let Some(ref excerpt) = article.excerpt {
        writeln!(out, "> {excerpt}\n")?;
    }
    write!(out, "{}", article.text_content)?;
    Ok(clean_markdown(&out))
}

/// Extract readable content as JSON.
pub fn extract_json(input: &ExtractInput<'_>) -> Result<String, ExtractError> {
    if let Some(selector) = input.selector {
        let text = extract_by_selector(input, selector)?;
        let data = ArticleData {
            title: String::new(),
            content: String::new(),
            text_content: text,
            byline: None,
            excerpt: None,
            lang: None,
            url: Some(input.url.to_string()),
        };
        return Ok(serde_json::to_string_pretty(&data)?);
    }
    let article = parse_article(input);
    let data = ArticleData {
        title: article.title,
        content: article.content,
        text_content: article.text_content,
        byline: article.byline,
        excerpt: article.excerpt,
        lang: article.lang,
        url: Some(input.url.to_string()),
    };
    Ok(serde_json::to_string_pretty(&data)?)
}

struct ParsedArticle {
    title: String,
    content: String,
    text_content: String,
    byline: Option<String>,
    excerpt: Option<String>,
    lang: Option<String>,
}

fn is_nextjs_error_page(text: &str) -> bool {
    let t = text.trim();
    t.contains("client-side exception has occurred") || t.contains("Application error: a")
}

fn parse_article(input: &ExtractInput<'_>) -> ParsedArticle {
    let filtered = filter(input);

    let doc = Document::from(filtered.as_ref());
    if let Ok(mut readability) = Readability::with_document(doc, Some(input.url), None) {
        if let Ok(article) = readability.parse() {
            if !is_nextjs_error_page(&article.text_content) {
                let converter = HtmlToMarkdown::builder().build();
                let markdown = converter
                    .convert(&article.content)
                    .unwrap_or_else(|_| article.text_content.to_string());
                return ParsedArticle {
                    title: article.title.clone(),
                    content: article.content.to_string(),
                    text_content: markdown,
                    byline: article.byline.clone(),
                    excerpt: article.excerpt.clone(),
                    lang: article.lang,
                };
            }
        }
    }

    // Readability failed or returned an error page — fall back to the filtered
    // document's text content.
    let doc = Document::from(filtered.as_ref());
    doc.select("script, style, noscript").remove();
    let title = doc.select("title").text().to_string();
    let filtered_text = doc.select("body").text().to_string();
    let body_text = if filtered_text.trim().is_empty() {
        input.inner_text.filter(|s| !s.trim().is_empty()).map_or_else(
            || {
                tracing::warn!(r#"could not extract content; try --js "document.body.innerText" for JS-heavy sites"#);
                String::new()
            },
            String::from,
        )
    } else {
        filtered_text
    };
    ParsedArticle {
        title,
        content: String::new(),
        text_content: body_text,
        byline: None,
        excerpt: None,
        lang: None,
    }
}

fn extract_by_selector(input: &ExtractInput<'_>, selector: &str) -> Result<String, ExtractError> {
    let matcher = dom_query::Matcher::new(selector).map_err(|_| ExtractError::InvalidSelector)?;
    let filtered = filter(input);
    let doc = Document::from(filtered.as_ref());
    let selected = doc.select_matcher(&matcher);
    let fragment = selected.html();
    if fragment.is_empty() {
        return Ok(String::new());
    }
    let converter = HtmlToMarkdown::builder().skip_tags(vec!["script", "style"]).build();
    let markdown = converter
        .convert(&fragment)
        .unwrap_or_else(|_| selected.text().to_string());
    Ok(clean_markdown(&markdown))
}

fn filter<'a>(input: &'a ExtractInput<'a>) -> Cow<'a, str> {
    let mut selectors: Vec<String> = Vec::new();

    if let Some(lj) = input.layout_json
        && let Ok(els) = serde_json::from_str::<Vec<LayoutElement>>(lj)
    {
        selectors.extend(layout::selectors_to_strip(&els));
    }

    let a11y_index = input.a11y.map(A11yIndex::new);

    selectors.extend(visibility::selectors_to_strip(
        input.visibility,
        a11y_index.as_ref(),
        input.visibility_json,
    ));

    let needs_attr_cleanup = input.visibility_json.is_some() || input.html.contains("data-vf-id=");
    if selectors.is_empty() && !needs_attr_cleanup {
        return Cow::Borrowed(input.html);
    }

    let doc = Document::from(input.html);
    for sel in &selectors {
        doc.select(sel).remove();
    }
    if needs_attr_cleanup {
        doc.select("[data-vf-id]").remove_attr("data-vf-id");
    }
    Cow::Owned(doc.html().to_string())
}

// Collapse runs of 3+ blank lines down to 2.
fn clean_markdown(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut blank_count = 0u8;
    for line in input.lines() {
        if line.trim().is_empty() {
            blank_count = blank_count.saturating_add(1);
            if blank_count <= 2 {
                result.push('\n');
            }
        } else {
            blank_count = 0;
            result.push_str(line);
            result.push('\n');
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_nextjs_error_page_detects_nextjs() {
        assert!(is_nextjs_error_page(
            "Application error: a client-side exception has occurred"
        ));
    }

    #[test]
    fn is_nextjs_error_page_ignores_normal_content() {
        assert!(!is_nextjs_error_page("This article discusses error handling in Rust."));
        assert!(!is_nextjs_error_page(
            "A long page about many topics that happens to mention errors somewhere in the middle of a paragraph."
        ));
    }

    #[test]
    fn clean_markdown_collapses_blank_lines() {
        let input = "line1\n\n\n\n\nline2\n";
        let result = clean_markdown(input);
        assert_eq!(result, "line1\n\n\nline2\n");
    }

    #[test]
    fn clean_markdown_preserves_single_blank() {
        let input = "a\n\nb\n";
        assert_eq!(clean_markdown(input), "a\n\nb\n");
    }

    #[test]
    fn filter_off_policy_keeps_visible_content() {
        let input = ExtractInput::new("<html><body>hello</body></html>", "").with_visibility(VisibilityPolicy::off());
        let result = filter(&input);
        assert!(result.contains("hello"));
    }

    #[test]
    fn filter_strips_footer() {
        let html = r#"<html><body><footer style="position:static">nav</footer><p>content</p></body></html>"#;
        let layout = r#"[{"tag":"FOOTER","role":null,"w":1280,"h":100,"position":"static"}]"#;
        let input = ExtractInput::new(html, "")
            .with_layout_json(Some(layout))
            .with_visibility(VisibilityPolicy::off());
        let result = filter(&input);
        assert!(!result.contains("<footer"));
        assert!(result.contains("content"));
    }

    #[test]
    fn filter_strips_visibility_flagged_element() {
        let html = r#"<html><body><p data-vf-id="1">drop</p><p data-vf-id="2">keep</p></body></html>"#;
        let visibility = r#"[{"id":"1","flags":16}]"#;
        let input = ExtractInput::new(html, "")
            .with_visibility_json(Some(visibility))
            .with_visibility(VisibilityPolicy::moderate());
        let result = filter(&input);
        assert!(!result.contains("drop"));
        assert!(result.contains("keep"));
    }

    #[test]
    fn filter_removes_data_vf_id_from_output() {
        let html = r#"<html><body><p data-vf-id="1">keep</p></body></html>"#;
        let input = ExtractInput::new(html, "")
            .with_layout_json(Some("[]"))
            .with_visibility(VisibilityPolicy::off());
        let result = filter(&input);
        assert!(!result.contains("data-vf-id"));
    }

    #[test]
    fn extract_input_builder() {
        let input = ExtractInput::new("<html></html>", "https://example.com")
            .with_layout_json(Some("[]"))
            .with_visibility_json(Some(r"[]"))
            .with_inner_text(Some("hello"))
            .with_selector(Some("article"))
            .with_visibility(VisibilityPolicy::strict());
        assert_eq!(input.layout_json, Some("[]"));
        assert_eq!(input.visibility_json, Some("[]"));
        assert_eq!(input.inner_text, Some("hello"));
        assert_eq!(input.selector, Some("article"));
    }

    #[test]
    fn clean_markdown_no_trailing_newline() {
        let input = "line1\nline2";
        let result = clean_markdown(input);
        assert_eq!(result, "line1\nline2\n");
    }
}
