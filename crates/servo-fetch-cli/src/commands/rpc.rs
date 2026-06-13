//! Stdio JSON-RPC server subcommand (internal; spawned by language bindings).

pub(crate) fn run() -> anyhow::Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(crate::rpc::run())
}
