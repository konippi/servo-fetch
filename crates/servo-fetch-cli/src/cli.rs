//! CLI argument parsing.

use clap::builder::NonEmptyStringValueParser;
use clap::{ArgAction, Args, Parser, Subcommand, ValueEnum, value_parser};

#[derive(Parser)]
#[command(
    name = "servo-fetch",
    version,
    about = "A browser engine in a binary — fetch, render, and extract web content."
)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    #[command(flatten)]
    pub fetch: FetchArgs,

    /// Increase log verbosity (`-v` info, `-vv` debug, `-vvv` trace)
    #[arg(short = 'v', long, action = ArgAction::Count, global = true, conflicts_with = "quiet")]
    pub verbose: u8,

    /// Suppress all logs except errors
    #[arg(short = 'q', long, global = true)]
    pub quiet: bool,

    /// Allow requests to loopback/private addresses (for testing with local servers).
    #[arg(long = "allow-private-addresses", hide = true, global = true)]
    pub allow_private_addresses: bool,
}

#[derive(Args, Debug)]
pub(crate) struct FetchArgs {
    /// URLs to fetch (one or more)
    #[arg(num_args = 1..)]
    pub urls: Vec<String>,

    /// Output format
    #[arg(long, value_enum, value_name = "FORMAT", default_value_t = Format::Markdown,
          conflicts_with_all = ["screenshot", "js"])]
    pub format: Format,

    /// Save screenshot as PNG (single URL only)
    #[arg(long, value_name = "FILE", conflicts_with_all = ["js"])]
    pub screenshot: Option<String>,

    /// Capture the full scrollable page instead of just the viewport.
    #[arg(long, requires = "screenshot")]
    pub full_page: bool,

    /// Execute JavaScript and print the result (single URL only)
    #[arg(long, value_name = "EXPR", conflicts_with_all = ["screenshot"])]
    pub js: Option<String>,

    /// Timeout in seconds for page load
    #[arg(short = 't', long, default_value_t = 30, value_parser = value_parser!(u64).range(1..), value_name = "SECS")]
    pub timeout: u64,

    /// Extra wait in ms after the `load` event, for SPAs that keep hydrating.
    #[arg(long, default_value_t = 0, value_parser = value_parser!(u64).range(0..=10_000), value_name = "MS")]
    pub settle: u64,

    /// CSS selector to extract a specific section
    #[arg(long, value_name = "CSS", value_parser = NonEmptyStringValueParser::new())]
    pub selector: Option<String>,

    /// Override the User-Agent string
    #[arg(long, value_name = "UA")]
    pub user_agent: Option<String>,

    /// Path to a CSS-selector schema file for structured JSON extraction
    #[arg(long, value_name = "FILE", conflicts_with_all = ["screenshot", "js", "selector", "format"])]
    pub schema: Option<std::path::PathBuf>,

    /// Save the rendered output to a single file (single URL only).
    #[arg(short = 'o', long, value_name = "FILE", conflicts_with_all = ["screenshot", "output_dir"])]
    pub output: Option<std::path::PathBuf>,

    /// Write each URL's output to a file in this directory (auto-created).
    #[arg(long, value_name = "DIR", conflicts_with = "screenshot")]
    pub output_dir: Option<std::path::PathBuf>,

    /// Visibility-aware filtering policy.
    #[arg(long, value_name = "POLICY", value_enum, default_value_t = VisibilityArg::Moderate)]
    pub visibility: VisibilityArg,
}

/// Visibility filtering policy.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub(crate) enum VisibilityArg {
    /// Strip CSS-, ARIA-, and geometry-hidden content (default).
    Moderate,
    /// Moderate plus screen-reader-only content.
    Strict,
    /// No flag-based stripping. ARIA/HTML semantic hides still apply.
    Off,
}

impl VisibilityArg {
    pub(crate) fn to_policy(self) -> servo_fetch::VisibilityPolicy {
        match self {
            Self::Moderate => servo_fetch::VisibilityPolicy::moderate(),
            Self::Strict => servo_fetch::VisibilityPolicy::strict(),
            Self::Off => servo_fetch::VisibilityPolicy::off(),
        }
    }
}

/// Output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum Format {
    /// Readability-extracted Markdown
    Markdown,
    /// Readability-extracted JSON
    Json,
    /// Raw rendered HTML (post-JS execution)
    Html,
    /// Plain text (`document.body.innerText`)
    Text,
}

