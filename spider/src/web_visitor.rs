use futures::StreamExt;
use governor::{
    clock::DefaultClock,
    state::{InMemoryState, NotKeyed},
    Quota, RateLimiter,
};
use reqwest::{
    header::{HeaderMap, CONTENT_LENGTH, CONTENT_TYPE, LOCATION, RETRY_AFTER},
    Client, StatusCode,
};
use scraper::{Html, Selector};
use std::{
    collections::HashMap,
    fmt,
    num::NonZeroU32,
    sync::Arc,
    time::{Duration, SystemTime},
};
use tokio::sync::{Mutex, OwnedMutexGuard};
use url::Url;

use crate::REQUEST_TIMEOUT_SECS;

const MAX_ATTEMPTS: usize = 3;
pub const MAX_HTML_BYTES: usize = 5 * 1024 * 1024;

type OriginLimiter = RateLimiter<NotKeyed, InMemoryState, DefaultClock>;

struct OriginState {
    active: Arc<Mutex<()>>,
    limiter: OriginLimiter,
}

#[derive(Clone, Default)]
pub struct OriginScheduler {
    origins: Arc<Mutex<HashMap<String, Arc<OriginState>>>>,
}

impl OriginScheduler {
    async fn acquire(&self, url: &Url) -> OwnedMutexGuard<()> {
        let origin = origin(url).expect("validated URL has an origin");
        let state = {
            let mut origins = self.origins.lock().await;
            origins
                .entry(origin)
                .or_insert_with(|| {
                    Arc::new(OriginState {
                        active: Arc::new(Mutex::new(())),
                        limiter: RateLimiter::direct(
                            Quota::with_period(Duration::from_secs(2))
                                .unwrap()
                                .allow_burst(NonZeroU32::new(1).unwrap()),
                        ),
                    })
                })
                .clone()
        };
        let guard = state.active.clone().lock_owned().await;
        state.limiter.until_ready().await;
        guard
    }
}

#[derive(Debug, Clone)]
pub struct FetchResult {
    pub final_url: Url,
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub content_type: Option<mime::Mime>,
    pub body: Vec<u8>,
}

#[derive(Debug)]
pub enum FetchError {
    InvalidUrl(String),
    Blocked(String),
    UnsupportedContentType(String),
    BodyTooLarge,
    Http(StatusCode),
    Request(reqwest::Error),
    Redirect(String),
}

impl fmt::Display for FetchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidUrl(url) => write!(f, "invalid URL: {url}"),
            Self::Blocked(url) => write!(f, "origin blocked crawling: {url}"),
            Self::UnsupportedContentType(value) => write!(f, "unsupported content type: {value}"),
            Self::BodyTooLarge => write!(f, "response body exceeds limit"),
            Self::Http(status) => write!(f, "HTTP {status}"),
            Self::Request(error) => error.fmt(f),
            Self::Redirect(value) => write!(f, "redirect error: {value}"),
        }
    }
}

impl std::error::Error for FetchError {}

#[derive(Clone)]
pub struct WebVisitorImpl {
    client: Client,
    scheduler: OriginScheduler,
}

impl WebVisitorImpl {
    pub fn new(user_agent: &str, scheduler: OriginScheduler) -> Self {
        let client = Client::builder()
            .user_agent(user_agent)
            .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
            .redirect(reqwest::redirect::Policy::none())
            .pool_idle_timeout(Duration::from_secs(90))
            .pool_max_idle_per_host(1)
            .build()
            .expect("failed to create HTTP client");
        Self { client, scheduler }
    }

    pub async fn fetch_html(&self, url: &str) -> Result<FetchResult, FetchError> {
        let result = self.fetch_resource(url, MAX_HTML_BYTES, 10).await?;
        if result.status == StatusCode::FORBIDDEN {
            return Err(FetchError::Blocked(result.final_url.to_string()));
        }
        if !result.status.is_success() {
            return Err(FetchError::Http(result.status));
        }
        let content_type = result
            .content_type
            .as_ref()
            .map(|value| value.essence_str())
            .unwrap_or("");
        if content_type != "text/html" && content_type != "application/xhtml+xml" {
            return Err(FetchError::UnsupportedContentType(content_type.to_string()));
        }
        Ok(result)
    }

