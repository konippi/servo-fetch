//! MCP server — exposes Servo's web rendering capabilities to AI agents.

mod executor;
mod output;
mod server;
mod tools;

use std::net::SocketAddr;

use rmcp::ServiceExt as _;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};

/// Start the MCP server on stdio or Streamable HTTP transport.
pub(crate) async fn run(port: Option<u16>) -> anyhow::Result<()> {
    let broker = servo_fetch::SessionBrokerConfig::default()
        .queue_capacity(servo_fetch::SessionBrokerConfig::MAX_QUEUE_CAPACITY);
    let session_capacity = broker.session_capacity();
    servo_fetch::configure_default_broker(broker)?;
    tokio::task::spawn_blocking(servo_fetch::initialize_default_broker).await??;
    if let Some(port) = port {
        run_http(port, session_capacity).await
    } else {
        run_stdio(session_capacity).await
    }
}

async fn run_stdio(session_capacity: usize) -> anyhow::Result<()> {
    let service = server::ServoFetchMcp::new(session_capacity)
        .serve(rmcp::transport::stdio())
        .await
        .map_err(|e| anyhow::anyhow!("MCP server failed to start: {e}"))?;
    service
        .waiting()
        .await
        .map_err(|e| anyhow::anyhow!("MCP server error: {e}"))?;
    Ok(())
}

async fn run_http(port: u16, session_capacity: usize) -> anyhow::Result<()> {
    let service = StreamableHttpService::new(
        move || Ok(server::ServoFetchMcp::new(session_capacity)),
        LocalSessionManager::default().into(),
        StreamableHttpServerConfig::default(),
    );

    let router = axum::Router::new().nest_service("/mcp", service);
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "MCP server listening");

    axum::serve(listener, router)
        .with_graceful_shutdown(async {
            tokio::signal::ctrl_c().await.ok();
        })
        .await?;
    Ok(())
}
