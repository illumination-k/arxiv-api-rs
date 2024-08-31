use std::fmt::{Debug, Display};

use time::format_description::well_known::{Iso8601, Rfc2822, Rfc3339};
use time::macros::format_description;
use time::OffsetDateTime;

fn remove_outside_brackets(s: &str) -> String {
    if s.starts_with('(') && s.ends_with(')') {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

pub trait ISearchQuery {
    fn to_query_string(&self) -> String;
}

#[derive(Debug, Clone)]
pub enum RangeField {
    LastUpdatedDate,
    SubmittedDate,
}

impl AsRef<str> for RangeField {
    fn as_ref(&self) -> &str {
        match self {
            RangeField::LastUpdatedDate => "lastUpdatedDate",
            RangeField::SubmittedDate => "submittedDate",
        }
    }
}

#[derive(Debug, Clone)]
pub enum SearchField {
    Title,
    Author,
    Abstract,
    Comment,
    JournalReference,
    SubjectCategory,
    ReportNumber,
    Doi,
    All,
}

impl AsRef<str> for SearchField {
    fn as_ref(&self) -> &str {
        match self {
            SearchField::Title => "ti",
            SearchField::Author => "au",
            SearchField::Abstract => "abs",
            SearchField::Comment => "co",
            SearchField::JournalReference => "jr",
            SearchField::SubjectCategory => "cat",
            SearchField::ReportNumber => "rn",
            SearchField::Doi => "doi",
            SearchField::All => "all",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SearchRange {
    field: RangeField,
    start: OffsetDateTime,
    end: OffsetDateTime,
}

impl SearchRange {
    pub fn new(field: RangeField, start: OffsetDateTime, end: OffsetDateTime) -> Self {
        Self { field, start, end }
    }

    pub fn try_from_iso_8601(field: RangeField, start: &str, end: &str) -> anyhow::Result<Self> {
        Ok(Self {
            field,
            start: OffsetDateTime::parse(start, &Iso8601::DEFAULT)?,
            end: OffsetDateTime::parse(end, &Iso8601::DEFAULT)?,
        })
    }

    pub fn try_from_rfc_3339(field: RangeField, start: &str, end: &str) -> anyhow::Result<Self> {
        Ok(Self {
            field,
            start: OffsetDateTime::parse(start, &Rfc3339)?,
            end: OffsetDateTime::parse(end, &Rfc3339)?,
        })
    }

    pub fn try_from_rfc_2822(field: RangeField, start: &str, end: &str) -> anyhow::Result<Self> {
        Ok(Self {
            field,
            start: OffsetDateTime::parse(start, &Rfc2822)?,
            end: OffsetDateTime::parse(end, &Rfc2822)?,
        })
    }

    pub fn try_from_date(field: RangeField, start: &str, end: &str) -> anyhow::Result<Self> {
        Ok(Self {
            field,
            start: OffsetDateTime::parse(start, format_description!("[year]-[month]-[day]"))?,
            end: OffsetDateTime::parse(end, format_description!("[year]-[month]-[day]"))?,
        })
    }
}

impl ISearchQuery for SearchRange {
    fn to_query_string(&self) -> String {
        format!(
            "{}:[{} TO {}]",
            self.field.as_ref(),
            self.start
                .format(&Iso8601::DEFAULT)
                .expect("invalid start offset datetime"), // 1970-01-01T00:00:00Z
            self.end
                .format(&Iso8601::DEFAULT)
                .expect("invalid end offset datetime"), // 1970-01-01T00:16:40Z
        )
    }
}

impl Display for SearchRange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_query_string())
    }
}

#[derive(Debug, Clone)]
pub struct SearchTerm {
    field: SearchField,
    term: String,
}

impl SearchTerm {
    pub fn new<S: ToString>(field: SearchField, term: S) -> Self {
        Self {
            field,
            term: term.to_string(),
        }
    }
}

impl ISearchQuery for SearchTerm {
    fn to_query_string(&self) -> String {
        format!("{}:{}", self.field.as_ref(), self.term)
    }
}

impl Display for SearchTerm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_query_string())
    }
}

fn bracket_format(query: &str) -> String {
    format!("({})", query)
}