    pub async fn fetch_resource(
        &self,
        url: &str,
        max_bytes: usize,
        max_redirects: usize,
    ) -> Result<FetchResult, FetchError> {
        let start = normalize_url(url).ok_or_else(|| FetchError::InvalidUrl(url.to_string()))?;
        let mut last_error = None;

        for attempt in 0..MAX_ATTEMPTS {
            match self
                .request_following_redirects(start.clone(), max_bytes, max_redirects)
                .await
            {
                Ok(result) if result.status == StatusCode::FORBIDDEN => return Ok(result),
                Ok(result)
                    if result.status == StatusCode::TOO_MANY_REQUESTS
                        || result.status == StatusCode::SERVICE_UNAVAILABLE =>
                {
                    if attempt + 1 == MAX_ATTEMPTS {
                        return Ok(result);
                    }
                    tokio::time::sleep(
                        retry_after(&result.headers).unwrap_or_else(|| backoff(attempt)),
                    )
                    .await;
                }
                Ok(result) if result.status.is_server_error() => {
                    if attempt + 1 == MAX_ATTEMPTS {
                        return Ok(result);
                    }
                    tokio::time::sleep(backoff(attempt)).await;
                }
                Ok(result) => return Ok(result),
                Err(error @ (FetchError::BodyTooLarge | FetchError::Redirect(_))) => {
                    return Err(error);
                }
                Err(error) => {
                    last_error = Some(error);
                    if attempt + 1 < MAX_ATTEMPTS {
                        tokio::time::sleep(backoff(attempt)).await;
                    }
                }
            }
        }
        Err(last_error.unwrap())
    }

    async fn request_following_redirects(
        &self,
        mut url: Url,
        max_bytes: usize,
        max_redirects: usize,
    ) -> Result<FetchResult, FetchError> {
        for redirects in 0..=max_redirects {
            let guard = self.scheduler.acquire(&url).await;
            let response = self
                .client
                .get(url.clone())
                .send()
                .await
                .map_err(FetchError::Request)?;
            let status = response.status();
            let headers = response.headers().clone();

            if status.is_redirection() {
                drop(guard);
                if redirects == max_redirects {
                    return Err(FetchError::Redirect("limit exceeded".to_string()));
                }
                let location = headers
                    .get(LOCATION)
                    .and_then(|value| value.to_str().ok())
                    .ok_or_else(|| FetchError::Redirect("missing Location".to_string()))?;
                url = url
                    .join(location)
                    .map_err(|_| FetchError::Redirect(location.to_string()))?;
                continue;
            }

            if headers
                .get(CONTENT_LENGTH)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<usize>().ok())
                .is_some_and(|length| length > max_bytes)
            {
                return Err(FetchError::BodyTooLarge);
            }

            let content_type = headers
                .get(CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse().ok());
            let mut body = Vec::new();
            let mut stream = response.bytes_stream();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(FetchError::Request)?;
                if body.len() + chunk.len() > max_bytes {
                    return Err(FetchError::BodyTooLarge);
                }
                body.extend_from_slice(&chunk);
            }
            drop(guard);
            return Ok(FetchResult {
                final_url: url,
                status,
                headers,
                content_type,
                body,
            });
        }
        unreachable!()
    }
}

pub fn retry_after(headers: &HeaderMap) -> Option<Duration> {
    let value = headers.get(RETRY_AFTER)?.to_str().ok()?.trim();
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }
    let date = httpdate::parse_http_date(value).ok()?;
    date.duration_since(SystemTime::now()).ok()
}

fn backoff(attempt: usize) -> Duration {
    let base = 1u64 << attempt;
    let jitter = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|value| value.subsec_millis() as u64 % 250)
        .unwrap_or(0);
    Duration::from_millis(base * 1000 + jitter)
}

pub fn origin(url: &Url) -> Option<String> {
    let host = url.host_str()?;
    let port = url
        .port()
        .map(|value| format!(":{value}"))
        .unwrap_or_default();
    Some(format!("{}://{}{}", url.scheme(), host, port))
}

