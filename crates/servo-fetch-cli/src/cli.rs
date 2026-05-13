//! CLI argument parsing.

use clap::Parser;

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
    #[arg(short = 'v', long, action = clap::ArgAction::Count, global = true, conflicts_with = "quiet")]
    pub verbose: u8,

    /// Suppress all logs except errors
    #[arg(short = 'q', long, global = true)]
    pub quiet: bool,

    /// Allow requests to loopback/private addresses (for testing with local servers).
    #[arg(long = "allow-private-addresses", hide = true, global = true)]
    pub allow_private_addresses: bool,
}

#[derive(clap::Args, Debug)]
pub(crate) struct FetchArgs {
    /// URLs to fetch (one or more)
    #[arg(num_args = 1..)]
    pub urls: Vec<String>,

    /// Output as structured JSON (NDJSON when multiple URLs)
    #[arg(long, conflicts_with_all = ["screenshot", "js"])]
    pub json: bool,

    /// Save screenshot as PNG (single URL only)
    #[arg(long, value_name = "FILE", conflicts_with_all = ["json", "js"])]
    pub screenshot: Option<String>,

    /// Capture the full scrollable page instead of just the viewport.
    #[arg(long, requires = "screenshot")]
    pub full_page: bool,

    /// Execute JavaScript and print the result (single URL only)
    #[arg(long, value_name = "EXPR", conflicts_with_all = ["json", "screenshot"])]
    pub js: Option<String>,

    /// Timeout in seconds for page load
    #[arg(short = 't', long, default_value_t = 30, value_parser = clap::value_parser!(u64).range(1..), value_name = "SECS")]
    pub timeout: u64,

    /// Extra wait in ms after the `load` event, for SPAs that keep hydrating.
    #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(u64).range(0..=10_000), value_name = "MS")]
    pub settle: u64,

    /// CSS selector to extract a specific section
    #[arg(long, value_name = "CSS", value_parser = clap::builder::NonEmptyStringValueParser::new())]
    pub selector: Option<String>,

    /// Output raw HTML or plain text instead of Readability extraction
    #[arg(long, value_name = "MODE", value_enum, conflicts_with_all = ["json", "screenshot", "js", "selector"])]
    pub raw: Option<RawMode>,

    /// Override the User-Agent string
    #[arg(long, value_name = "UA")]
    pub user_agent: Option<String>,

    /// Path to a CSS-selector schema file for structured JSON extraction
    #[arg(long, value_name = "FILE", conflicts_with_all = ["screenshot", "js", "raw", "selector"])]
    pub schema: Option<std::path::PathBuf>,
}

/// Raw output mode.
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub(crate) enum RawMode {
    /// Raw HTML
    Html,
    /// Plain text (document.body.innerText)
    Text,
}

/// Available subcommands.
#[derive(clap::Subcommand)]
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

#[derive(clap::Args, Debug)]
pub(crate) struct McpArgs {
    /// Port for Streamable HTTP transport. Omit for stdio.
    #[arg(long, value_name = "PORT")]
    pub port: Option<u16>,
}

#[derive(clap::Args, Debug)]
pub(crate) struct ServeArgs {
    /// Host to bind on.
    #[arg(long, value_name = "HOST", default_value = "127.0.0.1")]
    pub host: String,

    /// Port to listen on.
    #[arg(long, value_name = "PORT", default_value_t = 3000)]
    pub port: u16,
}

#[derive(clap::Args, Debug)]
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

    /// Output as NDJSON
    #[arg(long)]
    pub json: bool,

    /// CSS selector to extract a specific section per page
    #[arg(long, value_name = "CSS", value_parser = clap::builder::NonEmptyStringValueParser::new())]
    pub selector: Option<String>,

    /// Timeout in seconds per page
    #[arg(short = 't', long, default_value_t = 30, value_parser = clap::value_parser!(u64).range(1..), value_name = "SECS")]
    pub timeout: u64,

    /// Extra wait in ms after load event per page
    #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(u64).range(0..=10_000), value_name = "MS")]
    pub settle: u64,

    /// Maximum parallel page fetches. Yields in completion order when greater than 1.
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u64).range(1..=64), value_name = "N")]
    pub concurrency: u64,

    /// Minimum dispatch interval in ms (0 to disable).
    #[arg(long, default_value_t = 500, value_parser = clap::value_parser!(u64).range(0..=60_000), value_name = "MS")]
    pub delay_ms: u64,

    /// Override the User-Agent string
    #[arg(long, value_name = "UA")]
    pub user_agent: Option<String>,
}

#[derive(clap::Args, Debug)]
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
    #[arg(short = 't', long, default_value_t = 30, value_parser = clap::value_parser!(u64).range(1..), value_name = "SECS")]
    pub timeout: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_mode_from_str() {
        use clap::ValueEnum;
        assert!(RawMode::from_str("html", true).is_ok());
        assert!(RawMode::from_str("text", true).is_ok());
        assert!(RawMode::from_str("xml", true).is_err());
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
    fn conflicting_json_and_screenshot() {
        servo_fetch()
            .args(["--json", "--screenshot", "out.png", "https://example.com"])
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
    fn raw_conflicts_with_json() {
        servo_fetch()
            .args(["--raw", "html", "--json", "https://example.com"])
            .assert()
            .failure()
            .stderr(predicate::str::contains("cannot be used with"));
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
}
