//! Content extraction — converts raw HTML into readable Markdown or structured JSON.

use std::borrow::Cow;
use std::fmt::Write;

use dom_query::Document;
use dom_smoothie::Readability;
use htmd::HtmlToMarkdown;
use serde::Serialize;

use crate::layout::{self, LayoutElement};

/// Errors that can occur during content extraction.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ExtractError {
    /// Failed to format Markdown output.
    #[error("markdown formatting failed")]
    Fmt(#[from] std::fmt::Error),
    /// Failed to serialize JSON output.
    #[error("JSON serialization failed")]
    Json(#[from] serde_json::Error),
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

/// Extract text content from a PDF byte slice.
///
/// Returns the extracted text, or an empty string if extraction fails.
#[must_use]
pub fn extract_pdf(data: &[u8]) -> String {
    match pdf_extract::extract_text_from_mem(data) {
        Ok(text) => text,
        Err(e) => {
            eprintln!("warning: PDF text extraction failed: {e}");
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
    /// `document.body.innerText` fallback, if available.
    pub inner_text: Option<&'a str>,
    /// CSS selector to extract a specific section instead of using Readability.
    pub selector: Option<&'a str>,
}

impl<'a> ExtractInput<'a> {
    /// Create a new `ExtractInput` with required fields.
    #[must_use]
    pub fn new(html: &'a str, url: &'a str) -> Self {
        Self {
            html,
            url,
            layout_json: None,
            inner_text: None,
            selector: None,
        }
    }

    /// Set the layout JSON data.
    #[must_use]
    pub fn with_layout_json(mut self, layout_json: Option<&'a str>) -> Self {
        self.layout_json = layout_json;
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
}

/// Extract readable content as Markdown text.
///
/// # Errors
///
/// Returns [`ExtractError::Fmt`] if the Markdown assembly fails.
pub fn extract_text(input: &ExtractInput<'_>) -> Result<String, ExtractError> {
    if let Some(selector) = input.selector {
        return Ok(extract_by_selector(input.html, input.layout_json, selector));
    }
    let article = parse_article(input.html, input.url, input.layout_json, input.inner_text);

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
///
/// # Errors
///
/// Returns [`ExtractError::Json`] if JSON serialization fails.
pub fn extract_json(input: &ExtractInput<'_>) -> Result<String, ExtractError> {
    if let Some(selector) = input.selector {
        let text = extract_by_selector(input.html, input.layout_json, selector);
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
    let article = parse_article(input.html, input.url, input.layout_json, input.inner_text);
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

fn parse_article(html: &str, url: &str, layout_json: Option<&str>, inner_text: Option<&str>) -> ParsedArticle {
    let filtered = filter(html, layout_json);

    let doc = Document::from(filtered.as_ref());
    if let Ok(mut readability) = Readability::with_document(doc, Some(url), None) {
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

    // Readability failed or returned an error page — fall back to innerText.
    let doc = Document::from(filtered.as_ref());
    let title = doc.select("title").text().to_string();
    let body_text = inner_text.filter(|s| !s.trim().is_empty()).map_or_else(
        || {
            eprintln!(
                "warning: could not extract content. \
                 Try --js \"document.body.innerText\" for JS-heavy sites."
            );
            String::new()
        },
        String::from,
    );
    ParsedArticle {
        title,
        content: String::new(),
        text_content: body_text,
        byline: None,
        excerpt: None,
        lang: None,
    }
}

fn extract_by_selector(html: &str, layout_json: Option<&str>, selector: &str) -> String {
    let filtered = filter(html, layout_json);
    let doc = Document::from(filtered.as_ref());
    let selected = doc.select(selector);
    let fragment = selected.html();
    if fragment.is_empty() {
        return String::new();
    }
    let converter = HtmlToMarkdown::builder().skip_tags(vec!["script", "style"]).build();
    let markdown = converter
        .convert(&fragment)
        .unwrap_or_else(|_| selected.text().to_string());
    clean_markdown(&markdown)
}

fn filter<'a>(html: &'a str, layout_json: Option<&str>) -> Cow<'a, str> {
    layout_json
        .and_then(|lj| serde_json::from_str::<Vec<LayoutElement>>(lj).ok())
        .map_or(Cow::Borrowed(html), |els| {
            let sels = layout::selectors_to_strip(&els);
            if sels.is_empty() {
                return Cow::Borrowed(html);
            }
            let doc = Document::from(html);
            for sel in &sels {
                doc.select(sel).remove();
            }
            Cow::Owned(doc.html().to_string())
        })
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
    fn filter_without_layout_returns_original() {
        let html = "<html><body>hello</body></html>";
        let result = filter(html, None);
        assert_eq!(result.as_ref(), html);
    }

    #[test]
    fn filter_strips_footer() {
        let html = r#"<html><body><footer style="position:static">nav</footer><p>content</p></body></html>"#;
        let layout = r#"[{"tag":"FOOTER","role":null,"w":1280,"h":100,"position":"static"}]"#;
        let result = filter(html, Some(layout));
        assert!(!result.contains("<footer"));
        assert!(result.contains("content"));
    }

    #[test]
    fn extract_input_builder() {
        let input = ExtractInput::new("<html></html>", "https://example.com")
            .with_layout_json(Some("[]"))
            .with_inner_text(Some("hello"))
            .with_selector(Some("article"));
        assert_eq!(input.layout_json, Some("[]"));
        assert_eq!(input.inner_text, Some("hello"));
        assert_eq!(input.selector, Some("article"));
    }
}
