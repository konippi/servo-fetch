use serde::{Deserialize, Serialize};

/// Visibility-aware filtering policy applied during extraction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(ts_rs::TS), ts(export, export_to = "index.ts"))]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum Visibility {
    /// Strip CSS-, ARIA-, and geometry-hidden content.
    Moderate,
    /// Moderate, plus screen-reader-only content.
    Strict,
    /// No flag-based stripping; only semantic hides apply.
    Off,
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
    /// `console.trace`.
    Trace,
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
        #[cfg_attr(feature = "codegen", ts(type = "number"))]
        depth: u64,
        /// RFC 3339 timestamp when the fetch completed.
        fetched_at: String,
        /// Page title, if any.
        #[serde(skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "codegen", ts(optional))]
        title: Option<String>,
        /// Extracted content (Markdown or JSON per request).
        content: String,
        /// Number of links discovered on the page.
        #[cfg_attr(feature = "codegen", ts(type = "number"))]
        links_found: u64,
    },
    /// A page that failed to fetch.
    #[serde(rename_all = "camelCase")]
    Error {
        /// URL that failed.
        url: String,
        /// Link depth from the seed URL.
        #[cfg_attr(feature = "codegen", ts(type = "number"))]
        depth: u64,
        /// RFC 3339 timestamp when the attempt completed.
        fetched_at: String,
        /// Failure description.
        error: String,
    },
    /// Terminal summary emitted once the crawl finishes.
    #[serde(rename_all = "camelCase")]
    Stats {
        /// Pages crawled successfully.
        #[cfg_attr(feature = "codegen", ts(type = "number"))]
        crawled: u64,
        /// Pages that errored.
        #[cfg_attr(feature = "codegen", ts(type = "number"))]
        errors: u64,
        /// Total wall-clock time in milliseconds.
        #[cfg_attr(feature = "codegen", ts(type = "number"))]
        elapsed_ms: u64,
    },
}

/// Terminal summary returned by the `crawl` method once the stream completes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(ts_rs::TS), ts(export, export_to = "index.ts"))]
#[serde(rename_all = "camelCase")]
pub struct CrawlStats {
    /// Pages crawled successfully.
    #[cfg_attr(feature = "codegen", ts(type = "number"))]
    pub crawled: u64,
    /// Pages that errored.
    #[cfg_attr(feature = "codegen", ts(type = "number"))]
    pub errors: u64,
    /// Total wall-clock time in milliseconds.
    #[cfg_attr(feature = "codegen", ts(type = "number"))]
    pub elapsed_ms: u64,
}

/// Structured-extraction result for the `--schema` path.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(ts_rs::TS), ts(export, export_to = "index.ts"))]
#[serde(rename_all = "camelCase")]
pub struct SchemaExtractResult {
    /// URL the schema ran against.
    pub url: String,
    /// Extracted value; shape depends on the schema.
    #[cfg_attr(feature = "codegen", ts(type = "unknown"))]
    pub extracted: serde_json::Value,
}

/// Error payload returned by every non-streaming failure.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(ts_rs::TS), ts(export, export_to = "index.ts"))]
#[serde(rename_all = "camelCase")]
pub struct ErrorEnvelope {
    /// Human-readable error message.
    pub error: String,
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

/// Server name and version, reported by `initialize`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(ts_rs::TS), ts(export, export_to = "index.ts"))]
#[serde(rename_all = "camelCase")]
pub struct ServerInfo {
    /// Server name.
    pub name: String,
    /// Server version.
    pub version: String,
}

/// Capabilities advertised by the server (none yet; reserved for forward compatibility).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(ts_rs::TS), ts(export, export_to = "index.ts"))]
#[serde(rename_all = "camelCase")]
pub struct ServerCapabilities {}

/// Result of the `initialize` handshake.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(ts_rs::TS), ts(export, export_to = "index.ts"))]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    /// Wire protocol version the server speaks.
    pub protocol_version: u32,
    /// Server name and version.
    pub server_info: ServerInfo,
    /// Capabilities the server advertises.
    pub capabilities: ServerCapabilities,
}

/// Stable, surface-independent error category for programmatic handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(ts_rs::TS), ts(export, export_to = "index.ts"))]
#[serde(rename_all = "camelCase")]
pub enum ErrorKind {
    /// The URL is malformed or uses a disallowed scheme.
    InvalidUrl,
    /// The page did not finish loading within the timeout.
    Timeout,
    /// The URL resolves to a private or reserved address (SSRF block).
    AddressNotAllowed,
    /// The browser engine failed to render the page.
    Engine,
    /// JavaScript evaluation failed.
    Javascript,
    /// A request parameter was missing, malformed, or out of range.
    InvalidParams,
    /// The requested method does not exist.
    MethodNotFound,
    /// A frame could not be parsed.
    ParseError,
    /// The request was cancelled before completing.
    RequestCancelled,
    /// A request reused an in-flight request id.
    DuplicateRequest,
    /// An unexpected server-side failure.
    Internal,
    /// An error that does not map to a known category.
    Other,
}
