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

Logs are written to stderr; set `RUST_LOG=debug` for verbose output.
