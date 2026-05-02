//! MCP server subcommand.

use crate::cli::McpArgs;

/// Start the MCP server (stdio or HTTP transport).
pub(crate) fn run(args: &McpArgs) -> anyhow::Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(crate::mcp::run(args.port))
}
