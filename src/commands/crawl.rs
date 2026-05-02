//! Crawl subcommand — BFS website crawler.

use std::io::Write as _;

use crate::cli::{self, CrawlArgs};
use crate::progress::Progress;
use crate::{crawl, runtime};

/// Crawl a site starting from `args.url` and stream NDJSON results to stdout.
pub(crate) fn run(args: &CrawlArgs) -> anyhow::Result<()> {
    let opts = build_options(args)?;
    let progress = Progress::new();
    let mut completed = 0usize;

    runtime::block_on(crawl::run(opts, |result| {
        completed += 1;
        let line = serde_json::to_string(result).expect("CrawlPageResult is always serializable");
        let _ = writeln!(std::io::stdout(), "{}", servo_fetch::sanitize::sanitize(&line));
        let ok = matches!(result.status, crawl::CrawlStatus::Ok);
        progress.item_done(completed, None, &result.url, ok);
    }))?;

    Ok(())
}

fn build_options(args: &CrawlArgs) -> anyhow::Result<crawl::CrawlOptions> {
    Ok(crawl::CrawlOptions {
        seed: cli::validate_url(&args.url)?,
        limit: args.limit,
        max_depth: args.max_depth,
        timeout_secs: args.timeout,
        settle_ms: args.settle,
        include: (!args.include.is_empty())
            .then(|| crawl::build_globset(&args.include))
            .transpose()?,
        exclude: (!args.exclude.is_empty())
            .then(|| crawl::build_globset(&args.exclude))
            .transpose()?,
        selector: args.selector.clone(),
        json: args.json,
    })
}