pub fn same_origin(left: &Url, right: &Url) -> bool {
    left.scheme() == right.scheme()
        && left.host_str() == right.host_str()
        && left.port_or_known_default() == right.port_or_known_default()
}

pub fn normalize_url(value: &str) -> Option<Url> {
    let mut url = Url::parse(value).ok()?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return None;
    }
    url.set_fragment(None);
    if (url.scheme() == "http" && url.port() == Some(80))
        || (url.scheme() == "https" && url.port() == Some(443))
    {
        let _ = url.set_port(None);
    }
    let kept = url
        .query_pairs()
        .filter(|(key, _)| {
            let key = key.to_ascii_lowercase();
            !key.starts_with("utm_") && key != "gclid" && key != "fbclid"
        })
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();
    url.set_query(None);
    if !kept.is_empty() {
        url.query_pairs_mut().extend_pairs(kept);
    }
    Some(url)
}

pub fn extract_links(document: &Html, base_url: &Url) -> Vec<Url> {
    let selector = Selector::parse("a[href]").unwrap();
    document
        .select(&selector)
        .filter_map(|element| element.value().attr("href"))
        .filter(|href| {
            let href = href.trim();
            !href.is_empty()
                && !["javascript:", "mailto:", "tel:", "data:"]
                    .iter()
                    .any(|scheme| href.starts_with(scheme))
        })
        .filter_map(|href| base_url.join(href).ok())
        .filter_map(|url| normalize_url(url.as_str()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::HeaderValue;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn normalizes_tracking_fragments_and_default_ports() {
        assert_eq!(
            normalize_url("https://example.com:443/a?utm_source=x&keep=1#part")
                .unwrap()
                .as_str(),
            "https://example.com/a?keep=1"
        );
    }

    #[test]
    fn parses_retry_after_seconds_and_dates() {
        let mut headers = HeaderMap::new();
        headers.insert(RETRY_AFTER, HeaderValue::from_static("12"));
        assert_eq!(retry_after(&headers), Some(Duration::from_secs(12)));
        headers.insert(
            RETRY_AFTER,
            HeaderValue::from_str(&httpdate::fmt_http_date(
                SystemTime::now() + Duration::from_secs(30),
            ))
            .unwrap(),
        );
        assert!(retry_after(&headers).unwrap() <= Duration::from_secs(30));
    }

    #[tokio::test]
    async fn enforces_per_origin_concurrency_and_cooldown() {
        let scheduler = OriginScheduler::default();
        let url = Url::parse("https://example.com/").unwrap();
        let first = scheduler.acquire(&url).await;
        let scheduler_clone = scheduler.clone();
        let url_clone = url.clone();
        let second = tokio::spawn(async move { scheduler_clone.acquire(&url_clone).await });
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!second.is_finished());
        let started = std::time::Instant::now();
        drop(first);
        let _guard = second.await.unwrap();
        assert!(started.elapsed() >= Duration::from_millis(1_800));
    }

    #[tokio::test]
    async fn rejects_unsupported_content_and_large_bodies() {
        async fn server(response: &'static str) -> String {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            tokio::spawn(async move {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = [0; 1024];
                let _ = stream.read(&mut request).await;
                stream.write_all(response.as_bytes()).await.unwrap();
            });
            format!("http://{address}/")
        }

        let binary = server(
            "HTTP/1.1 200 OK\r\nContent-Type: application/pdf\r\nContent-Length: 1\r\n\r\nx",
        )
        .await;
        let visitor = WebVisitorImpl::new("TestBot/1.0", OriginScheduler::default());
        assert!(matches!(
            visitor.fetch_html(&binary).await,
            Err(FetchError::UnsupportedContentType(_))
        ));

        let large =
            server("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 6000000\r\n\r\n")
                .await;
        let visitor = WebVisitorImpl::new("TestBot/1.0", OriginScheduler::default());
        assert!(matches!(
            visitor.fetch_html(&large).await,
            Err(FetchError::BodyTooLarge)
        ));
    }
}
