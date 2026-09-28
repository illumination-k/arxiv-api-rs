use std::sync::{Arc, Mutex};
use std::time::Duration;

use reqwest::StatusCode;
use tokio::time::Instant;
use tracing::{debug, instrument, warn};

use crate::error::{ArxivError, Result};
use crate::models::{self, SearchResponse};
use crate::query::ArxivQuery;

const DEFAULT_API_BASE_URL: &str = "https://export.arxiv.org/api/query";
const DEFAULT_HTML_BASE_URL: &str = "https://arxiv.org/html/";
const DEFAULT_USER_AGENT: &str = concat!(
    env!("CARGO_PKG_NAME"),
    "/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/illumination-k/arxiv-api-rs)"
);
/// Upper bound for a server-provided `Retry-After` delay.
const MAX_RETRY_AFTER: Duration = Duration::from_secs(60);

/// Async client for the arXiv API and arXiv HTML pages.
///
/// Cloning is cheap and clones share the same connection pool and rate limiter,
/// so a single client (and its clones) never exceeds the configured request rate.
#[derive(Debug, Clone)]
pub struct ArxivClient {
    client: reqwest::Client,
    initial_interval: Duration,
    n_retries: usize,
    rate_limiter: RateLimiter,
    api_base_url: String,
    #[cfg_attr(not(feature = "html"), allow(dead_code))]
    html_base_url: String,
}

impl Default for ArxivClient {
    fn default() -> Self {
        Self::builder()
            .build()
            .expect("failed to build default HTTP client")
    }
}

impl ArxivClient {
    /// Create a new client. `interval` is the base delay before the first retry;
    /// subsequent retries use exponential backoff (interval * 2^(attempt-1)).
    ///
    /// Use [`ArxivClient::builder`] for more options.
    pub fn new(interval: Duration, n_retries: usize) -> Self {
        Self::builder()
            .initial_interval(interval)
            .n_retries(n_retries)
            .build()
            .expect("failed to build HTTP client")
    }

    pub fn builder() -> ArxivClientBuilder {
        ArxivClientBuilder::default()
    }

    #[instrument(skip(self, query), fields(n_retries = self.n_retries))]
    pub async fn search<S: ToString>(&self, query: ArxivQuery<S>) -> Result<SearchResponse> {
        let url = query.to_url(&self.api_base_url)?;
        debug!(url = %url, "Fetching from arXiv API");

        let response = self.get_with_retry(&url).await?;
        let status = response.status();
        if !status.is_success() {
            return Err(http_status_error(response).await);
        }

        let text = response.text().await.map_err(ArxivError::ResponseBody)?;
        let feed = quick_xml::de::from_str::<models::Feed>(&text).map_err(ArxivError::XmlParse)?;

        let response = SearchResponse::from_feed(feed);
        debug!(count = response.results.len(), "Search completed");
        Ok(response)
    }

    /// Fetch the HTML version of an arXiv paper and parse it into an [`ArxivPaper`](crate::ArxivPaper).
    ///
    /// The `arxiv_id` should be the paper identifier (e.g. `"2402.16893v1"`).
    /// Not all arXiv papers have an HTML version; this method returns
    /// [`ArxivError::HtmlNotAvailable`] when the HTML page is not found.
    #[cfg(feature = "html")]
    #[instrument(skip(self), fields(n_retries = self.n_retries))]
    pub async fn fetch_html(&self, arxiv_id: &str) -> Result<crate::html_parser::ArxivPaper> {
        let url = format!("{}{arxiv_id}", self.html_base_url);
        debug!(url = %url, "Fetching HTML from arXiv");

        let response = self.get_with_retry(&url).await?;
        let status = response.status();
        if status == StatusCode::NOT_FOUND {
            return Err(ArxivError::HtmlNotAvailable {
                arxiv_id: arxiv_id.to_string(),
            });
        }
        if !status.is_success() {
            return Err(http_status_error(response).await);
        }

        let text = response.text().await.map_err(ArxivError::ResponseBody)?;
        let paper = crate::html_parser::ArxivPaper::parse(&text);
        debug!(
            sections = paper.sections.len(),
            references = paper.references.len(),
            "HTML parsed"
        );
        Ok(paper)
    }

    /// Send a GET request, retrying on network errors and retryable statuses
    /// (429 and 5xx). The final response is returned as-is, whatever its status.
    async fn get_with_retry(&self, url: &str) -> Result<reqwest::Response> {
        let mut errors = vec![];

        for attempt in 1..=self.n_retries {
            let is_last = attempt == self.n_retries;
            self.rate_limiter.acquire().await;
            debug!(attempt, "Sending request");

            match self.client.get(url).send().await {
                Ok(response) => {
                    let status = response.status();
                    debug!(%status, "Received response");
                    if is_last || !is_retryable(status) {
                        return Ok(response);
                    }
                    let delay = retry_after(&response).unwrap_or_else(|| self.backoff(attempt));
                    warn!(attempt, %status, ?delay, "Retryable status; waiting before retry");
                    tokio::time::sleep(delay).await;
                }
                Err(e) => {
                    warn!(attempt, error = %e, "Request failed");
                    errors.push(e);
                    if !is_last {
                        let delay = self.backoff(attempt);
                        debug!(?delay, "Waiting before retry");
                        tokio::time::sleep(delay).await;
                    }
                }
            }
        }

        Err(ArxivError::RequestFailed {
            retries: self.n_retries,
            errors,
        })
    }

