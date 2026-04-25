//! CLI argument parsing.

use clap::Parser;

#[derive(Parser)]
#[command(
    name = "servo-fetch",
    version,
    about = "Fetch web pages with JS execution, no Chromium required.",
    after_help = "\
Examples:
  servo-fetch https://example.com              Readable Markdown (default)
  servo-fetch https://example.com --json       Structured JSON
  servo-fetch https://example.com --screenshot page.png
  servo-fetch https://example.com --js \"document.title\"
  servo-fetch https://example.com -t 60        Custom timeout (seconds)
  servo-fetch https://example.com --selector article  Extract specific section
  servo-fetch mcp                              Start MCP server (stdio)"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// URL to fetch
    pub url: Option<String>,

    /// Output as structured JSON
    #[arg(long, conflicts_with_all = ["screenshot", "js"])]
    pub json: bool,

    /// Save screenshot as PNG
    #[arg(long, value_name = "FILE", conflicts_with_all = ["json", "js"])]
    pub screenshot: Option<String>,

    /// Execute JavaScript and print the result
    #[arg(long, value_name = "EXPR", conflicts_with_all = ["json", "screenshot"])]
    pub js: Option<String>,

    /// Timeout in seconds for page load
    #[arg(short = 't', long, default_value_t = 30, value_name = "SECS")]
    pub timeout: u64,

    /// CSS selector to extract a specific section
    #[arg(long, value_name = "SELECTOR")]
    pub selector: Option<String>,

    /// Output raw HTML or plain text instead of Readability extraction (html or text)
    #[arg(long, value_name = "MODE", conflicts_with_all = ["json", "screenshot", "js", "selector"])]
    pub raw: Option<String>,
}

/// Available subcommands.
#[derive(clap::Subcommand)]
pub enum Command {
    /// Start MCP server (stdio transport by default, or HTTP with --port)
    Mcp {
        /// Port for Streamable HTTP transport. Omit for stdio.
        #[arg(long, value_name = "PORT")]
        port: Option<u16>,
    },
}

/// Validate and sanitize a URL for fetching.
///
/// Delegates to [`crate::net::validate_url`].
pub fn validate_url(input: &str) -> anyhow::Result<url::Url> {
    crate::net::validate_url(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_https() {
        assert!(validate_url("https://example.com").is_ok());
    }

    #[test]
    fn accepts_http() {
        assert!(validate_url("http://example.com").is_ok());
    }

    #[test]
    fn rejects_file_scheme() {
        let err = validate_url("file:///etc/passwd").unwrap_err();
        assert!(err.to_string().contains("not allowed"));
    }

    #[test]
    fn rejects_javascript_scheme() {
        let err = validate_url("javascript:alert(1)").unwrap_err();
        assert!(err.to_string().contains("not allowed"));
    }

    #[test]
    fn strips_credentials() {
        let url = validate_url("https://user:pass@example.com").unwrap();
        assert!(url.username().is_empty());
        assert!(url.password().is_none());
    }

    #[test]
    fn rejects_invalid_url() {
        assert!(validate_url("not a url").is_err());
    }

    #[test]
    fn rejects_private_host_via_url() {
        assert!(validate_url("http://127.0.0.1/").is_err());
    }

    #[test]
    fn rejects_hex_ip() {
        // url::Url::parse normalizes 0x7f000001 → 127.0.0.1
        assert!(validate_url("http://0x7f000001/").is_err());
    }

    #[test]
    fn rejects_decimal_ip() {
        assert!(validate_url("http://2130706433/").is_err());
    }

    #[test]
    fn rejects_data_scheme() {
        assert!(
            validate_url("data:text/html,<h1>hi</h1>")
                .unwrap_err()
                .to_string()
                .contains("not allowed")
        );
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
}
