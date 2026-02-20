mod error;
mod models;
mod query;
mod search_query;

pub use error::{ArxivError, Result};
pub use models::{ArxivAuthor, ArxivResult, Link, SearchResponse};
pub use query::*;
pub use search_query::{RangeField, SearchField, SearchPredicate, SearchRange, SearchTerm};

use tracing::{debug, instrument, warn};

const BASE_URL: &str = "https://export.arxiv.org/api/query";

#[derive(Debug, Clone)]
pub struct ArxivClient {
    client: reqwest::Client,
    initial_interval: std::time::Duration,
    n_retries: usize,
}

impl Default for ArxivClient {
    fn default() -> Self {
        Self {
            client: reqwest::Client::new(),
            initial_interval: std::time::Duration::from_secs(3),
            n_retries: 3,
        }
    }
}

impl ArxivClient {
    /// Create a new client. `interval` is the base delay before the first retry;
    /// subsequent retries use exponential backoff (interval * 2^(attempt-1)).
    pub fn new(interval: std::time::Duration, n_retries: usize) -> Self {
        Self {
            client: reqwest::Client::new(),
            initial_interval: interval,
            n_retries,
        }
    }

    #[instrument(skip(self, query), fields(n_retries = self.n_retries))]
    pub async fn search<S: ToString>(&self, query: ArxivQuery<S>) -> Result<SearchResponse> {
        let url = query.to_url(BASE_URL)?;
        debug!(url = %url, "Fetching from arXiv API");

        let mut errors = vec![];

        for attempt in 1..=self.n_retries {
            debug!(attempt, "Sending request");
            let response = match self.client.get(&url).send().await {
                Ok(resp) => resp,
                Err(e) => {
                    warn!(attempt, error = %e, "Request failed");
                    errors.push(e);
                    let backoff = self.initial_interval * 2u32.saturating_pow(attempt as u32 - 1);
                    debug!(?backoff, "Waiting before retry");
                    tokio::time::sleep(backoff).await;
                    continue;
                }
            };

            let status = response.status();
            debug!(%status, "Received response");

            if !status.is_success() {
                let body = response
                    .text()
                    .await
                    .unwrap_or_else(|_| String::from("<failed to read body>"));
                return Err(ArxivError::HttpStatus { status, body });
            }

            let text = response.text().await.map_err(ArxivError::ResponseBody)?;
            let feed =
                quick_xml::de::from_str::<models::Feed>(&text).map_err(ArxivError::XmlParse)?;

            let response = SearchResponse::from_feed(feed);
            debug!(count = response.results.len(), "Search completed");
            return Ok(response);
        }

        Err(ArxivError::RequestFailed {
            retries: self.n_retries,
            errors,
        })
    }
}

#[cfg(test)]
mod test {
    use search_query::{RangeField, SearchField, SearchPredicate, SearchRange, SearchTerm};
    use time::format_description::well_known::Rfc3339;
    use time::OffsetDateTime;

    use super::*;

    #[tokio::test]
    async fn test_search() {
        let max_results = 3;
        let client = ArxivClient::new(std::time::Duration::from_secs(1), 3);
        let query = ArxivQuery::default()
            .with_search_query("all:RAG")
            .with_max_results(max_results);

        let response = client.search(query).await.unwrap();
        assert_eq!(response.results.len(), max_results);
        assert!(response.total_results > 0);
        assert_eq!(response.start_index, 0);
        assert_eq!(response.items_per_page, max_results);
    }

    #[tokio::test]
    async fn test_search_with_id_list() {
        let client = ArxivClient::new(std::time::Duration::from_secs(1), 3);
        let query: ArxivQuery<&str> =
            ArxivQuery::default().with_id_list(vec!["2402.16893v1".to_string()]);

        let response = client.search(query).await.unwrap();
        assert_eq!(response.results.len(), 1);

        let result = &response.results[0];
        assert_eq!(result.id, "http://arxiv.org/abs/2402.16893v1");
        assert_eq!(result.arxiv_id, "2402.16893v1");
        assert!(result
            .title
            .contains("Exploring Privacy Issues in Retrieval-Augmented"));
    }

    #[tokio::test]
    async fn test_with_search_query() {
        let term1 = SearchTerm::new(SearchField::Title, "RAG");
        let term2 = SearchTerm::new(SearchField::Abstract, "hallucination");
        let search_query = SearchPredicate::and(term1, term2);
        assert_eq!(search_query.to_string(), "ti:RAG AND abs:hallucination");

        let client = ArxivClient::new(std::time::Duration::from_secs(1), 3);
        let query = ArxivQuery::default()
            .with_search_query(search_query)
            .with_max_results(2);

        let response = client.search(query).await.unwrap();
        assert!(!response.results.is_empty());
    }

    #[tokio::test]
    async fn test_with_search_range() {
        let start = OffsetDateTime::parse("2022-04-12T23:20:50.52Z", &Rfc3339).unwrap();
        let end = OffsetDateTime::parse("2023-04-13T23:20:50.52Z", &Rfc3339).unwrap();

        let range = SearchRange::new(RangeField::LastUpdatedDate, start, end);

        let client = ArxivClient::new(std::time::Duration::from_secs(1), 3);
        let query = ArxivQuery::default()
            .with_search_query(range)
            .with_max_results(2);

        let response = client.search(query).await.unwrap();
        assert!(!response.results.is_empty());
    }

    #[tokio::test]
    async fn test_with_search_query_and_range() {
        let term1 = SearchTerm::new(SearchField::Title, "graph");
        let term2 = SearchTerm::new(SearchField::Abstract, "graph");
        let search_query = SearchPredicate::and(term1, term2);

        let start = OffsetDateTime::parse("2022-04-12T23:20:50.52Z", &Rfc3339).unwrap();
        let end = OffsetDateTime::parse("2024-04-13T23:20:50.52Z", &Rfc3339).unwrap();
        let range = SearchRange::new(RangeField::SubmittedDate, start, end);

        let and_predicate = SearchPredicate::and(search_query, range);
        let client = ArxivClient::new(std::time::Duration::from_secs(1), 3);
        let query = ArxivQuery::default()
            .with_search_query(and_predicate)
            .with_max_results(2);

        let response = client.search(query).await.unwrap();
        assert_eq!(response.results.len(), 2);
    }
}
