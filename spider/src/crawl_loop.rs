use std::{
    collections::{HashMap, VecDeque},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use tokio::{
    sync::{Mutex, RwLock},
    task::JoinHandle,
    time::sleep,
};
use url::Url;

use weaviate_community::WeaviateClient;

use crate::{
    web_visitor::{extract_links, get_base_url, normalize_url, WebVisitor, WebVisitorImpl},
    web_visitor_browser::WebVisitorBrowser,
    CrawlRequest,
};

pub struct CrawlLoopSettings {
    pub max_rps: u32, // global (not enforced in this simple runner)
    pub max_rps_per_base_url: u32,
    pub max_concurrent_requests: u32,

    // Browser fallback configuration:
    // If enabled, the crawler will attempt a browser-backed fetch when the
    // HTTP client returns empty or the page matches lightweight heuristics
    // that commonly indicate a JS-rendered site.
    pub browser_fallback_enabled: bool,
    // Minimum HTML size (bytes) below which we'll consider falling back to the browser.
    pub browser_fallback_min_html_size: usize,
}

impl CrawlLoopSettings {
    pub fn new(
        max_rps: u32,
        max_rps_per_base_url: u32,
        max_concurrent_requests: u32,
        browser_fallback_enabled: bool,
        browser_fallback_min_html_size: usize,
    ) -> Self {
        CrawlLoopSettings {
            max_rps,
            max_rps_per_base_url,
            max_concurrent_requests,
            browser_fallback_enabled,
            browser_fallback_min_html_size,
        }
    }

    pub fn default() -> Self {
        CrawlLoopSettings {
            max_rps: 100,
            max_rps_per_base_url: 10,
            max_concurrent_requests: 4,
            // enable browser fallback by default; toggle via code or environment if needed
            browser_fallback_enabled: true,
            // pages with HTML under this many bytes will be considered for a browser fetch
            browser_fallback_min_html_size: 1024,
        }
    }
}

/// A CrawlRunner that runs in the background and processes CrawlRequests.
/// It respects a simple per-base-url rate limit by consulting a shared
/// map of last-request timestamps protected by a mutex. It optionally holds
/// a Weaviate client (shared via Arc) used to index fetched pages.
struct CrawlRunner {
    id: usize,
    crawl_requests: Arc<RwLock<HashMap<String, CrawlRequest>>>, // base_url -> req
    settings: Arc<RwLock<CrawlLoopSettings>>,
    last_request_times: Arc<Mutex<HashMap<String, Instant>>>,
    shutdown: Arc<AtomicBool>,
    weaviate_client: Option<Arc<WeaviateClient>>,
    handle: Option<JoinHandle<()>>,
}

impl CrawlRunner {
    fn new(
        id: usize,
        crawl_requests: Arc<RwLock<HashMap<String, CrawlRequest>>>,
        settings: Arc<RwLock<CrawlLoopSettings>>,
        last_request_times: Arc<Mutex<HashMap<String, Instant>>>,
        shutdown: Arc<AtomicBool>,
        weaviate_client: Option<Arc<WeaviateClient>>,
    ) -> Self {
        CrawlRunner {
            id,
            crawl_requests,
            settings,
            last_request_times,
            shutdown,
            weaviate_client,
            handle: None,
        }
    }

    /// Spawn the runner background task. The runner will continuously take
    /// available crawl requests (one at a time) and process them until
    /// `shutdown` is set to false or there are no requests.
    fn start(&mut self) {
        if self.handle.is_some() {
            // already started
            return;
        }

        let id = self.id;
        let crawl_requests = self.crawl_requests.clone();
        let settings = self.settings.clone();
        let last_request_times = self.last_request_times.clone();
        let shutdown = self.shutdown.clone();
        let maybe_client = self.weaviate_client.clone();

        // Spawn the background task.
        let handle = tokio::spawn(async move {
            let web_visitor = WebVisitorImpl::new();
            // move the optional client into the async block
            let weaviate_client = maybe_client;

            loop {
                if !shutdown.load(Ordering::SeqCst) {
                    // shutdown requested
                    tracing::info!("runner[{}] shutting down", id);
                    break;
                }

                // Try to pick one crawl request (remove it from the map so other
                // runners won't pick it concurrently).
                let maybe_request = {
                    // Acquire a write lock, pick the first key (if any), remove it and return the removed request.
                    // Using `keys().next().cloned()` avoids cloning the whole value just to discover a key.
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
                        // nothing to do right now
                        sleep(Duration::from_millis(250)).await;
                        continue;
                    }
                };

                tracing::info!("runner[{}] starting crawl {}", id, crawl_request.url);

                // Perform BFS-like crawl up to max_pages, constrained to the same base_url.
                let start_base = get_base_url(&crawl_request.url);
                let mut to_visit: VecDeque<String> = VecDeque::new();
                let mut visited: HashMap<String, ()> = HashMap::new();
                to_visit.push_back(normalize_url(&crawl_request.url));
                let mut pages_crawled = 0usize;

                while shutdown.load(Ordering::SeqCst)
                    && pages_crawled < crawl_request.max_pages
                    && !to_visit.is_empty()
                {
                    let url = to_visit.pop_front().unwrap();
                    if visited.contains_key(&url) {
                        continue;
                    }

                    // Enforce simple per-base-url rate limiting
                    {
                        let settings_read = settings.read().await;
                        let max_per_base = settings_read.max_rps_per_base_url;
                        drop(settings_read);

                        if max_per_base > 0 {
                            let min_interval_secs = 1.0 / (max_per_base as f64);
                            let min_interval = Duration::from_secs_f64(min_interval_secs.max(0.0));

                            let mut lrt = last_request_times.lock().await;
                            let now = Instant::now();
                            if let Some(last) = lrt.get(&start_base) {
                                if now.duration_since(*last) < min_interval {
                                    let wait = min_interval - now.duration_since(*last);
                                    tracing::debug!(
                                        "runner[{}] sleeping {:?} to respect rate limit for {}",
                                        id,
                                        wait,
                                        start_base
                                    );
                                    // release lock while sleeping
                                    drop(lrt);
                                    sleep(wait).await;
                                    // after sleeping re-acquire and update last time below
                                    lrt = last_request_times.lock().await;
                                }
                            }

                            lrt.insert(start_base.clone(), Instant::now());
                        }
                    }

                    // Fetch the page (try HTTP client first; if it returns empty content or heuristics match,
                    // fall back to a browser-backed fetch when enabled).
                    match web_visitor.fetch_page(&url).await {
                        Ok(mut html) => {
                            // Decide whether to attempt a browser-backed fetch based on settings
                            let mut try_browser = false;
                            {
                                let settings_read = settings.read().await;
                                if settings_read.browser_fallback_enabled {
                                    // Empty body -> definitely consider browser fetch
                                    if html.trim().is_empty() {
                                        try_browser = true;
                                    } else {
                                        // Heuristic: if HTML is very small, or contains markers of client-side rendered apps,
                                        // prefer a browser fetch to capture hydrated content.
                                        if html.len() < settings_read.browser_fallback_min_html_size
                                        {
                                            try_browser = true;
                                        } else {
                                            let html_lower = html.to_lowercase();
                                            // Common markers for client-side frameworks / JS apps
                                            if html_lower.contains("<noscript")
                                                || html_lower.contains("id=\"app\"")
                                                || html_lower.contains("id=\"root\"")
                                                || html_lower.contains("data-reactroot")
                                                || html_lower.contains("__next_data__")
                                                || html_lower.contains("window.__initial_state__")
                                                || html_lower.contains(
                                                    "window.__NEXT_DATA__".to_lowercase().as_str(),
                                                )
                                            {
                                                try_browser = true;
                                            }
                                        }
                                    }
                                }
                            }

                            if try_browser {
                                tracing::debug!(
                                    "runner[{}] heuristic/browser fallback triggered for {}",
                                    id,
                                    url
                                );

                                match WebVisitorBrowser::new().fetch_page(&url).await {
                                    Ok(browser_html) => {
                                        if !browser_html.trim().is_empty() {
                                            tracing::info!(
                                                "runner[{}] browser fetched content for {}",
                                                id,
                                                url
                                            );
                                            html = browser_html;
                                        } else {
                                            tracing::warn!(
                                                "runner[{}] browser returned empty content for {}",
                                                id,
                                                url
                                            );
                                        }
                                    }
                                    Err(e) => {
                                        tracing::warn!(
                                            "runner[{}] browser fetch failed for {}: {:?}",
                                            id,
                                            url,
                                            e
                                        );
                                    }
                                }
                            }

                            if html.trim().is_empty() {
                                tracing::warn!("runner[{}] fetched empty content for {}", id, url);
                                // Mark visited to avoid retry storms and count the page as attempted.
                                pages_crawled += 1;
                                visited.insert(url.clone(), ());
                                // Skip parsing links for empty content and continue with next URL.
                                continue;
                            }

                            tracing::info!("runner[{}] fetched {}", id, url);
                            pages_crawled += 1;
                            visited.insert(url.clone(), ());

                            // If a Weaviate client was provided, index the page asynchronously and safely.
                            if let Some(client_arc) = weaviate_client.as_ref() {
                                // index_page_safe_with_client expects &WeaviateClient
                                let client_ref: &WeaviateClient = &*client_arc;
                                // Call the safe indexing helper (it will log on failure).
                                // We await here to ensure ordering per page; if you'd prefer
                                // indexing to happen fully in the background, spawn a task.
                                crate::weaviate::index_page_safe_with_client(
                                    client_ref,
                                    url.clone(),
                                    html.clone(),
                                )
                                .await
                            } else {
                                tracing::error!("Failed to index page");
                            }

                            // Try to parse page to find links
                            if let Ok(base_url_parsed) = Url::parse(&url) {
                                let links = extract_links(&html, &base_url_parsed);

                                for link in links.into_iter() {
                                    // If the crawl request asked to stay on the same domain, enforce that.
                                    // Otherwise follow any valid links found.
                                    if crawl_request.same_domain {
                                        // Only follow links on same host (same base URL)
                                        if get_base_url(&link) != start_base {
                                            continue;
                                        }
                                    }

                                    let normalized = normalize_url(&link);
                                    if !visited.contains_key(&normalized)
                                        && !to_visit.contains(&normalized)
                                    {
                                        to_visit.push_back(normalized);
                                    }
                                }
                            } else {
                                tracing::debug!("runner[{}] couldn't parse URL {}", id, url);
                            }
                        }
                        Err(e) => {
                            tracing::warn!("runner[{}] failed fetching {}: {:?}", id, url, e);
                            // Mark visited to avoid retry storm
                            visited.insert(url.clone(), ());
                        }
                    }

                    // small yield to give other tasks some time
                    sleep(Duration::from_millis(5)).await;
                } // end page crawl loop

                tracing::info!(
                    "runner[{}] finished crawl {} (pages {})",
                    id,
                    crawl_request.url,
                    pages_crawled
                );

                // After finishing a crawl we loop back to pick another request.
            } // end main runner loop
        });

        self.handle = Some(handle);
    }

    /// Stop the background task if it's running. This only signals shutdown;
    /// the task will exit on its next check. We also abort the task to avoid
    /// hanging on Drop.
    fn stop(&mut self) {
        self.shutdown.store(false, Ordering::SeqCst);
        if let Some(h) = self.handle.take() {
            h.abort();
        }
    }
}

