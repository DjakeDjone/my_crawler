use robotstxt::{parse_robotstxt, DefaultMatcher, RobotsParseHandler};
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::RwLock;
use url::Url;

use crate::web_visitor::{origin, FetchResult, WebVisitorImpl};

const ROBOTS_MAX_BYTES: usize = 512 * 1024;
const CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);
const FAILURE_TTL: Duration = Duration::from_secs(5 * 60);

#[derive(Clone)]
struct CacheEntry {
    body: Option<String>,
    allow_all: bool,
    sitemaps: Vec<Url>,
    fetched_at: Instant,
    ttl: Duration,
}

impl CacheEntry {
    fn fresh(&self) -> bool {
        self.fetched_at.elapsed() < self.ttl
    }

    fn allowed(&self, product_token: &str, url: &Url) -> bool {
        match &self.body {
            Some(body) => DefaultMatcher::default().one_agent_allowed_by_robots(
                body,
                product_token,
                url.as_str(),
            ),
            None => self.allow_all,
        }
    }
}

pub struct RobotsPolicy {
    pub allowed: bool,
    pub sitemaps: Vec<Url>,
}

pub struct RobotsCache {
    cache: RwLock<HashMap<String, CacheEntry>>,
    visitor: Arc<WebVisitorImpl>,
    product_token: String,
}

impl RobotsCache {
    pub fn new(visitor: Arc<WebVisitorImpl>, product_token: String) -> Self {
        Self {
            cache: RwLock::new(HashMap::new()),
            visitor,
            product_token,
        }
    }

    pub async fn policy(&self, url: &Url) -> RobotsPolicy {
        let Some(key) = origin(url) else {
            return RobotsPolicy {
                allowed: false,
                sitemaps: Vec::new(),
            };
        };
        if let Some(entry) = self
            .cache
            .read()
            .await
            .get(&key)
            .filter(|entry| entry.fresh())
        {
            return RobotsPolicy {
                allowed: entry.allowed(&self.product_token, url),
                sitemaps: entry.sitemaps.clone(),
            };
        }

        let stale = self.cache.read().await.get(&key).cloned();
        let robots_url = format!("{key}/robots.txt");
        let entry = match self
            .visitor
            .fetch_resource(&robots_url, ROBOTS_MAX_BYTES, 5)
            .await
        {
            Ok(result) => Self::entry_from_response(result, stale),
            Err(_) => stale
                .filter(|entry| entry.body.is_some())
                .unwrap_or_else(Self::disallow),
        };
        let allowed = entry.allowed(&self.product_token, url);
        let sitemaps = entry.sitemaps.clone();
        self.cache.write().await.insert(key, entry);
        RobotsPolicy { allowed, sitemaps }
    }

    fn entry_from_response(result: FetchResult, stale: Option<CacheEntry>) -> CacheEntry {
        if result.status.is_success() {
            let body = String::from_utf8_lossy(&result.body).into_owned();
            let sitemaps = extract_sitemaps(&body);
            CacheEntry {
                body: Some(body),
                allow_all: false,
                sitemaps,
                fetched_at: Instant::now(),
                ttl: CACHE_TTL,
            }
        } else if result.status.is_client_error() {
            CacheEntry {
                body: None,
                allow_all: true,
                sitemaps: Vec::new(),
                fetched_at: Instant::now(),
                ttl: CACHE_TTL,
            }
        } else {
            stale
                .filter(|entry| entry.body.is_some())
                .unwrap_or_else(Self::disallow)
        }
    }

    fn disallow() -> CacheEntry {
        CacheEntry {
            body: None,
            allow_all: false,
            sitemaps: Vec::new(),
            fetched_at: Instant::now(),
            ttl: FAILURE_TTL,
        }
    }
}

#[derive(Default)]
struct SitemapCollector {
    values: Vec<Url>,
}

impl RobotsParseHandler for SitemapCollector {
    fn handle_robots_start(&mut self) {}
    fn handle_robots_end(&mut self) {}
    fn handle_user_agent(&mut self, _: u32, _: &str) {}
    fn handle_allow(&mut self, _: u32, _: &str) {}
    fn handle_disallow(&mut self, _: u32, _: &str) {}
    fn handle_sitemap(&mut self, _: u32, value: &str) {
        if let Ok(url) = Url::parse(value.trim()) {
            self.values.push(url);
        }
    }
    fn handle_unknown_action(&mut self, _: u32, _: &str, _: &str) {}
}

fn extract_sitemaps(body: &str) -> Vec<Url> {
    let mut collector = SitemapCollector::default();
    parse_robotstxt(body, &mut collector);
    collector.values
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_query_and_extracts_sitemaps() {
        let body = "User-agent: TestBot\nDisallow: /private?token=\nAllow: /\nSitemap: https://example.com/sitemap.xml\n";
        let entry = CacheEntry {
            body: Some(body.to_string()),
            allow_all: false,
            sitemaps: extract_sitemaps(body),
            fetched_at: Instant::now(),
            ttl: CACHE_TTL,
        };
        assert!(!entry.allowed(
            "TestBot",
            &Url::parse("https://example.com/private?token=x").unwrap()
        ));
        assert_eq!(entry.sitemaps.len(), 1);
    }

    #[test]
    fn cache_expiration_and_error_policy() {
        let expired = CacheEntry {
            fetched_at: Instant::now() - CACHE_TTL,
            ttl: CACHE_TTL,
            ..RobotsCache::disallow()
        };
        assert!(!expired.fresh());
        assert!(!RobotsCache::disallow()
            .allowed("TestBot", &Url::parse("https://example.com/").unwrap()));
    }
}
