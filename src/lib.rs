mod models;
mod query;

pub use models::ArxivResult;
pub use query::*;

const BASE_URL: &str = "http://export.arxiv.org/api/query";

#[derive(Debug, Clone)]
pub struct ArxivClient {
    client: reqwest::Client,
    interval: std::time::Duration,
    n_retries: usize,
    last_request: std::time::Instant,
}

impl Default for ArxivClient {
    fn default() -> Self {
        Self {
            client: reqwest::Client::new(),
            interval: std::time::Duration::from_secs(3),
            n_retries: 3,
            last_request: std::time::Instant::now(),
        }
    }
}

impl ArxivClient {
    pub fn new(interval: std::time::Duration, n_retries: usize) -> Self {
        Self {
            client: reqwest::Client::new(),
            interval,
            n_retries,
            last_request: std::time::Instant::now(),
        }
    }

    pub async fn search(&self, query: ArxivQuery<&str>) -> anyhow::Result<Vec<ArxivResult>> {
        if self.last_request.elapsed() < self.interval {
            tokio::time::sleep(self.interval - self.last_request.elapsed()).await;
        }

        let mut results = Vec::new();
        for _ in 0..self.n_retries {
            let response = self.client.get(&query.to_url(BASE_URL)?).send().await;

            if let Err(e) = response {
                eprintln!("Error: {}", e);
                tokio::time::sleep(self.interval).await;
                continue;
            }

            let response = response.unwrap();

            let text = response.text().await?;
            let feed = quick_xml::de::from_str::<models::Feed>(&text)?;

            results.extend(feed.entries_.into_iter().map(ArxivResult::from_entry));
            break;
        }
        Ok(results)
    }
}

#[cfg(test)]
mod test {
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
        let query = ArxivQuery::default().with_id_list(vec!["2402.16893v1".to_string()]);

        let results = client.search(query).await.unwrap();
        assert_eq!(results.len(), 1);
    }
}
