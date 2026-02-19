# CLAUDE.md

## Project Overview

`arxiv-api-rs` is a Rust library for querying the arXiv.org API. It provides an async HTTP client, a builder-pattern query API, a search DSL with boolean logic and date ranges, and typed data models for arXiv paper results.

## Repository Structure

```
src/
├── lib.rs            # Library entry point, ArxivClient (HTTP client with retries), re-exports
├── error.rs          # ArxivError enum (thiserror), Result type alias
├── models.rs         # Data models: Feed (XML root), Entry (internal), ArxivResult (public output)
├── query.rs          # ArxivQuery<S> builder, SortBy/SortOrder enums, URL construction
└── search_query.rs   # Search DSL: SearchTerm, SearchRange, SearchPredicate, ISearchQuery trait
```

- `lib.rs` — `ArxivClient` struct with configurable retries and interval. `search()` method makes async HTTP requests to arXiv, parses XML, and returns `Vec<ArxivResult>`.
- `models.rs` — Serde/quick-xml deserialization of arXiv Atom XML. Internal `Entry`/`Feed` types are converted to the public `ArxivResult`.
- `query.rs` — Generic `ArxivQuery<S>` with builder methods (`with_search_query`, `with_id_list`, `with_max_results`, `with_start`, `with_sort_by`, `next_page_query`). Builds URL query parameters.
- `search_query.rs` — `ISearchQuery` trait, `SearchTerm` (field:value), `SearchRange` (date ranges), and `SearchPredicate` (AND/OR/ANDNOT/Bracket combinators with lifetime `'a`).

## Build and Development Commands

```bash
# Build the project
cargo build

# Run all tests (includes integration tests that make real HTTP requests to arXiv)
cargo test

# Check formatting (CI uses this)
cargo fmt --check

# Apply formatting
cargo fmt

# Lint with clippy (CI treats warnings as errors)
cargo clippy -- --deny warnings

# Check dprint formatting (JSON, Markdown, TOML, YAML)
dprint check

# Apply dprint formatting
dprint fmt
```

## CI Pipeline

Defined in `.github/workflows/ci.yaml`. Triggers on pushes to `main` and all PRs. Three jobs:

1. **actionlint** — Lints the CI workflow file itself.
2. **dprint** — Checks formatting of JSON, Markdown, TOML, and YAML files via `dprint check`.
3. **lint_and_test** — Runs `cargo fmt --check`, `cargo clippy -- --deny warnings`, and `cargo test`.

All three jobs must pass. Clippy warnings are treated as errors (`--deny warnings`).

## Testing

Tests are inline (`#[cfg(test)]` modules) in each source file:

- `lib.rs` — 5 async integration tests (`#[tokio::test]`) that make real HTTP requests to the arXiv API. These may be slow or flaky due to network dependency.
- `query.rs` — 3 sync unit tests for query builder and URL parameter construction.
- `search_query.rs` — 4 sync unit tests for search term formatting, date range formatting, and predicate composition.

Run all tests with `cargo test`. There is no separate integration test directory.

## Code Conventions

- **Error handling**: Uses `thiserror` with a custom `ArxivError` enum in `error.rs`. Public API returns `crate::Result<T>` (alias for `Result<T, ArxivError>`). Error variants include `RequestFailed`, `HttpStatus`, `ResponseBody`, `XmlParse`, `UrlParse`, and `DateTimeParse`, each wrapping the underlying error as a `#[source]`.
- **Async runtime**: Tokio with `features = ["full"]`. All I/O is async.
- **Logging**: `tracing` crate with `#[instrument]` attributes, `debug!` and `warn!` macros.
- **Serialization**: `serde` + `quick-xml` for XML deserialization. `serde_with` for datetime handling. XML field renames use `@` prefix for attributes (`@title`, `@rel`, etc.).
- **Builder pattern**: `ArxivQuery` uses `with_*` methods that consume and return `self`.
- **Generics**: `ArxivQuery<S>` is generic over `S: ToString` to accept both `&str` and structured `SearchPredicate`/`SearchTerm`/`SearchRange` types.
- **Lifetimes**: `SearchPredicate<'a>` uses trait objects (`Box<dyn ISearchQuery + 'a>`) for composable query predicates.
- **Naming**: PascalCase for types/enums, snake_case for functions/variables. Internal serde fields use `_` suffix (e.g., `entries_`).
- **Visibility**: Internal types (`Feed`, `Entry`) are `pub(crate)` or module-private. Public API is re-exported from `lib.rs`.

## Dependencies

| Crate | Purpose |
|-------|---------|
| `thiserror` | Derive macro for custom error types |
| `quick-xml` | XML parsing (with `serialize`, `overlapped-lists` features) |
| `reqwest` | HTTP client |
| `serde` | Serialization/deserialization (with `derive`) |
| `serde_with` | DateTime serde support (with `time_0_3`) |
| `time` | DateTime types and formatting |
| `tokio` | Async runtime |
| `tracing` | Structured logging |
| `url` | URL construction |
| `maplit` (dev) | HashMap literal macros for tests |

## Formatting

Two formatters are used:

1. **rustfmt** — Standard Rust formatting. Checked via `cargo fmt --check`.
2. **dprint** — Formats JSON, Markdown, TOML, and YAML files. Config in `dprint.json`.

Both are enforced in CI.
