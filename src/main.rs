//! servo-fetch — A browser engine in a binary.

#![deny(unsafe_code)]

mod bridge;
mod cli;
mod command;
mod mcp;
mod net;

use std::io::{IsTerminal, Write as _};

use clap::Parser;

fn main() {
    #[cfg(unix)]
    #[allow(unsafe_code)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }

    let args = cli::Cli::parse();

    // All paths use process::exit to avoid C++ static destructor races with SpiderMonkey.
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
    std::process::exit(code);
}

fn run(args: &cli::Cli) -> anyhow::Result<()> {
    let url_str = args
        .url
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("URL is required. Run with --help for usage."))?;
    let url = cli::validate_url(url_str)?;
    let need_screenshot = args.screenshot.is_some();

    let is_tty = std::io::stderr().is_terminal();
    if is_tty {
        eprint!("Fetching {url}...");
        let _ = std::io::Write::flush(&mut std::io::stderr());
    }

    let page = bridge::fetch_page(&bridge::FetchOptions {
        url: url.as_str(),
        timeout_secs: args.timeout,
        screenshot: need_screenshot,
        accessibility_tree: false,
        js: args.js.as_deref(),
    });

    if is_tty {
        eprint!("\r\x1b[K");
    }

    let page = page?;

    // PDF detected by Servo's load_web_resource callback (Content-Type based)
    if let Some(ref pdf_bytes) = page.pdf_data {
        let text = servo_fetch::extract::extract_pdf(pdf_bytes);
        write!(std::io::stdout(), "{}", servo_fetch::sanitize::sanitize(&text))?;
        return Ok(());
    }

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
