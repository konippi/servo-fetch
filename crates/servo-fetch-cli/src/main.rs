//! servo-fetch CLI.

#![deny(unsafe_code)]

mod cli;
mod commands;
mod exit;
mod logging;
mod mcp;
mod output;
mod progress;
mod rpc;
mod serve;
mod tools;
mod wire;

use clap::Parser;

use crate::cli::{Cli, Command};

fn main() -> ! {
    install_process_defaults();

    if is_internal_worker() {
        let code = exit::exit_code(servo_fetch::run_worker_stdio().map_err(anyhow::Error::from));
        exit::flush_and_exit(code);
    }
    if let Err(error) = configure_worker_command() {
        let result: anyhow::Result<()> = Err(error);
        exit::flush_and_exit(exit::exit_code(result));
    }

    let args = Cli::parse();
    logging::init(logging::Verbosity::from_flags(args.verbose, args.quiet));

    let code = exit::exit_code(dispatch(&args));
    exit::flush_and_exit(code);
}

fn is_internal_worker() -> bool {
    let mut args = std::env::args_os();
    let _ = args.next();
    args.next().is_some_and(|arg| arg == "__worker") && args.next().is_none()
}

fn configure_worker_command() -> anyhow::Result<()> {
    let program = std::env::var_os("SERVO_FETCH_WORKER")
        .map(std::path::PathBuf::from)
        .map_or_else(std::env::current_exe, Ok)?;
    servo_fetch::set_default_worker_command(servo_fetch::WorkerCommand::new(program).arg("__worker"))
        .map_err(anyhow::Error::from)
}

fn dispatch(args: &Cli) -> anyhow::Result<()> {
    if args.command.as_ref().is_none_or(Command::needs_servo_init) {
        let policy = if args.allow_private_addresses || std::env::var_os("SERVO_FETCH_ALLOW_PRIVATE").is_some() {
            tracing::warn!("SSRF protection disabled: private/loopback addresses are reachable");
            servo_fetch::NetworkPolicy::PERMISSIVE
        } else {
            servo_fetch::NetworkPolicy::STRICT
        };
        servo_fetch::init(policy);
    }
    match &args.command {
        Some(Command::Mcp(mcp)) => commands::mcp::run(mcp),
        Some(Command::Serve(serve)) => commands::serve::run(serve),
        Some(Command::Crawl(crawl)) => commands::crawl::run(crawl),
        Some(Command::Map(map)) => commands::map::run(map),
        Some(Command::Healthcheck(hc)) => commands::healthcheck::run(hc),
        Some(Command::Worker) => servo_fetch::run_worker_stdio().map_err(anyhow::Error::from),
        Some(Command::Rpc(_)) => commands::rpc::run(),
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