    fn backoff(&self, attempt: usize) -> Duration {
        self.initial_interval * 2u32.saturating_pow(attempt as u32 - 1)
    }
}

/// Builder for [`ArxivClient`].
#[derive(Debug, Clone)]
pub struct ArxivClientBuilder {
    user_agent: String,
    timeout: Option<Duration>,
    initial_interval: Duration,
    n_retries: usize,
    min_request_interval: Duration,
    api_base_url: String,
    html_base_url: String,
}

impl Default for ArxivClientBuilder {
    fn default() -> Self {
        Self {
            user_agent: DEFAULT_USER_AGENT.to_string(),
            timeout: Some(Duration::from_secs(30)),
            initial_interval: Duration::from_secs(3),
            n_retries: 3,
            // arXiv asks clients to make no more than one request every 3 seconds.
            min_request_interval: Duration::from_secs(3),
            api_base_url: DEFAULT_API_BASE_URL.to_string(),
            html_base_url: DEFAULT_HTML_BASE_URL.to_string(),
        }
    }
}

impl ArxivClientBuilder {
    /// `User-Agent` header sent with every request. arXiv recommends including contact information.
    pub fn user_agent(mut self, user_agent: impl Into<String>) -> Self {
        self.user_agent = user_agent.into();
        self
    }

    /// Total timeout for each request (default 30s). `None` disables the timeout.
    pub fn timeout(mut self, timeout: Option<Duration>) -> Self {
        self.timeout = timeout;
        self
    }

    /// Base delay before the first retry; doubles on each subsequent retry (default 3s).
    pub fn initial_interval(mut self, interval: Duration) -> Self {
        self.initial_interval = interval;
        self
    }

    /// Maximum number of attempts per request (default 3).
    pub fn n_retries(mut self, n_retries: usize) -> Self {
        self.n_retries = n_retries;
        self
    }

    /// Minimum delay between the start of consecutive requests, shared by all
    /// clones of the client (default 3s). `Duration::ZERO` disables rate limiting.
    pub fn min_request_interval(mut self, interval: Duration) -> Self {
        self.min_request_interval = interval;
        self
    }

    /// Override the arXiv API endpoint (default `https://export.arxiv.org/api/query`).
    pub fn api_base_url(mut self, url: impl Into<String>) -> Self {
        self.api_base_url = url.into();
        self
    }

    /// Override the arXiv HTML base URL (default `https://arxiv.org/html/`).
    pub fn html_base_url(mut self, url: impl Into<String>) -> Self {
        self.html_base_url = url.into();
        self
    }

    pub fn build(self) -> Result<ArxivClient> {
        let mut builder = reqwest::Client::builder().user_agent(self.user_agent);
        if let Some(timeout) = self.timeout {
            builder = builder.timeout(timeout);
        }
        let client = builder.build().map_err(ArxivError::ClientBuild)?;

        Ok(ArxivClient {
            client,
            initial_interval: self.initial_interval,
            n_retries: self.n_retries,
            rate_limiter: RateLimiter::new(self.min_request_interval),
            api_base_url: self.api_base_url,
            html_base_url: self.html_base_url,
        })
    }
}

/// Spaces out requests so that consecutive ones start at least `min_interval` apart.
#[derive(Debug, Clone)]
struct RateLimiter {
    min_interval: Duration,
    next_slot: Arc<Mutex<Option<Instant>>>,
}

impl RateLimiter {
    fn new(min_interval: Duration) -> Self {
        Self {
            min_interval,
            next_slot: Arc::new(Mutex::new(None)),
        }
    }

    async fn acquire(&self) {
        if self.min_interval.is_zero() {
            return;
        }
        // Reserve the next slot while holding the lock, then sleep without it.
        let slot = {
            let mut next = self.next_slot.lock().unwrap_or_else(|e| e.into_inner());
            let now = Instant::now();
            let slot = next.map_or(now, |n| n.max(now));
            *next = Some(slot + self.min_interval);
            slot
        };
        tokio::time::sleep_until(slot).await;
    }
}

