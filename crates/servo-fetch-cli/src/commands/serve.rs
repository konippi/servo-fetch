//! HTTP API server subcommand.

use crate::cli::ServeArgs;

pub(crate) fn run(args: &ServeArgs) -> anyhow::Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(crate::serve::run(&args.host, args.port))
}
