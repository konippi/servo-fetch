//! Crawl subcommand — BFS website crawler.

use std::fs;
use std::io::{self, Write as _};
use std::time::{Duration, Instant};

use crate::cli::{CrawlArgs, CrawlFormat};
use crate::output::{Ext, Sink};

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

    let counter = crate::progress::counter();
    let mut completed = 0usize;
    let mut failures = crate::exit::Failures::default();
    let mut write_err: Option<anyhow::Error> = None;
    let started = Instant::now();

    servo_fetch::blocking::crawl_each(&opts, |result| {
        completed += 1;
        if let Some(err) = result.outcome.as_ref().err() {
            failures.record(err);
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
        counter.set_message(url);
        counter.inc(1);
    })?;
    counter.finish_and_clear();

    let failed = failures.failed();
    let stats_error = if json {
        let elapsed = started.elapsed();
        let result = if sink.is_stdout() {
            emit_stats(&mut io::stdout(), completed, failed, elapsed)
        } else {
            emit_stats(&mut io::stderr(), completed, failed, elapsed)
        };
        result.err().map(crate::exit::output_error)
    } else {
        None
    };
    let output_error = write_err.or(stats_error);
    let failure_error = if completed != 0 && failed == completed {
        failures.into_error("pages", completed)
    } else {
        None
    };
    crate::exit::finalize_multi_item_result(output_error, failure_error)
}

fn build_crawl_options(args: &CrawlArgs, json: bool) -> servo_fetch::CrawlOptions {
    let mut opts = servo_fetch::CrawlOptions::new(&args.url)
        .limit(args.limit.get())
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

fn emit_stats(out: &mut impl io::Write, crawled: usize, errors: usize, elapsed: Duration) -> io::Result<()> {
    let crawled = u64::try_from(crawled).unwrap_or(u64::MAX);
    let errors = u64::try_from(errors).unwrap_or(u64::MAX);
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
        writeln!(out, "--- {} ---", result.url).map_err(crate::exit::output_error)?;
        out.write_all(servo_fetch::sanitize::sanitize(&page.content).as_bytes())
            .map_err(crate::exit::output_error)?;
        writeln!(out).map_err(crate::exit::output_error)?;
        Ok(())
    } else {
        sink.write(&result.url, Ext::Markdown, &page.content)
    }
}
