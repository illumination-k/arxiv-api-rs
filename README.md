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

### Requests to arXiv

All sessions share one client: requests start at least 3 seconds apart (arXiv's guideline), time out after 30 seconds, and are retried with backoff on network errors, `429`, and `5xx` (honoring `Retry-After`). Set a `User-Agent` with contact info via `--user-agent` or `ARXIV_MCP_USER_AGENT`:

```bash
arxiv-mcp --user-agent "my-app/1.0 (mailto:me@example.com)"
```

Logs are written to stderr; set `RUST_LOG=debug` for verbose output.
