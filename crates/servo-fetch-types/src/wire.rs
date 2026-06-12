use serde::{Deserialize, Serialize};

/// Visibility-aware filtering policy applied during extraction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(ts_rs::TS), ts(export, export_to = "index.ts"))]
#[serde(rename_all = "lowercase")]
pub enum Visibility {
    /// Strip CSS-, ARIA-, and geometry-hidden content.
    Moderate,
    /// Moderate, plus screen-reader-only content.
    Strict,
    /// No flag-based stripping; only semantic hides apply.
    Off,
}

/// Output representation for a single-page fetch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(ts_rs::TS), ts(export, export_to = "index.ts"))]
#[serde(rename_all = "camelCase")]
pub enum Format {
    /// Readability-extracted Markdown.
    Markdown,
    /// Readability-extracted structured data.
    Json,
    /// Raw rendered HTML, post-JS execution.
    Html,
    /// Plain text (`document.body.innerText`).
    Text,
    /// Serialized accessibility tree.
    AccessibilityTree,
}

/// Options shared by every single-page operation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(ts_rs::TS), ts(export, export_to = "index.ts"))]
#[serde(rename_all = "camelCase", default)]
pub struct FetchOptions {
    /// Page-load timeout in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional))]
    pub timeout: Option<u32>,
    /// Extra wait in milliseconds after the `load` event, for SPAs.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional))]
    pub settle: Option<u32>,
    /// Override for the `User-Agent` header.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional))]
    pub user_agent: Option<String>,
    /// Netscape-format `cookies.txt` contents to inject.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional))]
    pub cookies: Option<String>,
    /// Visibility filtering policy.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional))]
    pub visibility: Option<Visibility>,
    /// CSS selector to extract a specific section.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional))]
    pub selector: Option<String>,
    /// Allow requests to loopback/private addresses, relaxing the SSRF guard.
    pub allow_private_addresses: bool,
}

/// Readability-extracted article.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(ts_rs::TS), ts(export, export_to = "index.ts"))]
#[serde(rename_all = "camelCase")]
pub struct Article {
    /// Page title.
    pub title: String,
    /// Readable article HTML.
    pub content: String,
    /// Readable content as Markdown.
    pub text_content: String,
    /// Author or byline, if detected.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional))]
    pub byline: Option<String>,
    /// Short excerpt or description.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional))]
    pub excerpt: Option<String>,
    /// Document language (e.g. `en`).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional))]
    pub lang: Option<String>,
    /// Canonical URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional))]
    pub url: Option<String>,
}

/// Severity of a captured console message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(ts_rs::TS), ts(export, export_to = "index.ts"))]
#[serde(rename_all = "lowercase")]
pub enum ConsoleLevel {
    /// `console.log`.
    Log,
    /// `console.info`.
    Info,
    /// `console.warn`.
    Warn,
    /// `console.error`.
    Error,
    /// `console.debug`.
    Debug,
}

/// A console message captured while evaluating JavaScript.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(ts_rs::TS), ts(export, export_to = "index.ts"))]
#[serde(rename_all = "camelCase")]
pub struct ConsoleMessage {
    /// Message severity.
    pub level: ConsoleLevel,
    /// Message text.
    pub message: String,
}

/// Result of evaluating a JavaScript expression on a page.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(ts_rs::TS), ts(export, export_to = "index.ts"))]
#[serde(rename_all = "camelCase")]
pub struct EvaluateResult {
    /// URL the expression ran against.
    pub url: String,
    /// Stringified expression result.
    pub result: String,
    /// Console output captured during evaluation.
    pub console: Vec<ConsoleMessage>,
}

/// One event emitted by a streaming crawl.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(ts_rs::TS), ts(export, export_to = "index.ts"))]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum CrawlEvent {
    /// A successfully crawled page.
    #[serde(rename_all = "camelCase")]
    Page {
        /// URL of the crawled page.
        url: String,
        /// Link depth from the seed URL.
        depth: u32,
        /// RFC 3339 timestamp when the fetch completed.
        fetched_at: String,
        /// Page title, if any.
        #[serde(skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "codegen", ts(optional))]
        title: Option<String>,
        /// Extracted content (Markdown or JSON per request).
        content: String,
        /// Number of links discovered on the page.
        links_found: u32,
    },
    /// A page that failed to fetch.
    #[serde(rename_all = "camelCase")]
    Error {
        /// URL that failed.
        url: String,
        /// Link depth from the seed URL.
        depth: u32,
        /// RFC 3339 timestamp when the attempt completed.
        fetched_at: String,
        /// Failure description.
        error: String,
    },
    /// Terminal summary emitted once the crawl finishes.
    #[serde(rename_all = "camelCase")]
    Stats {
        /// Pages crawled successfully.
        crawled: u32,
        /// Pages that errored.
        errors: u32,
    },
}

/// A URL discovered by sitemap mapping.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(ts_rs::TS), ts(export, export_to = "index.ts"))]
#[serde(rename_all = "camelCase")]
pub struct MappedUrl {
    /// The discovered URL.
    pub url: String,
    /// Last-modified timestamp from the sitemap, if present.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional))]
    pub lastmod: Option<String>,
}

/// Per-URL result of a batch fetch.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(ts_rs::TS), ts(export, export_to = "index.ts"))]
#[serde(rename_all = "camelCase")]
pub struct BatchResult {
    /// The fetched URL.
    pub url: String,
    /// Whether the fetch succeeded.
    pub ok: bool,
    /// Markdown content when `ok` is true.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional))]
    pub markdown: Option<String>,
    /// Error message when `ok` is false.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional))]
    pub error: Option<String>,
}

/// A declarative CSS-selector extraction schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(ts_rs::TS), ts(export, export_to = "index.ts"))]
#[serde(rename_all = "camelCase")]
pub struct Schema {
    /// Repeated container selector; each match yields one object.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "codegen", ts(optional))]
    pub base_selector: Option<String>,
    /// Fields read from each container.
    pub fields: Vec<Field>,
}

/// One field within a [`Schema`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(ts_rs::TS), ts(export, export_to = "index.ts"))]
#[serde(rename_all = "camelCase")]
pub struct Field {
    /// Output key for this field.
    pub name: String,
    /// CSS selector relative to the current container.
    pub selector: String,
    /// How to read the value once the selector matches.
    #[serde(flatten)]
    pub kind: FieldKind,
}

/// How a [`Field`] reads its value.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(ts_rs::TS), ts(export, export_to = "index.ts"))]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum FieldKind {
    /// Descendant text of the first match.
    Text,
    /// A named attribute on the first match.
    Attribute {
        /// Attribute name to read (e.g. `href`).
        attribute: String,
    },
    /// Outer HTML of the first match.
    Html,
    /// Inner HTML of the first match.
    InnerHtml,
    /// Repeated sub-object per match, using nested fields.
    NestedList {
        /// Nested field definitions.
        fields: Vec<Field>,
    },
}
