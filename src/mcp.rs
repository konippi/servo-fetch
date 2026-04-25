//! MCP server — exposes Servo's web rendering capabilities to AI agents.

mod server;
mod tools;

use rmcp::ServiceExt as _;
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};

/// Start the MCP server on stdio or Streamable HTTP transport.
pub async fn run(port: Option<u16>) -> anyhow::Result<()> {
    if let Some(port) = port {
        run_http(port).await
    } else {
        run_stdio().await
    }
}

async fn run_stdio() -> anyhow::Result<()> {
    let service = server::ServoMcp::new()
        .serve(rmcp::transport::stdio())
        .await
        .map_err(|e| anyhow::anyhow!("MCP server failed to start: {e}"))?;
    service
        .waiting()
        .await
        .map_err(|e| anyhow::anyhow!("MCP server error: {e}"))?;
    Ok(())
}

async fn run_http(port: u16) -> anyhow::Result<()> {
    let service = StreamableHttpService::new(
        || Ok(server::ServoMcp::new()),
        LocalSessionManager::default().into(),
        StreamableHttpServerConfig::default(),
    );

    let router = axum::Router::new().nest_service("/mcp", service);
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    eprintln!("MCP server listening on http://{addr}/mcp");

    axum::serve(listener, router)
        .with_graceful_shutdown(async {
            tokio::signal::ctrl_c().await.ok();
        })
        .await?;
    Ok(())
}
