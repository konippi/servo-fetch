//! MCP server subcommand.

use crate::cli::McpArgs;
use crate::{mcp, runtime};

/// Start the MCP server (stdio or HTTP transport).
pub(crate) fn run(args: &McpArgs) -> anyhow::Result<()> {
    runtime::block_on(mcp::run(args.port))?
}
