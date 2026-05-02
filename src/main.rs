//! servo-fetch — A browser engine in a binary.

#![deny(unsafe_code)]

mod bridge;
mod cli;
mod commands;
mod crawl;
mod exit;
mod logging;
mod mcp;
mod net;
mod output;
mod pdf;
mod progress;
mod runtime;
mod screenshot;

use clap::Parser;

use crate::cli::{Cli, Command};

fn main() -> ! {
    install_process_defaults();

    let args = Cli::parse();
    logging::init(logging::Verbosity::from_flags(args.verbose, args.quiet));

    let code = exit::exit_code(dispatch(&args));
    exit::flush_and_exit(code);
}

fn dispatch(args: &Cli) -> anyhow::Result<()> {
    match &args.command {
        Some(Command::Mcp(mcp)) => commands::mcp::run(mcp),
        Some(Command::Crawl(crawl)) => commands::crawl::run(crawl),
        None => commands::fetch::run(&args.fetch),
    }
}

fn install_process_defaults() {
    #[cfg(unix)]
    #[allow(unsafe_code)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("failed to install rustls crypto provider");
}
