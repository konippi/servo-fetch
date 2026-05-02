//! Default fetch command — single URL, batch, and PDF probe.

use std::io::Write as _;
use std::sync::Arc;

use anyhow::{Result, bail};
use tokio::sync::{Semaphore, mpsc};

use crate::bridge::{self, FetchMode, FetchOptions, ServoPage};
use crate::cli::{self, FetchArgs};
use crate::progress::Progress;
use crate::{output, pdf, runtime};

const MAX_CONCURRENT_FETCHES: usize = 4;

/// Fetch one or more URLs and write the rendered output to stdout.
pub(crate) fn run(args: &FetchArgs) -> Result<()> {
    match args.urls.as_slice() {
        [] => bail!("URL is required. Run with --help for usage."),
        [one] => run_single(args, one),
        many => {
            if args.screenshot.is_some() || args.js.is_some() || args.raw.is_some() {
                bail!("--screenshot, --js, and --raw are not supported with multiple URLs");
            }
            runtime::block_on(run_batch(args, many))?
        }
    }
}

fn run_single(args: &FetchArgs, url_str: &str) -> Result<()> {
    let url = cli::validate_url(url_str)?;
    let progress = Progress::new();
    progress.ticker(&format!("Fetching {url}..."));

    if is_content_mode(args)
        && let Some(bytes) = pdf::probe(url.as_str(), args.timeout)
    {
        progress.clear();
        let text = servo_fetch::extract::extract_pdf(&bytes);
        write!(std::io::stdout(), "{}", servo_fetch::sanitize::sanitize(&text))?;
        return Ok(());
    }

    let page = bridge::fetch_page(FetchOptions {
        url: url.as_str(),
        timeout_secs: args.timeout,
        settle_ms: args.settle,
        mode: fetch_mode(args),
    });
    progress.clear();
    let page = page?;
    dispatch_output(args, &page, url.as_str())
}

async fn run_batch(args: &FetchArgs, urls: &[String]) -> Result<()> {
    let urls: Vec<url::Url> = urls.iter().map(|s| cli::validate_url(s)).collect::<Result<_>>()?;
    let total = urls.len();
    let progress = Progress::new();
    progress.header(&format!("Fetching {total} URLs..."));

    let sem = Arc::new(Semaphore::new(MAX_CONCURRENT_FETCHES));
    let (tx, mut rx) = mpsc::channel::<(String, Result<ServoPage>)>(total);

    for url in &urls {
        let _permit = sem.clone().acquire_owned().await?;
        let tx = tx.clone();
        let url_str = url.to_string();
        let timeout = args.timeout;
        let settle = args.settle;
        tokio::task::spawn_blocking(move || {
            let result = bridge::fetch_page(FetchOptions {
                url: &url_str,
                timeout_secs: timeout,
                settle_ms: settle,
                mode: FetchMode::Content { include_a11y: false },
            });
            let _ = tx.blocking_send((url_str, result));
        });
    }
    drop(tx);

    let mut completed = 0usize;
    let mut failures = 0usize;
    while let Some((url, result)) = rx.recv().await {
        completed += 1;
        match result {
            Ok(page) => {
                batch_emit(args, &page, &url)?;
                progress.item_done(completed, Some(total), &url, true);
            }
            Err(err) => {
                failures += 1;
                tracing::error!(url = %url, "{err:#}");
            }
        }
    }

    if failures == total {
        bail!("all {total} URLs failed");
    }
    Ok(())
}

fn batch_emit(args: &FetchArgs, page: &ServoPage, url: &str) -> Result<()> {
    if args.json {
        output::Json {
            page,
            url,
            selector: args.selector.as_deref(),
        }
        .execute_compact()
    } else {
        writeln!(std::io::stdout(), "--- {url} ---")?;
        output::Markdown {
            page,
            url,
            selector: args.selector.as_deref(),
        }
        .execute()?;
        writeln!(std::io::stdout())?;
        Ok(())
    }
}

fn dispatch_output(args: &FetchArgs, page: &ServoPage, url: &str) -> Result<()> {
    if let Some(result) = page.js_result.as_deref() {
        return output::JsEval { result }.execute();
    }
    if let Some(path) = args.screenshot.as_deref() {
        return output::Screenshot { page, path }.execute();
    }
    if let Some(mode) = args.raw.as_ref() {
        return output::Raw { page, mode }.execute();
    }
    let selector = args.selector.as_deref();
    if args.json {
        output::Json { page, url, selector }.execute()
    } else {
        output::Markdown { page, url, selector }.execute()
    }
}

fn is_content_mode(args: &FetchArgs) -> bool {
    args.screenshot.is_none() && args.js.is_none()
}

fn fetch_mode(args: &FetchArgs) -> FetchMode {
    if args.screenshot.is_some() {
        FetchMode::Screenshot {
            full_page: args.full_page,
        }
    } else if let Some(expr) = args.js.clone() {
        FetchMode::ExecuteJs { expression: expr }
    } else {
        FetchMode::Content { include_a11y: false }
    }
}
