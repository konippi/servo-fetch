//! Crawl subcommand — BFS website crawler.

use std::fs;
use std::io::{self, Write as _};
use std::time::{Duration, Instant};

use crate::cli::{CrawlArgs, CrawlFormat};
use crate::output::{Ext, Sink};
use crate::progress::Progress;

/// Crawl a site starting from `args.url` and stream results to stdout or a directory.
pub(crate) fn run(args: &CrawlArgs) -> anyhow::Result<()> {
    if let Some(dir) = args.output_dir.as_deref() {
        fs::create_dir_all(dir)?;
    }
    let json = matches!(args.format, CrawlFormat::Json);
    let sink = Sink::from_dir(args.output_dir.as_deref());
    let mut opts = build_crawl_options(args, json);
    if let Some(path) = &args.cookies {
        opts = opts.cookies(servo_fetch::load_cookies(path)?);
    }
    opts = opts.headers(servo_fetch::headers::parse_lines(&args.headers)?);

    let progress = Progress::new();
    let mut completed = 0u64;
    let mut errors = 0u64;
    let mut write_err: Option<anyhow::Error> = None;
    let started = Instant::now();

    servo_fetch::blocking::crawl_each(&opts, |result| {
        completed += 1;
        let ok = result.outcome.is_ok();
        if !ok {
            errors += 1;
        }
        if write_err.is_some() {
            return;
        }
        let url = result.url.clone();
        let res = if json {
            emit_json(&url, result, sink)
        } else {
            emit_markdown(&result, sink)
        };
        if let Err(e) = res {
            write_err = Some(e);
            return;
        }
        progress.item_done(usize::try_from(completed).unwrap_or(usize::MAX), None, &url, ok);
    })?;

    if let Some(e) = write_err {
        return Err(e);
    }
    if json {
        let elapsed = started.elapsed();
        if sink.is_stdout() {
            emit_stats(&mut io::stdout(), completed, errors, elapsed)?;
        } else {
            emit_stats(&mut io::stderr(), completed, errors, elapsed)?;
        }
    }
    Ok(())
}

fn build_crawl_options(args: &CrawlArgs, json: bool) -> servo_fetch::CrawlOptions {
    let mut opts = servo_fetch::CrawlOptions::new(&args.url)
        .limit(args.limit)
        .max_depth(args.max_depth)
        .timeout(Duration::from_secs(args.timeout))
        .settle(Duration::from_millis(args.settle))
        .concurrency(usize::try_from(args.concurrency).unwrap_or(usize::MAX))
        .delay(if args.delay_ms == 0 {
            None
        } else {
            Some(Duration::from_millis(args.delay_ms))
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

fn emit_json(url: &str, result: servo_fetch::CrawlResult, sink: Sink<'_>) -> anyhow::Result<()> {
    let line = serde_json::to_string(&crate::wire::crawl_event(result)).expect("CrawlEvent is always serializable");
    sink.writeln(url, Ext::Json, &line)
}

fn emit_stats(out: &mut impl io::Write, crawled: u64, errors: u64, elapsed: Duration) -> io::Result<()> {
    let elapsed_ms = u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX);
    let event = crate::wire::crawl_stats(crawled, errors, elapsed_ms);
    let line = serde_json::to_string(&event).expect("CrawlEvent is always serializable");
    writeln!(out, "{line}")
}

fn emit_markdown(result: &servo_fetch::CrawlResult, sink: Sink<'_>) -> anyhow::Result<()> {
    let page = match &result.outcome {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(url = %result.url, "{e}");
            return Ok(());
        }
    };
    if sink.is_stdout() {
        let mut out = io::stdout().lock();
        writeln!(out, "--- {} ---", result.url)?;
        out.write_all(servo_fetch::sanitize::sanitize(&page.content).as_bytes())?;
        writeln!(out)?;
        Ok(())
    } else {
        sink.write(&result.url, Ext::Markdown, &page.content)
    }
}
