# arxiv-api-rs

Rust workspace for the arXiv.org API.

| Crate                                 | Description                                       |
| ------------------------------------- | ------------------------------------------------- |
| [`arxiv-api-rs`](crates/arxiv-api-rs) | Async client library for the arXiv API            |
| [`arxiv-mcp`](crates/arxiv-mcp)       | MCP server (stdio) exposing arXiv search as tools |

## MCP server

### Tools

| Tool                | Description                                                             |
| ------------------- | ----------------------------------------------------------------------- |
| `search_papers`     | Search arXiv (`query`, `max_results`, `start`, `sort_by`, `sort_order`) |
| `get_papers`        | Fetch metadata for a list of arXiv IDs                                  |
| `get_paper_content` | Fetch a paper's HTML version and return it as Markdown                  |

### Install

```bash
cargo install --path crates/arxiv-mcp
```

### Configure (Claude Code)

```bash
claude mcp add arxiv -- arxiv-mcp
```

Or in an MCP client config:

```json
{
  "mcpServers": {
    "arxiv": { "command": "arxiv-mcp" }
  }
}
```

### HTTP mode (Streamable HTTP)

By default the server speaks MCP over stdio. Pass `--http` to serve Streamable HTTP at `/mcp` instead:

```bash
# Loopback only (default allowed hosts: localhost, 127.0.0.1, ::1)
arxiv-mcp --http 127.0.0.1:8080

# Public deployment: restrict accepted Host / Origin headers
arxiv-mcp --http 0.0.0.0:8080 \
  --allowed-host mcp.example.com \
  --allowed-origin https://app.example.com:443
```

- `--allowed-host` (repeatable): allowed `Host` header values (`host` or `host:port`). Requests with other hosts get `403` (DNS rebinding protection).
- `--allowed-origin` (repeatable): allowed `Origin` header values; `:*` matches any port. Origin validation is disabled when omitted.

Logs are written to stderr; set `RUST_LOG=debug` for verbose output.