/// The CrawlLoop orchestrates crawl requests and runners.
pub struct CrawlLoop {
    crawl_requests: Arc<RwLock<HashMap<String, CrawlRequest>>>,
    settings: Arc<RwLock<CrawlLoopSettings>>,
    last_request_times: Arc<Mutex<HashMap<String, Instant>>>,
    runners: Vec<CrawlRunner>,
    shutdown: Arc<AtomicBool>,
    weaviate_client: Option<Arc<WeaviateClient>>,
}

impl CrawlLoop {
    pub fn new() -> Self {
        CrawlLoop {
            crawl_requests: Arc::new(RwLock::new(HashMap::new())),
            settings: Arc::new(RwLock::new(CrawlLoopSettings::default())),
            last_request_times: Arc::new(Mutex::new(HashMap::new())),
            runners: Vec::new(),
            shutdown: Arc::new(AtomicBool::new(true)),
            weaviate_client: None,
        }
    }

    /// Optionally attach an existing Weaviate client (shared via Arc) so runners can index pages.
    pub fn set_weaviate_client(&mut self, client: Arc<WeaviateClient>) {
        self.weaviate_client = Some(client);
    }

    pub async fn add_crawl_request(&mut self, crawl_request: CrawlRequest) {
        let base_url = get_base_url(&crawl_request.url);
        self.crawl_requests
            .write()
            .await
            .insert(base_url, crawl_request);
    }

    /// Start background runners and return once they've been spawned.
    /// Runners will keep running until `stop` or `drop`.
    pub async fn run(&mut self) {
        let settings = self.settings.read().await;
        let max_concurrent = settings.max_concurrent_requests as usize;
        drop(settings);

        // create and start runners
        for i in 0..max_concurrent {
            let mut runner = CrawlRunner::new(
                i,
                self.crawl_requests.clone(),
                self.settings.clone(),
                self.last_request_times.clone(),
                self.shutdown.clone(),
                self.weaviate_client.clone(),
            );
            runner.start();
            self.runners.push(runner);
        }

        tracing::info!("CrawlLoop started {} runners", self.runners.len());
    }

    /// Request a graceful stop. Runners will observe shutdown and stop; we also
    /// abort their tasks to ensure the process can exit promptly.
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
        // best-effort stop; task aborts happen in runner.stop()
        self.shutdown.store(false, Ordering::SeqCst);
        for runner in &mut self.runners {
            runner.stop();
        }
    }
}
