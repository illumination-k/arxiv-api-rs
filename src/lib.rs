mod models;
mod query;
mod search_query;

pub use models::ArxivResult;
pub use query::*;
pub use search_query::{RangeField, SearchField, SearchPredicate, SearchRange, SearchTerm};

use anyhow::anyhow;

const BASE_URL: &str = "http://export.arxiv.org/api/query";

#[derive(Debug, Clone)]
pub struct ArxivClient {
    client: reqwest::Client,
    interval: std::time::Duration,
    n_retries: usize,
}

impl Default for ArxivClient {
    fn default() -> Self {
        Self {
            client: reqwest::Client::new(),
            interval: std::time::Duration::from_secs(3),
            n_retries: 3,
        }
    }
}

impl ArxivClient {
    pub fn new(interval: std::time::Duration, n_retries: usize) -> Self {
        Self {
            client: reqwest::Client::new(),
            interval,
            n_retries,
        }
    }

    pub async fn search<S: ToString>(
        &self,
        query: ArxivQuery<S>,
    ) -> anyhow::Result<Vec<ArxivResult>> {
        let mut errors = vec![];

        for _ in 0..self.n_retries {
            let response = self.client.get(&query.to_url(BASE_URL)?).send().await;

            if let Err(e) = response {
                errors.push(e);
                tokio::time::sleep(self.interval).await;
                continue;
            }

            let response = response.unwrap();

            let text = response.text().await?;
            let feed = quick_xml::de::from_str::<models::Feed>(&text)?;

            return Ok(feed
                .entries_
                .into_iter()
                .map(ArxivResult::from_entry)
                .collect());
        }

        let err_msgs = errors
            .into_iter()
            .map(|e| e.to_string())
            .collect::<Vec<String>>()
            .join("\n");

        Err(anyhow!(
            "Failed to fetch data from Arxiv after {} retries\n{}",
            self.n_retries,
            err_msgs
        ))
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
        let max_results = 100;
        let client = ArxivClient::new(std::time::Duration::from_secs(1), 3);
        let query = ArxivQuery::default()
            .with_search_query("all:RAG")
            .with_max_results(max_results);

        let results = client.search(query).await.unwrap();
        assert_eq!(results.len(), max_results);
    }

    #[tokio::test]
    async fn test_search_with_id_list() {
        let client = ArxivClient::new(std::time::Duration::from_secs(1), 3);
        let query: ArxivQuery<&str> =
            ArxivQuery::default().with_id_list(vec!["2402.16893v1".to_string()]);

        let results = client.search(query).await.unwrap();
        assert_eq!(results.len(), 1);

        let result = &results[0];
        assert_eq!(result.id, "http://arxiv.org/abs/2402.16893v1");
        assert_eq!(result.title, "The Good and The Bad: Exploring Privacy Issues in Retrieval-Augmented\n  Generation (RAG)");
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

        let results = client.search(query).await.unwrap();
        assert!(!results.is_empty());
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

        let results = client.search(query).await.unwrap();
        assert!(!results.is_empty());
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
        //println!("{}", and_predicate.to_string());
        let client = ArxivClient::new(std::time::Duration::from_secs(1), 3);
        let query = ArxivQuery::default()
            .with_search_query(and_predicate)
            .with_max_results(2);

        let results = client.search(query).await.unwrap();

        assert_eq!(results.len(), 2);
    }
}
