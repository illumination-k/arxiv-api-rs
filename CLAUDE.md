# CLAUDE.md

## Project Overview

Cargo workspace for the arXiv.org API:

- `crates/arxiv-api-rs` — Rust library for querying the arXiv.org API. It provides an async HTTP client, a builder-pattern query API, a search DSL with boolean logic and date ranges, and typed data models for arXiv paper results.
- `crates/arxiv-mcp` — MCP server binary (stdio by default, or Streamable HTTP via `--http` with `--allowed-host`/`--allowed-origin` allowlists; built on `rmcp` + `axum`) that exposes the library as tools: `search_papers`, `get_papers`, `get_paper_content`.

## Repository Structure

```
Cargo.toml                # Workspace manifest
crates/arxiv-mcp/src/
├── main.rs               # CLI (clap), stdio / HTTP transports (logs to stderr)
└── server.rs             # ArxivServer: rmcp tool router and tool implementations
crates/arxiv-api-rs/
├── tests/fixtures/       # XML/HTML fixtures used by unit tests
└── src/
    ├── lib.rs            # Library entry point, re-exports, network integration tests
    ├── client.rs         # ArxivClient + ArxivClientBuilder (User-Agent, timeout, shared rate limiter, retries)
    ├── error.rs          # ArxivError enum (thiserror), Result type alias
    ├── html_parser.rs    # (feature "html") arXiv HTML (LaTeXML) parser: sections, figures, tables, equations, Markdown
    ├── models.rs         # Data models: Feed (XML root), Entry (internal), ArxivResult (public output)
    ├── query.rs          # ArxivQuery<S> builder, SortBy/SortOrder enums, URL construction
    └── search_query.rs   # Search DSL: SearchTerm, SearchRange, SearchPredicate, ISearchQuery trait
```

- `client.rs` — `ArxivClient` built via `ArxivClient::builder()` (defaults: descriptive User-Agent, 30s timeout, 3 attempts with exponential backoff, 3s minimum interval between requests shared by all clones). Retries network errors, 429, and 5xx (honors `Retry-After`). `search()` returns a `SearchResponse`; `fetch_html()` (feature `html`) returns `HtmlNotAvailable` on 404. Unit tests use a local mock TCP server and paused tokio time.
- `models.rs` — Serde/quick-xml deserialization of arXiv Atom XML. Internal `Entry`/`Feed` types are converted to the public `ArxivResult`.
- `query.rs` — Generic `ArxivQuery<S>` with builder methods (`with_search_query`, `with_id_list`, `with_max_results`, `with_start`, `with_sort_by`, `next_page_query`). Builds URL query parameters.
- `search_query.rs` — `ISearchQuery` trait, `SearchTerm` (field:value), `SearchRange` (date ranges), and `SearchPredicate` (AND/OR/ANDNOT/Bracket combinators with lifetime `'a`).

## Build and Development Commands

```bash
# Build the project
cargo build

# Run all tests across the workspace (includes integration tests that make real HTTP requests to arXiv)
cargo test --workspace --all-features

# Check formatting (CI uses this)
cargo fmt --check

# Apply formatting
cargo fmt

# Lint with clippy (CI treats warnings as errors)
cargo clippy --workspace --all-features --all-targets -- --deny warnings

# Check dprint formatting (JSON, Markdown, TOML, YAML)
dprint check

# Apply dprint formatting
dprint fmt
```

## CI Pipeline

Defined in `.github/workflows/ci.yaml`. Triggers on pushes to `main` and all PRs. Three jobs:

1. **actionlint** — Lints the CI workflow file itself.
2. **dprint** — Checks formatting of JSON, Markdown, TOML, and YAML files via `dprint check`.
3. **lint_and_test** — Runs `cargo fmt --check`, `cargo clippy --workspace --all-features --all-targets -- --deny warnings`, and `cargo test --workspace --all-features`.

All three jobs must pass. Clippy warnings are treated as errors (`--deny warnings`).

## Testing

Tests are inline (`#[cfg(test)]` modules) in each source file:

- `lib.rs` — 5 async integration tests (`#[tokio::test]`) that make real HTTP requests to the arXiv API. These may be slow or flaky due to network dependency.
- `client.rs` — offline unit tests for retry/status handling, User-Agent, and the rate limiter (mock HTTP server, `tokio::time::pause`).
- `query.rs` — 3 sync unit tests for query builder and URL parameter construction.
- `search_query.rs` — 4 sync unit tests for search term formatting, date range formatting, and predicate composition.
- `html_parser.rs` — sync unit tests (feature `html`) over HTML fixtures in `tests/fixtures/` (`sample_paper.html` and `html_*.html`: figures, tables, image grids, floats outside sections, text-only floats, inline content) modeled on real LaTeXML output.

Run all tests with `cargo test`. There is no separate integration test directory.

## Code Conventions

- **Error handling**: Uses `thiserror` with a custom `ArxivError` enum in `error.rs`. Public API returns `crate::Result<T>` (alias for `Result<T, ArxivError>`). Error variants include `RequestFailed`, `HttpStatus`, `ResponseBody`, `XmlParse`, `UrlParse`, `DateTimeParse`, `ClientBuild`, and `HtmlNotAvailable`, each wrapping the underlying error as a `#[source]`.
- **Async runtime**: Tokio with `features = ["full"]`. All I/O is async.
- **Logging**: `tracing` crate with `#[instrument]` attributes, `debug!` and `warn!` macros.
- **Serialization**: `serde` + `quick-xml` for XML deserialization. `serde_with` for datetime handling. XML field renames use `@` prefix for attributes (`@title`, `@rel`, etc.).
- **Builder pattern**: `ArxivQuery` uses `with_*` methods that consume and return `self`.
- **Generics**: `ArxivQuery<S>` is generic over `S: ToString` to accept both `&str` and structured `SearchPredicate`/`SearchTerm`/`SearchRange` types.
- **Lifetimes**: `SearchPredicate<'a>` uses trait objects (`Box<dyn ISearchQuery + 'a>`) for composable query predicates.
- **Naming**: PascalCase for types/enums, snake_case for functions/variables. Internal serde fields use `_` suffix (e.g., `entries_`).
- **Visibility**: Internal types (`Feed`, `Entry`) are `pub(crate)` or module-private. Public API is re-exported from `lib.rs`.

## Dependencies

| Crate          | Purpose                                                     |
| -------------- | ----------------------------------------------------------- |
| `thiserror`    | Derive macro for custom error types                         |
| `quick-xml`    | XML parsing (with `serialize`, `overlapped-lists` features) |
| `reqwest`      | HTTP client                                                 |
| `serde`        | Serialization/deserialization (with `derive`)               |
| `serde_with`   | DateTime serde support (with `time_0_3`)                    |
| `time`         | DateTime types and formatting                               |
| `tokio`        | Async runtime                                               |
| `tracing`      | Structured logging                                          |
| `url`          | URL construction                                            |
| `maplit` (dev) | HashMap literal macros for tests                            |

## Formatting

Two formatters are used:

1. **rustfmt** — Standard Rust formatting. Checked via `cargo fmt --check`.
2. **dprint** — Formats JSON, Markdown, TOML, and YAML files. Config in `dprint.json`.

Both are enforced in CI.
