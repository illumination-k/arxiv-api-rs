use std::collections::HashMap;

use anyhow::Context as _;

#[derive(Debug, Clone)]
pub enum SortBy {
    Relevance,
    LastUpdatedDate,
    SubmittedDate,
}

#[derive(Debug, Clone)]
pub enum SortOrder {
    Ascending,
    Descending,
}

#[derive(Debug, Clone)]
pub struct ArxivQuery<S> {
    search_query: Option<S>,
    id_list: Vec<String>,
    start: usize,
    max_results: usize,
    sort_by: Option<SortBy>,
    sort_order: Option<SortOrder>,
}

impl<S> Default for ArxivQuery<S> {
    fn default() -> Self {
        Self {
            search_query: None,
            id_list: Vec::new(),
            start: 0,
            max_results: 10,
            sort_by: None,
            sort_order: None,
        }
    }
}

impl<S> ArxivQuery<S> {
    pub fn with_search_query(mut self, search_query: S) -> Self {
        self.search_query = Some(search_query);
        self
    }

    pub fn with_id_list(mut self, id_list: Vec<String>) -> Self {
        self.id_list = id_list;
        self
    }

    pub fn with_start(mut self, start: usize) -> Self {
        self.start = start;
        self
    }

    pub fn with_max_results(mut self, max_results: usize) -> Self {
        self.max_results = max_results;
        self
    }

    pub fn with_sort_by(mut self, sort_by: SortBy) -> Self {
        self.sort_by = Some(sort_by);
        self
    }

    pub fn next_page_query(mut self) -> Self {
        self.start += self.max_results;
        self
    }
}

impl<S> ArxivQuery<S>
where
    S: ToString,
{
    fn query_map(&self) -> HashMap<&str, String> {
        let mut query_map = HashMap::new();

        if let Some(search_query) = &self.search_query {
            query_map.insert("search_query", search_query.to_string());
        }

        if !self.id_list.is_empty() {
            query_map.insert("id_list", self.id_list.join(","));
        }

        query_map.insert("start", self.start.to_string());
        query_map.insert("max_results", self.max_results.to_string());

        if let Some(sort_by) = &self.sort_by {
            query_map.insert(
                "sortBy",
                match sort_by {
                    SortBy::Relevance => "relevance".to_string(),
                    SortBy::LastUpdatedDate => "lastUpdatedDate".to_string(),
                    SortBy::SubmittedDate => "submittedDate".to_string(),
                },
            );
        }

        if let Some(sort_order) = &self.sort_order {
            query_map.insert(
                "sortOrder",
                match sort_order {
                    SortOrder::Ascending => "ascending".to_string(),
                    SortOrder::Descending => "descending".to_string(),
                },
            );
        }

        query_map
    }

    pub(crate) fn to_url(&self, base: &str) -> anyhow::Result<String> {
        let url = url::Url::parse_with_params(base, self.query_map())
            .with_context(|| format!("Failed to parse URL with params: {:?}", self.query_map()))?
            .to_string();

        Ok(url)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use maplit::hashmap;

    #[test]
    fn test_default() {
        let query: ArxivQuery<&str> = ArxivQuery::default();
        let actual = query.query_map();

        let expected = hashmap! {
            "start" => "0".to_string(),
            "max_results" => "10".to_string(),
        };

        assert_eq!(actual, expected);
    }

    #[test]
    fn test_with_search_query() {
        let query = ArxivQuery::default().with_search_query("all:RAG");

        let actual = query.query_map();
        let expected = hashmap! {
            "search_query" => "all:RAG".to_string(),
            "start" => "0".to_string(),
            "max_results" => "10".to_string(),
        };

        assert_eq!(actual, expected);
    }

    #[test]
    fn test_with_id_list() {
        let query: ArxivQuery<&str> =
            ArxivQuery::default().with_id_list(vec!["2402.16893v1".to_string()]);
        let actual = query.query_map();

        let expected = hashmap! {
            "id_list" => "2402.16893v1".to_string(),
            "start" => "0".to_string(),
            "max_results" => "10".to_string(),
        };

        assert_eq!(actual, expected);
    }
}
