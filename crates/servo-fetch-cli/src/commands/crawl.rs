//! Crawl subcommand — BFS website crawler.

use std::io::{self, Write as _};
use std::time::Instant;

use serde::Serialize;

use crate::cli::{CrawlArgs, CrawlFormat};
use crate::progress::Progress;

#[derive(Serialize)]
struct StatsRecord {
    #[serde(rename = "type")]
    kind: &'static str,
    crawled: u64,
    errors: u64,
    elapsed_ms: u64,
}

/// Crawl a site starting from `args.url` and stream results to stdout.
pub(crate) fn run(args: &CrawlArgs) -> anyhow::Result<()> {
    let json = matches!(args.format, CrawlFormat::Json);
    let opts = build_crawl_options(args, json);

    let progress = Progress::new();
    let mut completed = 0u64;
    let mut errors = 0u64;
    let mut write_err: Option<io::Error> = None;
    let started = Instant::now();

    servo_fetch::crawl_each(opts, |result| {
        completed += 1;
        if result.outcome.is_err() {
            errors += 1;
        }
        if write_err.is_some() {
            return;
        }
        let res = if json { emit_json(result) } else { emit_markdown(result) };
        if let Err(e) = res {
            write_err = Some(e);
            return;
        }
        progress.item_done(
            usize::try_from(completed).unwrap_or(usize::MAX),
            None,
            &result.url,
            result.outcome.is_ok(),
        );
    })?;

    if let Some(e) = write_err {
        return Err(e.into());
    }
    if json {
        emit_stats(completed, errors, started.elapsed())?;
    }
    Ok(())
}

fn build_crawl_options(args: &CrawlArgs, json: bool) -> servo_fetch::CrawlOptions {
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
        .json(json);
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

fn emit_stats(crawled: u64, errors: u64, elapsed: std::time::Duration) -> io::Result<()> {
    let stats = StatsRecord {
        kind: "stats",
        crawled,
        errors,
        elapsed_ms: u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX),
    };
    let line = serde_json::to_string(&stats).expect("StatsRecord is always serializable");
    writeln!(io::stdout(), "{line}")
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
