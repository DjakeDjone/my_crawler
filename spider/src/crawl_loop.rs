use std::{
    collections::{HashSet, VecDeque},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::{sync::Mutex, task::JoinHandle};
use url::Url;

use crate::{
    index::extract_page,
    qdrant::PageIndexer,
    robots::RobotsCache,
    sitemap,
    stats::CrawlStats,
    web_visitor::{
        normalize_url, origin, same_origin, FetchError, OriginScheduler, WebVisitorImpl,
    },
    web_visitor_browser::BrowserPool,
    CrawlRequest,
};

#[derive(Clone)]
struct QueuedUrl {
    url: Url,
    depth: usize,
}

pub struct CrawlLoop {
    requests: Arc<Mutex<VecDeque<CrawlRequest>>>,
    shutdown: Arc<AtomicBool>,
    stats: Arc<CrawlStats>,
    visitor: Arc<WebVisitorImpl>,
    robots: Arc<RobotsCache>,
    indexer: Arc<PageIndexer>,
    runners: Vec<JoinHandle<()>>,
}

impl CrawlLoop {
    pub fn new(
        stats: Arc<CrawlStats>,
        indexer: Arc<PageIndexer>,
        product_token: String,
        user_agent: String,
    ) -> Self {
        let visitor = Arc::new(WebVisitorImpl::new(&user_agent, OriginScheduler::default()));
        let robots = Arc::new(RobotsCache::new(visitor.clone(), product_token));
        Self {
            requests: Arc::new(Mutex::new(VecDeque::new())),
            shutdown: Arc::new(AtomicBool::new(true)),
            stats,
            visitor,
            robots,
            indexer,
            runners: Vec::new(),
        }
    }

    pub async fn add_crawl_request(&self, mut request: CrawlRequest) -> Result<(), String> {
        let seed = normalize_url(&request.url).ok_or_else(|| "invalid HTTP(S) URL".to_string())?;
        request.url = seed.to_string();
        self.requests.lock().await.push_back(request);
        Ok(())
    }

    pub async fn queue_size(&self) -> usize {
        self.requests.lock().await.len()
    }

    pub fn run(&mut self) {
        for id in 0..4 {
            let requests = self.requests.clone();
            let shutdown = self.shutdown.clone();
            let stats = self.stats.clone();
            let visitor = self.visitor.clone();
            let robots = self.robots.clone();
            let indexer = self.indexer.clone();
            self.runners.push(tokio::spawn(async move {
                while shutdown.load(Ordering::Relaxed) {
                    let request = requests.lock().await.pop_front();
                    if let Some(request) = request {
                        crawl_request(id, request, &visitor, &robots, &indexer, &stats).await;
                    } else {
                        tokio::time::sleep(Duration::from_millis(200)).await;
                    }
                }
            }));
        }
    }
}

impl Drop for CrawlLoop {
    fn drop(&mut self) {
        self.shutdown.store(false, Ordering::Relaxed);
        for runner in &self.runners {
            runner.abort();
        }
    }
}

async fn crawl_request(
    id: usize,
    request: CrawlRequest,
    visitor: &Arc<WebVisitorImpl>,
    robots: &Arc<RobotsCache>,
    indexer: &Arc<PageIndexer>,
    stats: &Arc<CrawlStats>,
) {
    let seed = normalize_url(&request.url).unwrap();
    let mut frontier = VecDeque::from([QueuedUrl {
        url: seed.clone(),
        depth: 0,
    }]);
    let mut queued = HashSet::from([seed.to_string()]);
    let mut visited = HashSet::new();
    let mut blocked_origins = HashSet::new();
    let mut pages = 0usize;
    let mut indexed = 0usize;
    let mut skipped_depth = 0usize;
    let mut skipped_robots = 0usize;
    let mut blocked = 0usize;
    let mut failed = 0usize;

    let seed_policy = robots.policy(&seed).await;
    if seed_policy.allowed {
        for url in sitemap::discover(
            visitor.clone(),
            &seed,
            seed_policy.sitemaps,
            request.max_pages,
        )
        .await
        {
            enqueue(&mut frontier, &mut queued, &visited, url, 1);
        }
    }

    while let Some(item) = frontier.pop_front() {
        queued.remove(item.url.as_str());
        if pages >= request.max_pages || item.depth > request.max_depth {
            if item.depth > request.max_depth {
                skipped_depth += 1;
                stats.inc_skipped_depth();
            }
            continue;
        }
        let url_key = item.url.to_string();
        if visited.contains(&url_key)
            || origin(&item.url).is_some_and(|value| blocked_origins.contains(&value))
        {
            continue;
        }
        visited.insert(url_key.clone());

        let policy = robots.policy(&item.url).await;
        if !policy.allowed {
            skipped_robots += 1;
            stats.inc_skipped_robots();
            continue;
        }

        let fetched = if request.use_browser {
            BrowserPool::fetch_page_with_options(
                item.url.as_str(),
                request.wait_for_selector.as_deref(),
                request.wait_timeout_ms,
            )
            .await
            .map(|html| (item.url.clone(), html))
            .map_err(|error| FetchError::Redirect(error.to_string()))
        } else {
            visitor.fetch_html(item.url.as_str()).await.map(|result| {
                (
                    result.final_url,
                    String::from_utf8_lossy(&result.body).into_owned(),
                )
            })
        };

        let (final_url, mut html) = match fetched {
            Ok(value) => value,
            Err(FetchError::Blocked(value)) => {
                blocked += 1;
                if let Ok(url) = Url::parse(&value) {
                    if let Some(origin) = origin(&url) {
                        blocked_origins.insert(origin);
                    }
                }
                continue;
            }
            Err(error) => {
                tracing::warn!("runner[{id}] failed {}: {error}", item.url);
                failed += 1;
                stats.inc_failed();
                continue;
            }
        };
        visited.insert(final_url.to_string());

        if !request.use_browser && needs_browser(&html) {
            if let Ok(browser_html) = BrowserPool::fetch_page_with_options(
                final_url.as_str(),
                request.wait_for_selector.as_deref(),
                request.wait_timeout_ms,
            )
            .await
            {
                if !browser_html.trim().is_empty() {
                    html = browser_html;
                }
            }
        }

        let extracted = extract_page(&final_url, &html);
        pages += 1;
        stats.inc_crawled();
        let has_chunks = !extracted.chunks.is_empty();
        if let Err(error) = indexer.index_page(&extracted.chunks).await {
            tracing::warn!("failed to index {final_url}: {error}");
            failed += 1;
            stats.inc_failed();
        } else if has_chunks {
            indexed += 1;
            stats.inc_indexed();
        }

        for link in extracted.links {
            if request.same_domain && !same_origin(&seed, &link) {
                continue;
            }
            if !is_crawl_trap(&link) {
                enqueue(&mut frontier, &mut queued, &visited, link, item.depth + 1);
            }
        }
    }
    tracing::info!(
        "runner[{id}] finished crawl seed={} crawled={} indexed={} visited={} skipped_robots={} skipped_depth={} blocked={} failed={} max_pages={} max_depth={} same_domain={}",
        seed,
        pages,
        indexed,
        visited.len(),
        skipped_robots,
        skipped_depth,
        blocked,
        failed,
        request.max_pages,
        request.max_depth,
        request.same_domain,
    );
}

fn enqueue(
    frontier: &mut VecDeque<QueuedUrl>,
    queued: &mut HashSet<String>,
    visited: &HashSet<String>,
    url: Url,
    depth: usize,
) {
    let key = url.to_string();
    if !visited.contains(&key) && queued.insert(key) {
        frontier.push_back(QueuedUrl { url, depth });
    }
}

fn needs_browser(html: &str) -> bool {
    if html.trim().is_empty() {
        return true;
    }
    let lower = html.to_ascii_lowercase();
    let has_app_root = lower.contains("id=\"app\"")
        || lower.contains("id=\"root\"")
        || lower.contains("__next_data__")
        || lower.contains("data-reactroot");
    has_app_root && !lower.contains("<p") && !lower.contains("<article")
}

fn is_crawl_trap(url: &Url) -> bool {
    let path = url.path().to_ascii_lowercase();
    if ["login", "logout", "signin", "signout", "search", "calendar"]
        .iter()
        .any(|part| path.split('/').any(|segment| segment == *part))
    {
        return true;
    }
    let pairs = url.query_pairs().collect::<Vec<_>>();
    pairs.len() > 5
        || pairs.iter().any(|(key, _)| {
            matches!(
                key.to_ascii_lowercase().as_str(),
                "filter" | "facet" | "sort" | "page" | "calendar"
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_common_crawl_traps() {
        assert!(is_crawl_trap(
            &Url::parse("https://example.com/search?q=x").unwrap()
        ));
        assert!(!is_crawl_trap(
            &Url::parse("https://example.com/article?id=1").unwrap()
        ));
    }
}
