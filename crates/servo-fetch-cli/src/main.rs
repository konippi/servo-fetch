//! servo-fetch CLI.

#![deny(unsafe_code)]

mod cli;
mod commands;
mod exit;
mod logging;
mod mcp;
mod output;
mod progress;

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
    let policy = if args.allow_private_addresses || std::env::var_os("SERVO_FETCH_ALLOW_PRIVATE").is_some() {
        tracing::warn!("SSRF protection disabled: private/loopback addresses are reachable");
        servo_fetch::NetworkPolicy::PERMISSIVE
    } else {
        servo_fetch::NetworkPolicy::STRICT
    };
    servo_fetch::init(policy);
    match &args.command {
        Some(Command::Mcp(mcp)) => commands::mcp::run(mcp),
        Some(Command::Crawl(crawl)) => commands::crawl::run(crawl),
        Some(Command::Map(map)) => commands::map::run(map),
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
