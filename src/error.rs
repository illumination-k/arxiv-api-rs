/// Errors that can occur when using the arXiv API client.
#[derive(Debug, thiserror::Error)]
pub enum ArxivError {
    /// All retry attempts for an HTTP request were exhausted.
    #[error("HTTP request failed after {retries} retries")]
    RequestFailed {
        retries: usize,
        errors: Vec<reqwest::Error>,
    },

    /// The arXiv API returned a non-success HTTP status code.
    #[error("HTTP error: status {status}")]
    HttpStatus {
        status: reqwest::StatusCode,
        body: String,
    },

    /// Failed to read the HTTP response body.
    #[error("failed to read response body")]
    ResponseBody(#[source] reqwest::Error),

    /// Failed to deserialize the XML response from arXiv.
    #[error("failed to parse XML response")]
    XmlParse(#[source] quick_xml::DeError),

    /// Failed to construct the query URL.
    #[error("failed to build query URL: {params}")]
    UrlParse {
        #[source]
        source: url::ParseError,
        params: String,
    },

    /// Failed to parse a date/time string.
    #[error("failed to parse date/time: {input}")]
    DateTimeParse {
        #[source]
        source: time::error::Parse,
        input: String,
    },

    /// The arXiv HTML page was not available for the given paper.
    #[error("HTML version not available for paper: {arxiv_id}")]
    HtmlNotAvailable { arxiv_id: String },
}

impl ArxivError {
    /// Returns `true` if this error is due to exhausted retries.
    pub fn is_request_failed(&self) -> bool {
        matches!(self, ArxivError::RequestFailed { .. })
    }

    /// Returns `true` if this error is due to an HTTP status error.
    pub fn is_http_status(&self) -> bool {
        matches!(self, ArxivError::HttpStatus { .. })
    }

    /// Returns the HTTP status code if this is an HTTP status error.
    pub fn http_status(&self) -> Option<reqwest::StatusCode> {
        match self {
            ArxivError::HttpStatus { status, .. } => Some(*status),
            _ => None,
        }
    }
}

pub type Result<T> = std::result::Result<T, ArxivError>;
