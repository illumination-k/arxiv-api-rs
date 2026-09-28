mod client;
mod error;
#[cfg(feature = "html")]
pub mod html_parser;
mod models;
mod query;
mod search_query;

pub use client::{ArxivClient, ArxivClientBuilder};
pub use error::{ArxivError, Result};
#[cfg(feature = "html")]
pub use html_parser::{ArxivPaper, ContentBlock, Reference, Section};
pub use models::{ArxivAuthor, ArxivResult, Link, SearchResponse};
pub use query::*;
pub use search_query::{RangeField, SearchField, SearchPredicate, SearchRange, SearchTerm};

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
