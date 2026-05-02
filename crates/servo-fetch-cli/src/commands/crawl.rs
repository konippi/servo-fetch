//! Crawl subcommand — BFS website crawler.

use std::io::Write as _;

use crate::cli::CrawlArgs;
use crate::progress::Progress;

/// Crawl a site starting from `args.url` and stream NDJSON results to stdout.
pub(crate) fn run(args: &CrawlArgs) -> anyhow::Result<()> {
    let opts = servo_fetch::CrawlOptions::new(&args.url)
        .limit(args.limit)
        .max_depth(args.max_depth)
        .timeout(std::time::Duration::from_secs(args.timeout))
        .settle(std::time::Duration::from_millis(args.settle));
    let opts = if args.include.is_empty() {
        opts
    } else {
        opts.include(&args.include.iter().map(String::as_str).collect::<Vec<_>>())
    };
    let opts = if args.exclude.is_empty() {
        opts
    } else {
        opts.exclude(&args.exclude.iter().map(String::as_str).collect::<Vec<_>>())
    };

    let progress = Progress::new();
    let mut completed = 0usize;

    servo_fetch::crawl_each(opts, |result| {
        completed += 1;
        let line = serde_json::to_string(result).expect("CrawlResult is always serializable");
        let _ = writeln!(std::io::stdout(), "{}", servo_fetch::sanitize::sanitize(&line));
        let ok = result.status == servo_fetch::CrawlStatus::Ok;
        progress.item_done(completed, None, &result.url, ok);
    })?;

    Ok(())
}
