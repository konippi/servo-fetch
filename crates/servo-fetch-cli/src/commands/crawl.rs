//! Crawl subcommand — BFS website crawler.

use std::io::{self, Write as _};

use crate::cli::CrawlArgs;
use crate::progress::Progress;

/// Crawl a site starting from `args.url` and stream results to stdout.
pub(crate) fn run(args: &CrawlArgs) -> anyhow::Result<()> {
    let opts = build_crawl_options(args);

    let progress = Progress::new();
    let mut completed = 0usize;
    let json = args.json;
    let mut write_err: Option<io::Error> = None;

    servo_fetch::crawl_each(opts, |result| {
        completed += 1;
        if write_err.is_some() {
            return;
        }
        let res = if json { emit_json(result) } else { emit_markdown(result) };
        if let Err(e) = res {
            write_err = Some(e);
            return;
        }
        let ok = result.outcome.is_ok();
        progress.item_done(completed, None, &result.url, ok);
    })?;

    if let Some(e) = write_err {
        return Err(e.into());
    }
    Ok(())
}

fn build_crawl_options(args: &CrawlArgs) -> servo_fetch::CrawlOptions {
    let mut opts = servo_fetch::CrawlOptions::new(&args.url)
        .limit(args.limit)
        .max_depth(args.max_depth)
        .timeout(std::time::Duration::from_secs(args.timeout))
        .settle(std::time::Duration::from_millis(args.settle))
        .concurrency(usize::try_from(args.concurrency).unwrap_or(usize::MAX))
        .delay(if args.delay_ms == 0 {
            None
        } else {
            Some(std::time::Duration::from_millis(args.delay_ms))
        })
        .json(args.json);
    if !args.include.is_empty() {
        opts = opts.include(&args.include.iter().map(String::as_str).collect::<Vec<_>>());
    }
    if !args.exclude.is_empty() {
        opts = opts.exclude(&args.exclude.iter().map(String::as_str).collect::<Vec<_>>());
    }
    if let Some(ref s) = args.selector {
        opts = opts.selector(s);
    }
    if let Some(ref ua) = args.user_agent {
        opts = opts.user_agent(ua);
    }
    opts
}

fn emit_json(result: &servo_fetch::CrawlResult) -> io::Result<()> {
    let line = serde_json::to_string(result).expect("CrawlResult is always serializable");
    writeln!(io::stdout(), "{}", servo_fetch::sanitize::sanitize(&line))
}

fn emit_markdown(result: &servo_fetch::CrawlResult) -> io::Result<()> {
    let mut out = io::stdout();
    writeln!(out, "--- {} ---", result.url)?;
    match &result.outcome {
        Ok(page) => {
            writeln!(out, "{}", servo_fetch::sanitize::sanitize(&page.content))?;
        }
        Err(e) => {
            tracing::warn!(url = %result.url, "{e}");
        }
    }
    writeln!(out)
}