fn is_retryable(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

/// Parse a `Retry-After` header given in seconds.
fn retry_after(response: &reqwest::Response) -> Option<Duration> {
    let secs = response
        .headers()
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()?;
    Some(Duration::from_secs(secs).min(MAX_RETRY_AFTER))
}

async fn http_status_error(response: reqwest::Response) -> ArxivError {
    let status = response.status();
    let body = response
        .text()
        .await
        .unwrap_or_else(|_| String::from("<failed to read body>"));
    ArxivError::HttpStatus { status, body }
}

#[cfg(test)]
mod test {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::*;

    struct MockServer {
        base_url: String,
        hits: Arc<AtomicUsize>,
        requests: Arc<Mutex<Vec<String>>>,
    }

    /// Serve canned raw HTTP responses in order; the last one repeats.
    async fn mock_server(responses: Vec<String>) -> MockServer {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url = format!("http://{}/", listener.local_addr().unwrap());
        let hits = Arc::new(AtomicUsize::new(0));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let (h, r) = (hits.clone(), requests.clone());

        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                let i = h.fetch_add(1, Ordering::SeqCst);
                let response = responses[i.min(responses.len() - 1)].clone();
                let r = r.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 8192];
                    let n = socket.read(&mut buf).await.unwrap_or(0);
                    r.lock()
                        .unwrap()
                        .push(String::from_utf8_lossy(&buf[..n]).into_owned());
                    let _ = socket.write_all(response.as_bytes()).await;
                    let _ = socket.shutdown().await;
                });
            }
        });

        MockServer {
            base_url,
            hits,
            requests,
        }
    }

    fn response(status: &str, extra_headers: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n{extra_headers}\r\n{body}",
            body.len()
        )
    }

    fn fixture(name: &str) -> String {
        std::fs::read_to_string(format!(
            "{}/tests/fixtures/{name}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap()
    }

    fn test_client(server: &MockServer) -> ArxivClient {
        ArxivClient::builder()
            .initial_interval(Duration::from_millis(10))
            .min_request_interval(Duration::ZERO)
            .api_base_url(&server.base_url)
            .html_base_url(&server.base_url)
            .build()
            .unwrap()
    }

    fn query() -> ArxivQuery<&'static str> {
        ArxivQuery::default().with_search_query("all:RAG")
    }

    #[tokio::test]
    async fn test_retries_on_503_then_succeeds() {
        let server = mock_server(vec![
            response("503 Service Unavailable", "Retry-After: 0\r\n", ""),
            response("200 OK", "", &fixture("search_by_id.xml")),
        ])
        .await;

        let resp = test_client(&server).search(query()).await.unwrap();
        assert_eq!(resp.results.len(), 1);
        assert_eq!(server.hits.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn test_returns_http_status_after_exhausting_retries_on_429() {
        let server = mock_server(vec![response("429 Too Many Requests", "", "slow down")]).await;

        let err = test_client(&server).search(query()).await.unwrap_err();
        assert_eq!(err.http_status(), Some(StatusCode::TOO_MANY_REQUESTS));
        assert_eq!(server.hits.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_does_not_retry_client_errors() {
        let server = mock_server(vec![response("400 Bad Request", "", "bad query")]).await;

        let err = test_client(&server).search(query()).await.unwrap_err();
        assert_eq!(err.http_status(), Some(StatusCode::BAD_REQUEST));
        assert_eq!(server.hits.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_sends_user_agent() {
        let server = mock_server(vec![response("200 OK", "", &fixture("search_by_id.xml"))]).await;

        test_client(&server).search(query()).await.unwrap();
        let requests = server.requests.lock().unwrap();
        let request = requests[0].to_ascii_lowercase();
        assert!(
            request.contains(&format!(
                "user-agent: {}",
                DEFAULT_USER_AGENT.to_ascii_lowercase()
            )),
            "request was: {request}"
        );
    }

    #[cfg(feature = "html")]
    #[tokio::test]
    async fn test_fetch_html_not_found() {
        let server = mock_server(vec![response("404 Not Found", "", "")]).await;

        let err = test_client(&server)
            .fetch_html("0000.00000")
            .await
            .unwrap_err();
        assert!(matches!(err, ArxivError::HtmlNotAvailable { .. }));
        assert_eq!(server.hits.load(Ordering::SeqCst), 1);
    }

    #[cfg(feature = "html")]
    #[tokio::test]
    async fn test_fetch_html_server_error_is_http_status() {
        let server = mock_server(vec![response("502 Bad Gateway", "", "")]).await;

        let err = test_client(&server)
            .fetch_html("0000.00000")
            .await
            .unwrap_err();
        assert_eq!(err.http_status(), Some(StatusCode::BAD_GATEWAY));
        assert_eq!(server.hits.load(Ordering::SeqCst), 3);
    }

    #[tokio::test(start_paused = true)]
    async fn test_rate_limiter_spaces_requests() {
        let limiter = RateLimiter::new(Duration::from_secs(3));
        let start = Instant::now();

        limiter.acquire().await;
        assert_eq!(start.elapsed(), Duration::ZERO);

        // Clones share the same schedule.
        let clone = limiter.clone();
        clone.acquire().await;
        assert_eq!(start.elapsed(), Duration::from_secs(3));

        limiter.acquire().await;
        assert_eq!(start.elapsed(), Duration::from_secs(6));
    }

    #[tokio::test(start_paused = true)]
    async fn test_rate_limiter_disabled() {
        let limiter = RateLimiter::new(Duration::ZERO);
        let start = Instant::now();
        limiter.acquire().await;
        limiter.acquire().await;
        assert_eq!(start.elapsed(), Duration::ZERO);
    }
}