pub enum SearchPredicate<'a> {
    And(Vec<Box<dyn ISearchQuery + 'a>>),
    Or(Vec<Box<dyn ISearchQuery + 'a>>),
    AndNot(Box<dyn ISearchQuery + 'a>, Box<dyn ISearchQuery + 'a>),
    Bracket(Box<dyn ISearchQuery + 'a>),
}

impl<'a> SearchPredicate<'a> {
    pub fn and(lhs: impl ISearchQuery + 'a, rhs: impl ISearchQuery + 'a) -> Self {
        SearchPredicate::And(vec![Box::new(lhs), Box::new(rhs)])
    }

    pub fn and_all(predicates: Vec<Box<dyn ISearchQuery + 'a>>) -> Self {
        SearchPredicate::And(predicates)
    }

    pub fn or(lhs: impl ISearchQuery + 'a, rhs: impl ISearchQuery + 'a) -> Self {
        SearchPredicate::Or(vec![Box::new(lhs), Box::new(rhs)])
    }

    pub fn or_all(predicates: Vec<Box<dyn ISearchQuery + 'a>>) -> Self {
        SearchPredicate::Or(predicates)
    }

    pub fn and_not(lhs: impl ISearchQuery + 'a, rhs: impl ISearchQuery + 'a) -> Self {
        SearchPredicate::AndNot(Box::new(lhs), Box::new(rhs))
    }

    pub fn bracket(predicate: impl ISearchQuery + 'a) -> Self {
        SearchPredicate::Bracket(Box::new(predicate))
    }
}

impl<'a> ISearchQuery for SearchPredicate<'a> {
    fn to_query_string(&self) -> String {
        match self {
            SearchPredicate::And(predicates) => bracket_format(
                &predicates
                    .iter()
                    .map(|predicate| predicate.to_query_string())
                    .collect::<Vec<_>>()
                    .join(" AND "),
            ),
            SearchPredicate::Or(predicates) => bracket_format(
                &predicates
                    .iter()
                    .map(|predicate| predicate.to_query_string())
                    .collect::<Vec<_>>()
                    .join(" OR "),
            ),
            SearchPredicate::AndNot(lhs, rhs) => {
                format!(
                    "({} ANDNOT {})",
                    lhs.to_query_string(),
                    rhs.to_query_string()
                )
            }
            SearchPredicate::Bracket(inner) => {
                format!("({})", inner.to_query_string())
            }
        }
    }
}

impl Display for SearchPredicate<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", remove_outside_brackets(&self.to_query_string()))
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_search_term() {
        let term = SearchTerm::new(SearchField::Title, "RAG");
        assert_eq!(term.to_query_string(), "ti:RAG");
        assert_eq!(term.to_string(), "ti:RAG");
    }

    #[test]
    fn test_search_range() {
        let start = OffsetDateTime::from_unix_timestamp(0).unwrap();
        let end = OffsetDateTime::from_unix_timestamp(1000).unwrap();
        let range = SearchRange::new(RangeField::LastUpdatedDate, start, end);
        assert_eq!(
            range.to_query_string(),
            "lastUpdatedDate:[1970-01-01T00:00:00.000000000Z TO 1970-01-01T00:16:40.000000000Z]"
        );
        assert_eq!(
            range.to_string(),
            "lastUpdatedDate:[1970-01-01T00:00:00.000000000Z TO 1970-01-01T00:16:40.000000000Z]"
        );
    }

    #[test]
    fn test_simple_and_predicate() {
        let term1 = SearchTerm::new(SearchField::Title, "RAG");
        let term2 = SearchTerm::new(SearchField::Author, "John Doe");
        let predicate = SearchPredicate::and(term1, term2);
        assert_eq!(predicate.to_query_string(), "(ti:RAG AND au:John Doe)");
        assert_eq!(predicate.to_string(), "ti:RAG AND au:John Doe");
    }

    #[test]
    fn test_and_or_predicate() {
        let term1 = SearchTerm::new(SearchField::Title, "RAG");
        let term2 = SearchTerm::new(SearchField::Author, "John Doe");
        let term3 = SearchTerm::new(SearchField::Abstract, "Lorem Ipsum");
        let and_predicate = SearchPredicate::and(term1, term2);
        let or_predicate = SearchPredicate::or(and_predicate, term3);
        assert_eq!(
            or_predicate.to_query_string(),
            "((ti:RAG AND au:John Doe) OR abs:Lorem Ipsum)"
        );
        assert_eq!(
            or_predicate.to_string(),
            "(ti:RAG AND au:John Doe) OR abs:Lorem Ipsum"
        );
    }
}
