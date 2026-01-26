use std::{
    collections::{HashMap, VecDeque},
    env,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

use governor::{Quota, RateLimiter, clock::DefaultClock, state::{InMemoryState, NotKeyed}};
use tokio::{
    sync::{Mutex, RwLock},
    task::JoinHandle,
    time::sleep,
};
use url::Url;

use weaviate_community::WeaviateClient;

use crate::{
    dedup::ContentDedup,
    robots::RobotsCache,
    stats::CrawlStats,
    web_visitor::{extract_links, get_base_url, normalize_url, WebVisitor, WebVisitorImpl},
    web_visitor_browser::WebVisitorBrowser,
    CrawlRequest,
};

/// URL entry in the crawl queue with metadata
#[derive(Clone, Debug)]
struct QueuedUrl {
    url: String,
    depth: usize,
    retry_count: usize,
}

pub struct CrawlLoopSettings {
    pub max_rps: u32,
    pub max_rps_per_base_url: u32,
    pub max_concurrent_requests: u32,
    pub max_retries: usize,
    pub retry_base_delay_ms: u64,
    pub browser_fallback_enabled: bool,
    pub browser_fallback_min_html_size: usize,
}

impl CrawlLoopSettings {
    pub fn from_env() -> Self {
        let max_concurrent = env::var("MAX_CONCURRENT_REQUESTS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(4);
        
        let max_retries = env::var("MAX_RETRIES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(3);
        
        let retry_base_delay_ms = env::var("RETRY_BASE_DELAY_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1000);

        CrawlLoopSettings {
            max_rps: 100,
            max_rps_per_base_url: 10,
            max_concurrent_requests: max_concurrent,
            max_retries,
            retry_base_delay_ms,
            browser_fallback_enabled: true,
            browser_fallback_min_html_size: 1024,
        }
    }
}

impl Default for CrawlLoopSettings {
    fn default() -> Self {
        Self::from_env()
    }
}

type DomainRateLimiter = RateLimiter<NotKeyed, InMemoryState, DefaultClock>;

struct CrawlRunner {
    id: usize,
    crawl_requests: Arc<RwLock<HashMap<String, CrawlRequest>>>,
    settings: Arc<RwLock<CrawlLoopSettings>>,
    rate_limiters: Arc<Mutex<HashMap<String, Arc<DomainRateLimiter>>>>,
    shutdown: Arc<AtomicBool>,
    weaviate_client: Option<Arc<WeaviateClient>>,
    robots_cache: Arc<RobotsCache>,
    dedup: Arc<ContentDedup>,
    stats: Arc<CrawlStats>,
    handle: Option<JoinHandle<()>>,
}

impl CrawlRunner {
    #[allow(clippy::too_many_arguments)]
    fn new(
        id: usize,
        crawl_requests: Arc<RwLock<HashMap<String, CrawlRequest>>>,
        settings: Arc<RwLock<CrawlLoopSettings>>,
        rate_limiters: Arc<Mutex<HashMap<String, Arc<DomainRateLimiter>>>>,
        shutdown: Arc<AtomicBool>,
        weaviate_client: Option<Arc<WeaviateClient>>,
        robots_cache: Arc<RobotsCache>,
        dedup: Arc<ContentDedup>,
        stats: Arc<CrawlStats>,
    ) -> Self {
        CrawlRunner {
            id,
            crawl_requests,
            settings,
            rate_limiters,
            shutdown,
            weaviate_client,
            robots_cache,
            dedup,
            stats,
            handle: None,
        }
    }

    /// Get or create a rate limiter for a domain
    async fn get_rate_limiter(
        rate_limiters: &Arc<Mutex<HashMap<String, Arc<DomainRateLimiter>>>>,
        domain: &str,
        rps: u32,
    ) -> Arc<DomainRateLimiter> {
        let mut limiters = rate_limiters.lock().await;
        if let Some(limiter) = limiters.get(domain) {
            return limiter.clone();
        }

        // Use NonZeroU32::new with fallback to 1 for safety
        let rps_nonzero = std::num::NonZeroU32::new(rps.max(1)).unwrap();
        let quota = Quota::per_second(rps_nonzero);
        let limiter = Arc::new(RateLimiter::direct(quota));
        limiters.insert(domain.to_string(), limiter.clone());
        limiter
    }

    fn start(&mut self) {
        if self.handle.is_some() {
            return;
        }

        let id = self.id;
        let crawl_requests = self.crawl_requests.clone();
        let settings = self.settings.clone();
        let rate_limiters = self.rate_limiters.clone();
        let shutdown = self.shutdown.clone();
        let maybe_client = self.weaviate_client.clone();
        let robots_cache = self.robots_cache.clone();
        let dedup = self.dedup.clone();
        let stats = self.stats.clone();

        let handle = tokio::spawn(async move {
            let web_visitor = WebVisitorImpl::new();
            let weaviate_client = maybe_client;

            loop {
                if !shutdown.load(Ordering::SeqCst) {
                    tracing::info!("runner[{}] shutting down", id);
                    break;
                }

                let maybe_request = {
                    let mut map = crawl_requests.write().await;
                    if let Some(key) = map.keys().next().cloned() {
                        map.remove(&key)
                    } else {
                        None
                    }
                };

                let crawl_request = match maybe_request {
                    Some(r) => r,
                    None => {
                        sleep(Duration::from_millis(250)).await;
                        continue;
                    }
                };

                tracing::info!("runner[{}] starting crawl {}", id, crawl_request.url);

                let start_base = get_base_url(&crawl_request.url);
                let max_depth = crawl_request.max_depth;
                
                let mut to_visit: VecDeque<QueuedUrl> = VecDeque::new();
                let mut visited: HashMap<String, ()> = HashMap::new();
                
                to_visit.push_back(QueuedUrl {
                    url: normalize_url(&crawl_request.url),
                    depth: 0,
                    retry_count: 0,
                });
                
                let mut pages_crawled = 0usize;

                // Get settings once
                let (max_retries, retry_base_delay_ms, max_rps_per_base) = {
                    let s = settings.read().await;
                    (s.max_retries, s.retry_base_delay_ms, s.max_rps_per_base_url)
                };

                while shutdown.load(Ordering::SeqCst)
                    && pages_crawled < crawl_request.max_pages
                    && !to_visit.is_empty()
                {
                    let queued = to_visit.pop_front().unwrap();
                    let url = queued.url.clone();
                    let depth = queued.depth;
                    let retry_count = queued.retry_count;

                    if visited.contains_key(&url) {
                        continue;
                    }

                    // Check depth limit
                    if depth > max_depth {
                        tracing::debug!(
                            "runner[{}] skipping {} (depth {} > max {})",
                            id, url, depth, max_depth
                        );
                        stats.inc_skipped_depth();
                        visited.insert(url.clone(), ());
                        continue;
                    }

                    // Check robots.txt
                    if !robots_cache.is_allowed(&url).await {
                        tracing::info!(
                            "runner[{}] skipping {} (disallowed by robots.txt)",
                            id, url
                        );
                        stats.inc_skipped_robots();
                        visited.insert(url.clone(), ());
                        continue;
                    }

                    // Rate limiting using governor
                    let limiter = Self::get_rate_limiter(
                        &rate_limiters,
                        &start_base,
                        max_rps_per_base,
                    ).await;
                    limiter.until_ready().await;

                    // Fetch page
                    let fetch_result: Result<String, anyhow::Error> = if crawl_request.use_browser {
                        tracing::debug!("runner[{}] using browser mode for {}", id, url);
                        WebVisitorBrowser::new()
                            .fetch_page_with_options(
                                &url,
                                crawl_request.wait_for_selector.as_deref(),
                                crawl_request.wait_timeout_ms,
                            )
                            .await
                    } else {
                        web_visitor.fetch_page(&url).await
                    };

                    match fetch_result {
                        Ok(mut html) => {
                            // Browser fallback heuristics (if not in explicit browser mode)
                            if !crawl_request.use_browser {
                                let settings_read = settings.read().await;
                                let try_browser = settings_read.browser_fallback_enabled
                                    && (html.trim().is_empty()
                                        || html.len() < settings_read.browser_fallback_min_html_size
                                        || Self::looks_like_spa(&html));
                                drop(settings_read);

                                if try_browser {
                                    tracing::debug!(
                                        "runner[{}] browser fallback for {}",
                                        id, url
                                    );
                                    if let Ok(browser_html) = WebVisitorBrowser::new()
                                        .fetch_page_with_options(
                                            &url,
                                            crawl_request.wait_for_selector.as_deref(),
                                            crawl_request.wait_timeout_ms,
                                        )
                                        .await
                                    {
                                        if !browser_html.trim().is_empty() {
                                            html = browser_html;
                                        }
                                    }
                                }
                            }

                            if html.trim().is_empty() {
                                tracing::warn!("runner[{}] empty content for {}", id, url);
                                pages_crawled += 1;
                                stats.inc_failed();
                                visited.insert(url.clone(), ());
                                continue;
                            }

                            // Check for duplicate content
                            if dedup.is_duplicate(&html).await {
                                tracing::info!(
                                    "runner[{}] skipping {} (duplicate content)",
                                    id, url
                                );
                                stats.inc_skipped_dedup();
                                visited.insert(url.clone(), ());
                                // Still extract links from duplicates
                            } else {
                                // Index the page
                                tracing::info!("runner[{}] fetched {}", id, url);
                                pages_crawled += 1;
                                stats.inc_crawled();
                                visited.insert(url.clone(), ());

                                if let Some(client_arc) = weaviate_client.as_ref() {
                                    crate::weaviate::index_page_safe_with_client(
                                        client_arc,
                                        url.clone(),
                                        html.clone(),
                                    )
                                    .await;
                                }
                            }

                            // Extract links (always, even from duplicates)
                            if let Ok(base_url_parsed) = Url::parse(&url) {
                                let links = extract_links(&html, &base_url_parsed);

                                for link in links.into_iter() {
                                    if crawl_request.same_domain
                                        && get_base_url(&link) != start_base
                                    {
                                        continue;
                                    }

                                    let normalized = normalize_url(&link);
                                    if !visited.contains_key(&normalized) {
                                        let already_queued = to_visit.iter()
                                            .any(|q| q.url == normalized);
                                        if !already_queued {
                                            to_visit.push_back(QueuedUrl {
                                                url: normalized,
                                                depth: depth + 1,
                                                retry_count: 0,
                                            });
                                        }
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            tracing::warn!(
                                "runner[{}] failed fetching {} (attempt {}): {:?}",
                                id, url, retry_count + 1, e
                            );

                            // Retry with exponential backoff
                            if retry_count < max_retries {
                                let delay = retry_base_delay_ms * (1 << retry_count);
                                tracing::info!(
                                    "runner[{}] will retry {} in {}ms",
                                    id, url, delay
                                );
                                stats.inc_retries();
                                
                                // Re-queue with incremented retry count
                                to_visit.push_back(QueuedUrl {
                                    url: url.clone(),
                                    depth,
                                    retry_count: retry_count + 1,
                                });
                                
                                sleep(Duration::from_millis(delay)).await;
                            } else {
                                tracing::warn!(
                                    "runner[{}] giving up on {} after {} retries",
                                    id, url, max_retries
                                );
                                stats.inc_failed();
                                visited.insert(url.clone(), ());
                            }
                        }
                    }

                    // Small yield
                    sleep(Duration::from_millis(5)).await;
                }

                tracing::info!(
                    "runner[{}] finished crawl {} (pages {})",
                    id, crawl_request.url, pages_crawled
                );
            }
        });

        self.handle = Some(handle);
    }

    /// Check if HTML looks like a single-page app that needs browser rendering
    fn looks_like_spa(html: &str) -> bool {
        let html_lower = html.to_lowercase();
        html_lower.contains("<noscript")
            || html_lower.contains("id=\"app\"")
            || html_lower.contains("id=\"root\"")
            || html_lower.contains("data-reactroot")
            || html_lower.contains("__next_data__")
            || html_lower.contains("window.__initial_state__")
    }

    fn stop(&mut self) {
        self.shutdown.store(false, Ordering::SeqCst);
        if let Some(h) = self.handle.take() {
            h.abort();
        }
    }
}

pub struct CrawlLoop {
    crawl_requests: Arc<RwLock<HashMap<String, CrawlRequest>>>,
    settings: Arc<RwLock<CrawlLoopSettings>>,
    rate_limiters: Arc<Mutex<HashMap<String, Arc<DomainRateLimiter>>>>,
    runners: Vec<CrawlRunner>,
    shutdown: Arc<AtomicBool>,
    weaviate_client: Option<Arc<WeaviateClient>>,
    robots_cache: Arc<RobotsCache>,
    dedup: Arc<ContentDedup>,
    stats: Arc<CrawlStats>,
    queue_size: Arc<AtomicUsize>,
}

impl CrawlLoop {
    pub fn new(stats: Arc<CrawlStats>) -> Self {
        CrawlLoop {
            crawl_requests: Arc::new(RwLock::new(HashMap::new())),
            settings: Arc::new(RwLock::new(CrawlLoopSettings::default())),
            rate_limiters: Arc::new(Mutex::new(HashMap::new())),
            runners: Vec::new(),
            shutdown: Arc::new(AtomicBool::new(true)),
            weaviate_client: None,
            robots_cache: Arc::new(RobotsCache::new()),
            dedup: Arc::new(ContentDedup::new()),
            stats,
            queue_size: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn set_weaviate_client(&mut self, client: Arc<WeaviateClient>) {
        self.weaviate_client = Some(client);
    }

    pub async fn add_crawl_request(&mut self, crawl_request: CrawlRequest) {
        let base_url = get_base_url(&crawl_request.url);
        self.crawl_requests
            .write()
            .await
            .insert(base_url, crawl_request);
        self.queue_size.fetch_add(1, Ordering::Relaxed);
    }

    pub async fn queue_size(&self) -> usize {
        self.crawl_requests.read().await.len()
    }

    pub async fn run(&mut self) {
        let settings = self.settings.read().await;
        let max_concurrent = settings.max_concurrent_requests as usize;
        drop(settings);

        tracing::info!("Starting {} concurrent crawl runners", max_concurrent);

        for i in 0..max_concurrent {
            let mut runner = CrawlRunner::new(
                i,
                self.crawl_requests.clone(),
                self.settings.clone(),
                self.rate_limiters.clone(),
                self.shutdown.clone(),
                self.weaviate_client.clone(),
                self.robots_cache.clone(),
                self.dedup.clone(),
                self.stats.clone(),
            );
            runner.start();
            self.runners.push(runner);
        }

        tracing::info!("CrawlLoop started {} runners", self.runners.len());
    }

    pub fn stop(&mut self) {
        self.shutdown.store(false, Ordering::SeqCst);
        for runner in &mut self.runners {
            runner.stop();
        }
        self.runners.clear();
    }
}

impl Drop for CrawlLoop {
    fn drop(&mut self) {
        self.shutdown.store(false, Ordering::SeqCst);
        for runner in &mut self.runners {
            runner.stop();
        }
    }
}
