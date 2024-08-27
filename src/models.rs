use serde::{Deserialize, Serialize};
use time::serde::iso8601;
use time::OffsetDateTime;

#[derive(Debug, Clone, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct Feed {
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
            eprintln!("Multiple pdf links found for entry: {}", self.id);
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
    title: Option<String>,
    #[serde(rename = "@rel")]
    rel: String,
    #[serde(rename = "@href")]
    href: String,
    #[serde(rename = "@type")]
    content_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct Category {
    #[serde(rename = "@term")]
    term: String,
    #[serde(rename = "@scheme")]
    scheme: String,
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