/// Crawl output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum CrawlFormat {
    /// Readability-extracted Markdown
    Markdown,
    /// NDJSON
    Json,
}

/// Available subcommands.
#[derive(Subcommand)]
pub(crate) enum Command {
    /// Start MCP server (stdio transport by default, or HTTP with --port)
    Mcp(McpArgs),
    /// Start HTTP API server for fetch/screenshot/crawl/map operations.
    Serve(ServeArgs),
    /// Crawl a website by following links (BFS). Respects robots.txt.
    Crawl(CrawlArgs),
    /// Discover URLs on a site via sitemaps (no rendering).
    Map(MapArgs),
}

#[derive(Args, Debug)]
pub(crate) struct McpArgs {
    /// Port for Streamable HTTP transport. Omit for stdio.
    #[arg(long, value_name = "PORT")]
    pub port: Option<u16>,
}

#[derive(Args, Debug)]
pub(crate) struct ServeArgs {
    /// Host to bind on.
    #[arg(long, value_name = "HOST", default_value = "127.0.0.1")]
    pub host: String,

    /// Port to listen on.
    #[arg(long, value_name = "PORT", default_value_t = 3000)]
    pub port: u16,
}

#[derive(Args, Debug)]
pub(crate) struct CrawlArgs {
    /// Starting URL to crawl
    pub url: String,

    /// Maximum number of pages to crawl
    #[arg(long, default_value_t = 50, value_name = "N")]
    pub limit: usize,

    /// Maximum link depth from the seed URL
    #[arg(long, default_value_t = 3, value_name = "N")]
    pub max_depth: usize,

    /// URL path glob patterns to include (e.g. "/docs/**")
    #[arg(long, value_name = "GLOB")]
    pub include: Vec<String>,

    /// URL path glob patterns to exclude (e.g. "/docs/archive/**")
    #[arg(long, value_name = "GLOB")]
    pub exclude: Vec<String>,

    /// Output format
    #[arg(long, value_enum, value_name = "FORMAT", default_value_t = CrawlFormat::Markdown)]
    pub format: CrawlFormat,

    /// CSS selector to extract a specific section per page
    #[arg(long, value_name = "CSS", value_parser = NonEmptyStringValueParser::new())]
    pub selector: Option<String>,

    /// Timeout in seconds per page
    #[arg(short = 't', long, default_value_t = 30, value_parser = value_parser!(u64).range(1..), value_name = "SECS")]
    pub timeout: u64,

    /// Extra wait in ms after load event per page
    #[arg(long, default_value_t = 0, value_parser = value_parser!(u64).range(0..=10_000), value_name = "MS")]
    pub settle: u64,

    /// Maximum parallel page fetches. Yields in completion order when greater than 1.
    #[arg(long, default_value_t = 1, value_parser = value_parser!(u64).range(1..=64), value_name = "N")]
    pub concurrency: u64,

    /// Minimum dispatch interval in ms (0 to disable).
    #[arg(long, default_value_t = 500, value_parser = value_parser!(u64).range(0..=60_000), value_name = "MS")]
    pub delay_ms: u64,

    /// Override the User-Agent string
    #[arg(long, value_name = "UA")]
    pub user_agent: Option<String>,

    /// Write each crawled page's output to a file in this directory.
    #[arg(long, value_name = "DIR")]
    pub output_dir: Option<std::path::PathBuf>,
}

#[derive(Args, Debug)]
pub(crate) struct MapArgs {
    /// Starting URL to discover links from
    pub url: String,

    /// Maximum number of URLs to discover
    #[arg(long, default_value_t = 5000, value_name = "N")]
    pub limit: usize,

    /// URL path glob patterns to include (e.g. "/docs/**")
    #[arg(long, value_name = "GLOB")]
    pub include: Vec<String>,

    /// URL path glob patterns to exclude (e.g. "/docs/archive/**")
    #[arg(long, value_name = "GLOB")]
    pub exclude: Vec<String>,

    /// Output as JSON array with metadata
    #[arg(long)]
    pub json: bool,

    /// Skip HTML link fallback if no sitemap is found
    #[arg(long)]
    pub no_fallback: bool,

    /// Override the User-Agent string
    #[arg(long, value_name = "UA")]
    pub user_agent: Option<String>,

