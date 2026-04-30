//! servo-fetch — A browser engine in a binary.

#![deny(unsafe_code)]

mod bridge;
mod cli;
mod command;
mod mcp;
mod net;
mod pdf;
mod screenshot;

use std::io::{IsTerminal, Write as _};

use clap::Parser;

fn main() {
    #[cfg(unix)]
    #[allow(unsafe_code)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }

    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    let args = cli::Cli::parse();

    if let Some(cli::Command::Mcp { port }) = args.command {
        flush_and_exit(run_mcp(port));
    }

    let code = match run(&args) {
        Ok(()) => 0,
        Err(err) if is_broken_pipe(&err) => 0,
        Err(err) => {
            eprintln!("error: {err:#}");
            1
        }
    };
    flush_and_exit(code);
}

fn run_mcp(port: Option<u16>) -> i32 {
    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    match rt.block_on(mcp::run(port)) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("error: {e:#}");
            1
        }
    }
}

fn is_broken_pipe(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|e| e.kind() == std::io::ErrorKind::BrokenPipe)
    })
}

fn flush_and_exit(code: i32) -> ! {
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    // On Linux, SpiderMonkey's static destructors race on `pthread_mutex_destroy`
    // during process exit, producing a post-exit SIGSEGV.
    #[cfg(target_os = "linux")]
    #[allow(unsafe_code)]
    unsafe {
        libc::_exit(code);
    }
    #[cfg(not(target_os = "linux"))]
    std::process::exit(code);
}

fn run(args: &cli::Cli) -> anyhow::Result<()> {
    if args.urls.is_empty() {
        anyhow::bail!("URL is required. Run with --help for usage.");
    }

    if args.urls.len() > 1 {
        if args.screenshot.is_some() || args.js.is_some() || args.raw.is_some() {
            anyhow::bail!("--screenshot, --js, and --raw are not supported with multiple URLs");
        }
        let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
        return rt.block_on(run_batch(args));
    }

    run_single(args)
}

async fn run_batch(args: &cli::Cli) -> anyhow::Result<()> {
    use std::sync::Arc;
    use tokio::sync::Semaphore;

    let urls: Vec<url::Url> = args
        .urls
        .iter()
        .map(|s| cli::validate_url(s))
        .collect::<anyhow::Result<Vec<_>>>()?;

    let is_tty = std::io::stderr().is_terminal();
    let total = urls.len();
    if is_tty {
        eprintln!("Fetching {total} URLs...");
    }

    let sem = Arc::new(Semaphore::new(4));
    let (tx, mut rx) = tokio::sync::mpsc::channel::<(String, anyhow::Result<bridge::ServoPage>)>(total);

    for url in &urls {
        let permit = sem.clone().acquire_owned().await?;
        let tx = tx.clone();
        let url_str = url.to_string();
        let timeout = args.timeout;
        let settle = args.settle;
        tokio::task::spawn_blocking(move || {
            let result = bridge::fetch_page(bridge::FetchOptions {
                url: &url_str,
                timeout_secs: timeout,
                settle_ms: settle,
                mode: bridge::FetchMode::Content { include_a11y: false },
            });
            let _ = tx.blocking_send((url_str, result));
            drop(permit);
        });
    }
    drop(tx);

    let mut completed = 0usize;
    let mut failures = 0usize;
    let json = args.json;
    let selector = args.selector.as_deref();

    while let Some((url, result)) = rx.recv().await {
        completed += 1;
        match result {
            Ok(page) => {
                if json {
                    let input = servo_fetch::extract::ExtractInput::new(&page.html, &url)
                        .with_layout_json(page.layout_json.as_deref())
                        .with_inner_text(page.inner_text.as_deref())
                        .with_selector(selector);
                    let pretty = servo_fetch::extract::extract_json(&input).unwrap_or_default();
                    // NDJSON: compact single-line JSON per URL
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&pretty) {
                        let compact = serde_json::to_string(&val).unwrap_or(pretty);
                        writeln!(std::io::stdout(), "{}", servo_fetch::sanitize::sanitize(&compact))?;
                    } else {
                        writeln!(std::io::stdout(), "{}", servo_fetch::sanitize::sanitize(&pretty))?;
                    }
                } else {
                    writeln!(std::io::stdout(), "--- {url} ---")?;
                    let input = servo_fetch::extract::ExtractInput::new(&page.html, &url)
                        .with_layout_json(page.layout_json.as_deref())
                        .with_inner_text(page.inner_text.as_deref())
                        .with_selector(selector);
                    let out = servo_fetch::extract::extract_text(&input).unwrap_or_default();
                    write!(std::io::stdout(), "{}", servo_fetch::sanitize::sanitize(&out))?;
                    writeln!(std::io::stdout())?;
                }
                if is_tty {
                    eprintln!("[{completed}/{total}] {url} ✓");
                }
            }
            Err(e) => {
                failures += 1;
                eprintln!("error: {url}: {e:#}");
            }
        }
    }

    if failures == total {
        anyhow::bail!("all {total} URLs failed");
    }
    Ok(())
}

fn run_single(args: &cli::Cli) -> anyhow::Result<()> {
    let url_str = &args.urls[0];
    let url = cli::validate_url(url_str)?;

    let is_tty = std::io::stderr().is_terminal();
    if is_tty {
        eprint!("Fetching {url}...");
        let _ = std::io::Write::flush(&mut std::io::stderr());
    }

    let is_content_mode = args.screenshot.is_none() && args.js.is_none();

    // PDF probe skips modes where Servo rendering is the point.
    if is_content_mode {
        if let Some(pdf_bytes) = pdf::probe(url.as_str(), args.timeout) {
            if is_tty {
                eprint!("\r\x1b[K");
            }
            let text = servo_fetch::extract::extract_pdf(&pdf_bytes);
            write!(std::io::stdout(), "{}", servo_fetch::sanitize::sanitize(&text))?;
            return Ok(());
        }
    }

    let mode = if args.screenshot.is_some() {
        bridge::FetchMode::Screenshot {
            full_page: args.full_page,
        }
    } else if let Some(expr) = args.js.clone() {
        bridge::FetchMode::ExecuteJs { expression: expr }
    } else {
        bridge::FetchMode::Content { include_a11y: false }
    };
    let page = bridge::fetch_page(bridge::FetchOptions {
        url: url.as_str(),
        timeout_secs: args.timeout,
        settle_ms: args.settle,
        mode,
    });

    if is_tty {
        eprint!("\r\x1b[K");
    }

    let page = page?;

    if let Some(ref result) = page.js_result {
        command::JsEval { result }.execute()?;
        return Ok(());
    }

    if let Some(ref path) = args.screenshot {
        return command::Screenshot { page: &page, path }.execute();
    }

    if let Some(ref mode) = args.raw {
        return command::Raw { page: &page, mode }.execute();
    }

    if args.json {
        return command::Json {
            page: &page,
            url: url.as_str(),
            selector: args.selector.as_deref(),
        }
        .execute();
    }

    command::Markdown {
        page: &page,
        url: url.as_str(),
        selector: args.selector.as_deref(),
    }
    .execute()
}
