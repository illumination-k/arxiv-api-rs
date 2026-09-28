use arxiv_api_rs::{ArxivClient, ArxivQuery, ArxivResult, SortBy, SortOrder};
use rmcp::{
    handler::server::{
        router::tool::ToolRouter,
        wrapper::{Json, Parameters},
    },
    model::{Implementation, ServerCapabilities, ServerConfig},
    tool, tool_handler, tool_router, ServerHandler,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

const MAX_RESULTS_LIMIT: usize = 100;

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SortByParam {
    Relevance,
    LastUpdatedDate,
    SubmittedDate,
}

impl From<SortByParam> for SortBy {
    fn from(value: SortByParam) -> Self {
        match value {
            SortByParam::Relevance => SortBy::Relevance,
            SortByParam::LastUpdatedDate => SortBy::LastUpdatedDate,
            SortByParam::SubmittedDate => SortBy::SubmittedDate,
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SortOrderParam {
    Ascending,
    Descending,
}

impl From<SortOrderParam> for SortOrder {
    fn from(value: SortOrderParam) -> Self {
        match value {
            SortOrderParam::Ascending => SortOrder::Ascending,
            SortOrderParam::Descending => SortOrder::Descending,
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchPapersParams {
    /// arXiv search query. Supports field prefixes (ti:, au:, abs:, co:, jr:, cat:, rn:, id:, all:),
    /// boolean operators (AND, OR, ANDNOT), parentheses, quoted phrases, and date ranges
    /// such as `submittedDate:[202401010000 TO 202412312359]`.
    /// Example: `ti:"large language model" AND cat:cs.CL`.
    pub query: String,
    /// Maximum number of results to return (1-100, default 10).
    pub max_results: Option<usize>,
    /// Offset of the first result for pagination (default 0).
    pub start: Option<usize>,
    /// Sort key (default: relevance).
    pub sort_by: Option<SortByParam>,
    /// Sort order (default: descending).
    pub sort_order: Option<SortOrderParam>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetPapersParams {
    /// arXiv identifiers, e.g. `["2402.16893v1", "hep-th/9901001"]`.
    pub ids: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetPaperContentParams {
    /// arXiv identifier of the paper, e.g. `2402.16893v1`.
    pub arxiv_id: String,
}

/// Compact paper representation returned to MCP clients.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PaperSummary {
    /// arXiv identifier including version, e.g. `2402.16893v1`.
    arxiv_id: String,
    title: String,
    /// Author names.
    authors: Vec<String>,
    /// Abstract.
    summary: String,
    primary_category: String,
    categories: Vec<String>,
    /// Date of the first version (`YYYY-MM-DD`).
    published: String,
    /// Date of the latest version (`YYYY-MM-DD`).
    updated: String,
    pdf_url: Option<String>,
    doi: Option<String>,
    comment: Option<String>,
    journal_ref: Option<String>,
}

impl From<ArxivResult> for PaperSummary {
    fn from(r: ArxivResult) -> Self {
        let date = |d: time::OffsetDateTime| d.date().to_string();
        Self {
            arxiv_id: r.arxiv_id,
            title: normalize_whitespace(&r.title),
            authors: r.authors.into_iter().map(|a| a.name).collect(),
            summary: normalize_whitespace(&r.summary),
            primary_category: r.primary_category,
            categories: r.categories,
            published: date(r.published),
            updated: date(r.updated),
            pdf_url: r.pdf_url,
            doi: r.doi,
            comment: r.comment,
            journal_ref: r.journal_ref,
        }
    }
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SearchOutput {
    /// Total number of papers matching the query.
    total_results: usize,
    /// Offset of the first returned paper.
    start_index: usize,
    /// Number of papers in `papers`.
    returned: usize,
    papers: Vec<PaperSummary>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct GetPapersOutput {
    papers: Vec<PaperSummary>,
}

fn normalize_whitespace(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[derive(Debug, Clone)]
pub struct ArxivServer {
    client: ArxivClient,
    tool_router: ToolRouter<Self>,
}

impl Default for ArxivServer {
    fn default() -> Self {
        Self::new(ArxivClient::default())
    }
}

#[tool_router]
impl ArxivServer {
    pub fn new(client: ArxivClient) -> Self {
        Self {
            client,
            tool_router: Self::tool_router(),
        }
    }

    /// Search arXiv papers and return their metadata (title, authors, abstract, categories, dates, PDF URL).
    #[tool]
    async fn search_papers(
        &self,
        Parameters(params): Parameters<SearchPapersParams>,
    ) -> Result<Json<SearchOutput>, String> {
        let query = build_search_query(&params)?;
        let response = self.client.search(query).await.map_err(|e| e.to_string())?;
        let papers: Vec<PaperSummary> = response.results.into_iter().map(Into::into).collect();
        Ok(Json(SearchOutput {
            total_results: response.total_results,
            start_index: response.start_index,
            returned: papers.len(),
            papers,
        }))
    }

    /// Fetch metadata for specific arXiv papers by their identifiers.
    #[tool]
    async fn get_papers(
        &self,
        Parameters(params): Parameters<GetPapersParams>,
    ) -> Result<Json<GetPapersOutput>, String> {
        if params.ids.is_empty() {
            return Err("`ids` must contain at least one arXiv identifier".to_string());
        }
        let n = params.ids.len();
        let query: ArxivQuery<&str> = ArxivQuery::default()
            .with_id_list(params.ids)
            .with_max_results(n);
        let response = self.client.search(query).await.map_err(|e| e.to_string())?;
        let papers: Vec<PaperSummary> = response.results.into_iter().map(Into::into).collect();
        Ok(Json(GetPapersOutput { papers }))
    }

    /// Fetch the full text of an arXiv paper (from its HTML version) converted to Markdown.
    /// Not all papers have an HTML version; older papers may be unavailable.
    #[tool]
    async fn get_paper_content(
        &self,
        Parameters(params): Parameters<GetPaperContentParams>,
    ) -> Result<String, String> {
        let arxiv_id = params.arxiv_id.trim();
        if arxiv_id.is_empty() {
            return Err("`arxiv_id` must not be empty".to_string());
        }
        let paper = self
            .client
            .fetch_html(arxiv_id)
            .await
            .map_err(|e| e.to_string())?;
        Ok(paper.to_markdown())
    }
}

fn build_search_query(params: &SearchPapersParams) -> Result<ArxivQuery<String>, String> {
    let query = params.query.trim();
    if query.is_empty() {
        return Err("`query` must not be empty".to_string());
    }
    let max_results = params.max_results.unwrap_or(10);
    if !(1..=MAX_RESULTS_LIMIT).contains(&max_results) {
        return Err(format!(
            "`max_results` must be between 1 and {MAX_RESULTS_LIMIT}"
        ));
    }

    let mut q = ArxivQuery::default()
        .with_search_query(query.to_string())
        .with_max_results(max_results)
        .with_start(params.start.unwrap_or(0));
    if let Some(sort_by) = params.sort_by {
        q = q.with_sort_by(sort_by.into());
    }
    if let Some(sort_order) = params.sort_order {
        q = q.with_sort_order(sort_order.into());
    }
    Ok(q)
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for ArxivServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                env!("CARGO_PKG_NAME"),
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(
            "Tools for searching arXiv and reading papers. Use `search_papers` to find papers, \
             `get_papers` to look up known IDs, and `get_paper_content` to read a paper's full text as Markdown.",
        )
    }
}

#[cfg(test)]
mod test {
    use super::*;

    fn params(query: &str, max_results: Option<usize>) -> SearchPapersParams {
        SearchPapersParams {
            query: query.to_string(),
            max_results,
            start: None,
            sort_by: None,
            sort_order: None,
        }
    }

    #[test]
    fn test_tools_are_registered() {
        let server = ArxivServer::default();
        let mut names: Vec<String> = server
            .tool_router
            .list_all()
            .into_iter()
            .map(|t| t.name.to_string())
            .collect();
        names.sort();
        assert_eq!(names, ["get_paper_content", "get_papers", "search_papers"]);
    }

    #[test]
    fn test_output_schemas() {
        let server = ArxivServer::default();
        let has_schema = |name: &str| {
            server
                .tool_router
                .get(name)
                .unwrap()
                .output_schema
                .is_some()
        };
        assert!(has_schema("search_papers"));
        assert!(has_schema("get_papers"));
        assert!(!has_schema("get_paper_content"));
    }

    #[test]
    fn test_build_search_query_validation() {
        assert!(build_search_query(&params("  ", None)).is_err());
        assert!(build_search_query(&params("all:RAG", Some(0))).is_err());
        assert!(build_search_query(&params("all:RAG", Some(101))).is_err());
        assert!(build_search_query(&params("all:RAG", Some(100))).is_ok());
        assert!(build_search_query(&params("all:RAG", None)).is_ok());
    }

    #[test]
    fn test_params_deserialize() {
        let p: SearchPapersParams = serde_json::from_value(serde_json::json!({
            "query": "ti:graph",
            "sort_by": "submitted_date",
            "sort_order": "ascending"
        }))
        .unwrap();
        assert!(matches!(p.sort_by, Some(SortByParam::SubmittedDate)));
        assert!(matches!(p.sort_order, Some(SortOrderParam::Ascending)));
    }

    #[test]
    fn test_normalize_whitespace() {
        assert_eq!(normalize_whitespace("  a\n  b\tc "), "a b c");
    }
}