    /// Timeout in seconds per HTTP request
    #[arg(short = 't', long, default_value_t = 30, value_parser = value_parser!(u64).range(1..), value_name = "SECS")]
    pub timeout: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_from_str() {
        use ValueEnum;
        assert!(Format::from_str("markdown", true).is_ok());
        assert!(Format::from_str("json", true).is_ok());
        assert!(Format::from_str("html", true).is_ok());
        assert!(Format::from_str("text", true).is_ok());
        assert!(Format::from_str("xml", true).is_err());
    }

    #[test]
    fn crawl_format_from_str() {
        use ValueEnum;
        assert!(CrawlFormat::from_str("markdown", true).is_ok());
        assert!(CrawlFormat::from_str("json", true).is_ok());
        assert!(CrawlFormat::from_str("html", true).is_err());
    }
}

#[cfg(test)]
mod cli_tests {
    use assert_cmd::Command;
    use predicates::prelude::*;

    fn servo_fetch() -> Command {
        Command::cargo_bin("servo-fetch").expect("binary exists")
    }

    #[test]
    fn conflicting_format_and_screenshot() {
        servo_fetch()
            .args(["--format", "json", "--screenshot", "out.png", "https://example.com"])
            .assert()
            .failure()
            .stderr(predicate::str::contains("cannot be used with"));
    }

    #[test]
    fn settle_rejects_out_of_range() {
        servo_fetch()
            .args(["--settle", "10001", "https://example.com"])
            .assert()
            .failure()
            .stderr(predicate::str::contains("invalid value"));
    }

    #[test]
    fn invalid_format_rejected() {
        servo_fetch()
            .args(["--format", "xml", "https://example.com"])
            .assert()
            .failure()
            .stderr(predicate::str::contains("invalid value"));
    }

    #[test]
    fn full_page_requires_screenshot() {
        servo_fetch()
            .args(["--full-page", "https://example.com"])
            .assert()
            .failure()
            .stderr(predicate::str::contains("--screenshot"));
    }

    #[test]
    fn schema_conflicts_with_selector() {
        servo_fetch()
            .args(["--schema", "s.json", "--selector", "div", "https://example.com"])
            .assert()
            .failure()
            .stderr(predicate::str::contains("cannot be used with"));
    }

    #[test]
    fn schema_conflicts_with_format() {
        servo_fetch()
            .args(["--schema", "s.json", "--format", "json", "https://example.com"])
            .assert()
            .failure()
            .stderr(predicate::str::contains("cannot be used with"));
    }

    #[test]
    fn format_html_with_selector_errors() {
        servo_fetch()
            .args(["--format", "html", "--selector", "article", "https://example.com"])
            .assert()
            .failure()
            .stderr(predicate::str::contains(
                "--selector cannot be used with --format html or text",
            ));
    }

    #[test]
    fn format_html_with_multi_urls_errors() {
        servo_fetch()
            .args(["--format", "html", "https://example.com", "https://example.org"])
            .assert()
            .failure()
            .stderr(predicate::str::contains("cannot be used with multiple URLs"));
    }

    #[test]
    fn output_dir_conflicts_with_screenshot() {
        servo_fetch()
            .args(["--output-dir", "out", "--screenshot", "out.png", "https://example.com"])
            .assert()
            .failure()
            .stderr(predicate::str::contains("cannot be used with"));
    }

    #[test]
    fn output_conflicts_with_screenshot() {
        servo_fetch()
            .args(["-o", "out.md", "--screenshot", "out.png", "https://example.com"])
            .assert()
            .failure()
            .stderr(predicate::str::contains("cannot be used with"));
    }

    #[test]
    fn output_conflicts_with_output_dir() {
        servo_fetch()
            .args(["-o", "out.md", "--output-dir", "out", "https://example.com"])
            .assert()
            .failure()
            .stderr(predicate::str::contains("cannot be used with"));
    }

    #[test]
    fn output_with_multi_urls_errors() {
        servo_fetch()
            .args(["-o", "out.md", "https://example.com", "https://example.org"])
            .assert()
            .failure()
            .stderr(predicate::str::contains("only valid with a single URL"));
    }

    #[test]
    fn output_with_js_is_allowed() {
        servo_fetch()
            .args(["--output", "out.txt", "--js", "document.title", "https://example.com"])
            .assert()
            .stderr(predicate::str::contains("cannot be used with").not())
            .stderr(predicate::str::contains("only valid with").not());
    }
}
