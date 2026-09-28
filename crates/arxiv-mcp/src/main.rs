mod server;

use std::net::SocketAddr;
use std::sync::Arc;

use clap::Parser;
use rmcp::transport::streamable_http_server::{
    session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
};
use rmcp::{transport::stdio, ServiceExt};
use tracing::info;
use tracing_subscriber::EnvFilter;

/// MCP server for searching and reading arXiv papers.
#[derive(Debug, Parser)]
#[command(version, about)]
struct Cli {
    /// Serve over Streamable HTTP on this address (e.g. `127.0.0.1:8080`) instead of stdio.
    #[arg(long, value_name = "ADDR")]
    http: Option<SocketAddr>,

    /// Allowed `Host` header value (`host` or `host:port`). Repeatable.
    /// Defaults to loopback only (`localhost`, `127.0.0.1`, `::1`).
    #[arg(long = "allowed-host", value_name = "HOST", requires = "http")]
    allowed_hosts: Vec<String>,

    /// Allowed `Origin` header value (e.g. `https://app.example.com:443`, `http://localhost:*`).
    /// Repeatable. When omitted, Origin validation is disabled.
    #[arg(long = "allowed-origin", value_name = "ORIGIN", requires = "http")]
    allowed_origins: Vec<String>,
}

const MCP_PATH: &str = "/mcp";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // stdout is reserved for the MCP protocol in stdio mode, so logs go to stderr.
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    let cli = Cli::parse();
    match cli.http {
        Some(addr) => serve_http(addr, http_config(&cli)).await,
        None => serve_stdio().await,
    }
}

async fn serve_stdio() -> anyhow::Result<()> {
    let service = server::ArxivServer::default().serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}

fn http_config(cli: &Cli) -> StreamableHttpServerConfig {
    let mut config = StreamableHttpServerConfig::default();
    if !cli.allowed_hosts.is_empty() {
        config = config.with_allowed_hosts(cli.allowed_hosts.iter().cloned());
    }
    if !cli.allowed_origins.is_empty() {
        config = config.with_allowed_origins(cli.allowed_origins.iter().cloned());
    }
    config
}

fn router(config: StreamableHttpServerConfig) -> axum::Router {
    let service = StreamableHttpService::new(
        || Ok(server::ArxivServer::default()),
        Arc::new(LocalSessionManager::default()),
        config,
    );
    axum::Router::new().nest_service(MCP_PATH, service)
}

async fn serve_http(addr: SocketAddr, config: StreamableHttpServerConfig) -> anyhow::Result<()> {
    let ct = config.cancellation_token.clone();
    info!(
        allowed_hosts = ?config.allowed_hosts,
        allowed_origins = ?config.allowed_origins,
        "Listening on http://{addr}{MCP_PATH}"
    );

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router(config))
        .with_graceful_shutdown(async move {
            let _ = tokio::signal::ctrl_c().await;
            ct.cancel();
        })
        .await?;
    Ok(())
}

#[cfg(test)]
mod test {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    use super::*;

    fn initialize_request(host: &str, origin: Option<&str>) -> Request<Body> {
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": { "name": "test", "version": "0" }
            }
        });
        let mut builder = Request::post(MCP_PATH)
            .header("host", host)
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream");
        if let Some(origin) = origin {
            builder = builder.header("origin", origin);
        }
        builder.body(Body::from(body.to_string())).unwrap()
    }

    async fn status(config: StreamableHttpServerConfig, req: Request<Body>) -> StatusCode {
        router(config).oneshot(req).await.unwrap().status()
    }

    fn cli(args: &[&str]) -> Cli {
        Cli::try_parse_from(std::iter::once("arxiv-mcp").chain(args.iter().copied())).unwrap()
    }

    #[test]
    fn test_cli_requires_http_for_allowlists() {
        assert!(Cli::try_parse_from(["arxiv-mcp", "--allowed-host", "example.com"]).is_err());
        let c = cli(&[
            "--http",
            "127.0.0.1:8080",
            "--allowed-host",
            "a.example.com",
            "--allowed-host",
            "b.example.com:8080",
        ]);
        assert_eq!(c.allowed_hosts, ["a.example.com", "b.example.com:8080"]);
    }

    #[tokio::test]
    async fn test_default_allows_only_loopback_hosts() {
        let c = cli(&["--http", "127.0.0.1:8080"]);
        assert!(
            status(http_config(&c), initialize_request("localhost:8080", None))
                .await
                .is_success()
        );
        assert_eq!(
            status(
                http_config(&c),
                initialize_request("evil.example.com", None)
            )
            .await,
            StatusCode::FORBIDDEN
        );
    }

    #[tokio::test]
    async fn test_custom_allowed_hosts() {
        let c = cli(&[
            "--http",
            "0.0.0.0:8080",
            "--allowed-host",
            "mcp.example.com",
        ]);
        assert!(
            status(http_config(&c), initialize_request("mcp.example.com", None))
                .await
                .is_success()
        );
        assert_eq!(
            status(http_config(&c), initialize_request("localhost", None)).await,
            StatusCode::FORBIDDEN
        );
    }

    #[tokio::test]
    async fn test_allowed_origins() {
        let c = cli(&[
            "--http",
            "127.0.0.1:8080",
            "--allowed-origin",
            "https://app.example.com:443",
        ]);
        assert!(status(
            http_config(&c),
            initialize_request("localhost", Some("https://app.example.com"))
        )
        .await
        .is_success());
        assert_eq!(
            status(
                http_config(&c),
                initialize_request("localhost", Some("https://evil.example.com"))
            )
            .await,
            StatusCode::FORBIDDEN
        );
    }
}
