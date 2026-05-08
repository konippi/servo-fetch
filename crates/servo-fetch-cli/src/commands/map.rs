//! Map subcommand — URL discovery via sitemaps.

use std::io::{self, Write as _};

use crate::cli::MapArgs;

/// Discover URLs on a site and stream them to stdout.
pub(crate) fn run(args: &MapArgs) -> anyhow::Result<()> {
    let mut opts = servo_fetch::MapOptions::new(&args.url)
        .limit(args.limit)
        .timeout(args.timeout)
        .no_fallback(args.no_fallback);
    if !args.include.is_empty() {
        opts = opts.include(&args.include.iter().map(String::as_str).collect::<Vec<_>>());
    }
    if !args.exclude.is_empty() {
        opts = opts.exclude(&args.exclude.iter().map(String::as_str).collect::<Vec<_>>());
    }
    if let Some(ref ua) = args.user_agent {
        opts = opts.user_agent(ua);
    }

    let results = servo_fetch::map(opts)?;

    let mut out = io::stdout().lock();
    if args.json {
        let json = serde_json::to_string_pretty(&results)?;
        writeln!(out, "{json}")?;
    } else {
        for entry in &results {
            writeln!(out, "{}", entry.url)?;
        }
    }
    Ok(())
}
