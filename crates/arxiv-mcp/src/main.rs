mod server;

use rmcp::{transport::stdio, ServiceExt};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // stdout is reserved for the MCP protocol, so logs go to stderr.
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    let service = server::ArxivServer::default().serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
