//! Default fetch command — single URL, batch, and PDF probe.

use std::io::Write as _;
use std::time::Duration;

use anyhow::{Result, bail};

use servo_fetch::{FetchOptions, Page};

use crate::cli::FetchArgs;
use crate::output;
use crate::progress::Progress;

/// Fetch one or more URLs and write the rendered output to stdout.
pub(crate) fn run(args: &FetchArgs) -> Result<()> {
    match args.urls.as_slice() {
        [] => bail!("URL is required. Run with --help for usage."),
        [one] => run_single(args, one),
        many => {
            if args.screenshot.is_some() || args.js.is_some() || args.raw.is_some() {
                bail!("--screenshot, --js, and --raw are not supported with multiple URLs");
            }
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(run_batch(args, many))
        }
    }
}

fn run_single(args: &FetchArgs, url_str: &str) -> Result<()> {
    let progress = Progress::new();
    progress.ticker(&format!("Fetching {url_str}..."));

    let opts = build_fetch_options(args, url_str)?;
    let page = servo_fetch::fetch(opts).map_err(anyhow::Error::from);
    progress.clear();
    let page = page?;
    dispatch_output(args, &page, url_str)
}

async fn run_batch(args: &FetchArgs, urls: &[String]) -> Result<()> {
    let total = urls.len();
    let progress = Progress::new();
    progress.header(&format!("Fetching {total} URLs..."));

    let schema = args.schema.as_ref().map(|p| load_schema(p)).transpose()?;

    let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(4));
    let (tx, mut rx) = tokio::sync::mpsc::channel::<(String, std::result::Result<Page, servo_fetch::Error>)>(total);

    for url in urls {
        let permit = sem.clone().acquire_owned().await?;
        let tx = tx.clone();
        let url_str = url.clone();
        let timeout = args.timeout;
        let settle = args.settle;
        let user_agent = args.user_agent.clone();
        let schema = schema.clone();
        tokio::task::spawn_blocking(move || {
            let mut opts = FetchOptions::new(&url_str)
                .timeout(Duration::from_secs(timeout))
                .settle(Duration::from_millis(settle));
            if let Some(ua) = user_agent {
                opts = opts.user_agent(ua);
            }
            if let Some(s) = schema {
                opts = opts.schema(s);
            }
            let result = servo_fetch::fetch(opts);
            let _ = tx.blocking_send((url_str, result));
            drop(permit);
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

fn batch_emit(args: &FetchArgs, page: &Page, url: &str) -> Result<()> {
    if args.schema.is_some() {
        return output::Extracted { page, url }.execute_compact();
    }
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

fn dispatch_output(args: &FetchArgs, page: &Page, url: &str) -> Result<()> {
    if let Some(result) = page.js_result.as_deref() {
        return output::JsEval { result }.execute();
    }
    if let Some(path) = args.screenshot.as_deref() {
        return output::Screenshot { page, path }.execute();
    }
    if let Some(mode) = args.raw.as_ref() {
        return output::Raw { page, mode }.execute();
    }
    if args.schema.is_some() {
        return output::Extracted { page, url }.execute();
    }
    let selector = args.selector.as_deref();
    if args.json {
        output::Json { page, url, selector }.execute()
    } else {
        output::Markdown { page, url, selector }.execute()
    }
}

fn build_fetch_options(args: &FetchArgs, url: &str) -> Result<FetchOptions> {
    let base = if args.screenshot.is_some() {
        FetchOptions::screenshot(url, args.full_page)
    } else if let Some(expr) = args.js.as_deref() {
        FetchOptions::javascript(url, expr)
    } else {
        FetchOptions::new(url)
    };
    let opts = base
        .timeout(Duration::from_secs(args.timeout))
        .settle(Duration::from_millis(args.settle));
    let opts = match args.user_agent {
        Some(ref ua) => opts.user_agent(ua),
        None => opts,
    };
    let opts = match args.schema {
        Some(ref path) => opts.schema(load_schema(path)?),
        None => opts,
    };
    Ok(opts)
}

fn load_schema(path: &std::path::Path) -> Result<servo_fetch::schema::ExtractSchema> {
    servo_fetch::schema::ExtractSchema::from_path(path).map_err(|e| anyhow::anyhow!("schema '{}': {e}", path.display()))
}
