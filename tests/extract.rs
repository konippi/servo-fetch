//! Content extraction integration tests — validates the full extraction pipeline
//! using HTML fixtures. These tests do NOT require Servo.

use servo_fetch::extract::{self, ExtractInput};

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!("tests/fixtures/{name}")).expect("fixture exists")
}

fn input(html: &str) -> ExtractInput<'_> {
    ExtractInput::new(html, "https://example.com")
}

#[test]
fn extracts_article_title() {
    let html = fixture("article.html");
    let text = extract::extract_text(&input(&html)).unwrap();
    assert!(text.contains("Test Article Title"));
}

#[test]
fn extracts_article_content() {
    let html = fixture("article.html");
    let text = extract::extract_text(&input(&html)).unwrap();
    assert!(text.contains("main content of the article"));
}

#[test]
fn simple_page_extracts_text() {
    let html = fixture("simple.html");
    let text = extract::extract_text(&input(&html)).unwrap();
    assert!(text.contains("Hello World"));
}

#[test]
fn json_output_is_valid() {
    let html = fixture("article.html");
    let json = extract::extract_json(&input(&html)).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
    assert!(parsed.get("title").is_some());
    assert!(parsed.get("text_content").is_some());
}

#[test]
fn json_simple_page() {
    let html = fixture("simple.html");
    let json = extract::extract_json(&input(&html)).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
    assert!(parsed["text_content"].as_str().unwrap().contains("Hello World"));
}

#[test]
fn inner_text_fallback() {
    let mut input = ExtractInput::new("<html><body></body></html>", "https://example.com");
    input.inner_text = Some("fallback text");
    let text = extract::extract_text(&input).unwrap();
    assert!(text.contains("fallback text"));
}

#[test]
fn handles_unicode_content() {
    let html = fixture("unicode.html");
    let text = extract::extract_text(&input(&html)).unwrap();
    assert!(text.contains("見出し"));
}

#[test]
fn strips_script_and_style() {
    let html = fixture("with_scripts.html");
    let text = extract::extract_text(&input(&html)).unwrap();
    assert!(!text.contains("alert"));
    assert!(text.contains("visible content"));
}

#[test]
fn empty_body_without_inner_text_returns_empty() {
    let input = ExtractInput::new("<html><body></body></html>", "https://example.com");
    let text = extract::extract_text(&input).unwrap();
    assert!(text.is_empty());
}

#[test]
fn byline_and_excerpt_rendered_in_markdown() {
    let html = fixture("article_with_meta.html");
    let text = extract::extract_text(&input(&html)).unwrap();
    assert!(text.contains("*Jane Doe*"), "byline should be italic");
    assert!(text.contains("> A short summary"), "excerpt should be blockquote");
}

#[test]
fn selector_extracts_specific_element() {
    let html = fixture("article.html");
    let mut input = input(&html);
    input.selector = Some("article");
    let text = extract::extract_text(&input).unwrap();
    assert!(text.contains("Test Article Title"));
    assert!(!text.contains("Sidebar item"), "sidebar should not be included");
}

#[test]
fn selector_no_match_returns_empty() {
    let html = fixture("simple.html");
    let mut input = input(&html);
    input.selector = Some(".nonexistent");
    let text = extract::extract_text(&input).unwrap();
    assert!(text.is_empty());
}

#[test]
fn extract_json_selector_includes_url() {
    let html = fixture("article.html");
    let input = ExtractInput::new(&html, "https://example.com/page").with_selector(Some("article"));
    let json = extract::extract_json(&input).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
    assert_eq!(parsed["url"].as_str(), Some("https://example.com/page"));
}

#[test]
fn extract_text_with_layout_and_selector() {
    let html = fixture("article.html");
    let layout = r#"[{"tag":"NAV","role":null,"w":1280,"h":50,"position":"fixed"}]"#;
    let input = ExtractInput::new(&html, "https://example.com")
        .with_layout_json(Some(layout))
        .with_selector(Some("article"));
    let text = extract::extract_text(&input).unwrap();
    assert!(text.contains("Test Article Title"));
}
