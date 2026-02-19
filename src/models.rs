use serde::{Deserialize, Serialize};
use time::serde::iso8601;
use time::OffsetDateTime;
use tracing::warn;

#[derive(Debug, Clone, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct Feed {
    #[serde(rename = "totalResults", default)]
    pub total_results_: usize,
    #[serde(rename = "startIndex", default)]
    pub start_index_: usize,
    #[serde(rename = "itemsPerPage", default)]
    pub items_per_page_: usize,
    #[serde(rename = "entry", default)]
    pub entries_: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct Entry {
    id: String,
    title: String,
    summary: String,
    #[serde(with = "iso8601")]
    updated: OffsetDateTime,
    #[serde(with = "iso8601")]
    published: OffsetDateTime,
    #[serde(rename = "author", default)]
    authors: Vec<Author>,
    #[serde(rename = "link", default)]
    links: Vec<Link>,
    #[serde(rename = "primary_category")]
    primary_category: Category,
    #[serde(rename = "category", default)]
    categories: Vec<Category>,
    #[serde(rename = "doi")]
    doi: Option<String>,
    #[serde(rename = "comment")]
    comment: Option<String>,
    #[serde(rename = "journal_ref")]
    journal_ref: Option<String>,
}

impl Entry {
    fn get_pdf_url(&self) -> Option<String> {
        let mut pdf_links = self
            .links
            .iter()
            .filter(|link| link.title == Some("pdf".to_string()));

        let ret = pdf_links.next().map(|link| link.href.clone());

        if pdf_links.next().is_some() {
            warn!(entry_id = %self.id, "Multiple pdf links found for entry");
        }

        ret
    }
}

#[derive(Debug, Clone, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct Author {
    name: String,
}

#[derive(Debug, Clone, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct Link {
    #[serde(rename = "@title")]
    pub title: Option<String>,
    #[serde(rename = "@rel")]
    pub rel: String,
    #[serde(rename = "@href")]
    pub href: String,
    #[serde(rename = "@type")]
    pub content_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct Category {
    #[serde(rename = "@term")]
    term: String,
    #[serde(rename = "@scheme")]
    scheme: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArxivResult {
    pub id: String,
    pub title: String,
    pub summary: String,
    pub authors: Vec<String>,

    pub doi: Option<String>,
    pub comment: Option<String>,
    pub journal_ref: Option<String>,

    pub primary_category: String,
    pub categories: Vec<String>,

    pub pdf_url: Option<String>,
    pub links: Vec<Link>,

    #[serde(with = "iso8601")]
    pub published: OffsetDateTime,
    #[serde(with = "iso8601")]
    pub updated: OffsetDateTime,
}

/// Response from an arXiv API search, including pagination metadata.
#[derive(Debug, Clone)]
pub struct SearchResponse {
    /// Total number of results matching the query.
    pub total_results: usize,
    /// The index of the first result in this response.
    pub start_index: usize,
    /// The number of results per page.
    pub items_per_page: usize,
    /// The arXiv paper results.
    pub results: Vec<ArxivResult>,
}

impl SearchResponse {
    pub(crate) fn from_feed(feed: Feed) -> Self {
        let results = feed
            .entries_
            .into_iter()
            .map(ArxivResult::from_entry)
            .collect();
        Self {
            total_results: feed.total_results_,
            start_index: feed.start_index_,
            items_per_page: feed.items_per_page_,
            results,
        }
    }
}

impl ArxivResult {
    pub(crate) fn from_entry(entry: Entry) -> Self {
        let pdf_url = entry.get_pdf_url();
        Self {
            id: entry.id,
            title: entry.title,
            summary: entry.summary,
            authors: entry
                .authors
                .into_iter()
                .map(|author| author.name)
                .collect(),
            doi: entry.doi,
            comment: entry.comment,
            journal_ref: entry.journal_ref,
            primary_category: entry.primary_category.term,
            categories: entry
                .categories
                .into_iter()
                .map(|category| category.term)
                .collect(),
            pdf_url,
            links: entry.links,
            published: entry.published,
            updated: entry.updated,
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    fn parse_fixture(filename: &str) -> SearchResponse {
        let path = format!("{}/tests/fixtures/{filename}", env!("CARGO_MANIFEST_DIR"));
        let xml = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("Failed to read fixture {path}: {e}"));
        let feed: Feed = quick_xml::de::from_str(&xml)
            .unwrap_or_else(|e| panic!("Failed to parse fixture {path}: {e}"));
        SearchResponse::from_feed(feed)
    }

    #[test]
    fn test_parse_search_all_rag() {
        let resp = parse_fixture("search_all_rag.xml");
        assert_eq!(resp.results.len(), 3);
        assert_eq!(resp.items_per_page, 3);
        assert_eq!(resp.start_index, 0);
        assert!(resp.total_results > 0);

        let first = &resp.results[0];
        assert!(!first.title.is_empty());
        assert!(!first.authors.is_empty());
        assert!(first.pdf_url.is_some());
        assert!(!first.primary_category.is_empty());
    }

    #[test]
    fn test_parse_search_by_id() {
        let resp = parse_fixture("search_by_id.xml");
        assert_eq!(resp.results.len(), 1);
        assert_eq!(resp.total_results, 1);

        let result = &resp.results[0];
        assert_eq!(result.id, "http://arxiv.org/abs/2402.16893v1");
        assert!(result.title.starts_with("The Good and The Bad"));
        assert_eq!(result.primary_category, "cs.CR");
        assert!(result.pdf_url.is_some());
        assert!(!result.links.is_empty());
        assert!(result.links.iter().any(|l| l.rel == "alternate"));
    }

    #[test]
    fn test_parse_search_title_abstract() {
        let resp = parse_fixture("search_title_abstract.xml");
        assert_eq!(resp.results.len(), 2);
        assert_eq!(resp.items_per_page, 2);
        assert!(resp.total_results > 0);

        for result in &resp.results {
            assert!(!result.id.is_empty());
            assert!(!result.summary.is_empty());
        }
    }

    #[test]
    fn test_parse_search_date_range() {
        let resp = parse_fixture("search_date_range.xml");
        assert_eq!(resp.results.len(), 2);
        assert_eq!(resp.items_per_page, 2);
        assert!(resp.total_results > 0);
    }

    #[test]
    fn test_parse_search_query_and_range() {
        let resp = parse_fixture("search_query_and_range.xml");
        assert_eq!(resp.results.len(), 2);
        assert_eq!(resp.items_per_page, 2);
        assert!(resp.total_results > 0);

        for result in &resp.results {
            assert!(!result.categories.is_empty());
        }
    }
}
