use crate::web_visitor::WebVisitor;
use anyhow::{Context, Result};
use chromiumoxide::browser::{Browser, BrowserConfig};
use futures::StreamExt;
use std::env;
use std::sync::Arc;
use tokio::sync::{Mutex, OnceCell};

/// A browser-backed `WebVisitor` implementation that uses a shared Chromium
/// instance (a simple pool / singleton) to load pages and return HTML content.
///
/// This file implements:
/// - `BrowserPool`: a lazily-initialized shared browser instance. The pool
///    launches Chromium once, keeps the Browser, and spawns the event handler
///    polling task so the browser remains functional.
/// - `WebVisitorBrowser`: lightweight façade that delegates fetches to the pool.
///
/// Configuration:
/// - `SPIDER_BROWSER_HEADLESS` (optional): if set to "false" (case-insensitive)
///    the browser will be launched with a visible headful window. Default: headless.
///
/// Notes:
/// - Reusing a single browser instance drastically reduces startup overhead
///   compared to launching a new browser per page.
/// - The simplistic pool here holds a single Browser. For higher throughput you
///   can extend this to a connection pool of multiple Browser instances.
static BROWSER_POOL: OnceCell<Arc<BrowserPool>> = OnceCell::const_new();

pub struct BrowserPool {
    browser: Arc<Browser>,
    // A mutex can be used in future to protect operations that require exclusive access.
    // We keep it here as a placeholder if the Browser API requires sequentialization.
    _lock: Arc<Mutex<()>>,
}

impl BrowserPool {
    /// Initialize a BrowserPool by launching Chromium and spawning the handler poller.
    async fn initialize() -> Result<Self> {
        // Determine headless setting from environment for flexibility.
        let headless = env::var("SPIDER_BROWSER_HEADLESS")
            .map(|v| v.to_lowercase() != "false")
            .unwrap_or(true);

        let mut builder = BrowserConfig::builder();
        if headless {
            // In many chromiumoxide versions the default is headless; if available,
            // don't call with_head to keep headless. If you prefer explicitness,
            // adjust here to call a headless-specific builder.
            // For development, callers can set SPIDER_BROWSER_HEADLESS=false.
        } else {
            // When not headless, request a headful browser window.
            builder = builder.with_head();
        }

        let config = builder
            .build()
            .map_err(|e| anyhow::anyhow!("failed to build BrowserConfig: {}", e))?;

        let (browser, mut handler) = Browser::launch(config)
            .await
            .context("failed to launch chromium browser")?;

        let browser = Arc::new(browser);

        // Spawn a background task to continuously poll the handler so the browser can process events.
        // Keep the task alive for the lifetime of the process; if the handler returns an error
        // we simply stop polling (the pool could be re-initialized if desired).
        let _browser_clone = Arc::clone(&browser);
        tokio::spawn(async move {
            while let Some(res) = handler.next().await {
                if res.is_err() {
                    // stop the background loop on error
                    break;
                }
            }
            // If the handler ended, the browser may no longer function properly.
            // For now we don't attempt automatic restart; callers can be updated to handle this.
            let _ = _browser_clone;
        });

        Ok(BrowserPool {
            browser,
            _lock: Arc::new(Mutex::new(())),
        })
    }

    /// Return the global BrowserPool instance, initializing it on first use.
    async fn get() -> Arc<Self> {
        BROWSER_POOL
            .get_or_init(|| async {
                Arc::new(
                    BrowserPool::initialize()
                        .await
                        .expect("Failed to initialize browser pool"),
                )
            })
            .await
            .clone()
    }

    /// Fetch a page using the pooled browser instance.
    /// This mirrors the previous per-call logic but reuses the Browser.
    pub async fn fetch_page(&self, url: &str) -> Result<String> {
        // Acquire a light mutex if future browser operations need to be sequentialized.
        // Currently Chromium's `new_page` is generally safe to call concurrently, but
        // keeping this allows easy adaptation if we observe races.
        let _guard = self._lock.lock().await;

        // Create a new page and navigate to the requested URL.
        let page = self
            .browser
            .new_page("about:blank")
            .await
            .context("failed to create new page")?;

        page.goto(url).await.context("failed to navigate to url")?;

        // Wait for navigation to finish. Depending on the site you might need to
        // wait for network idle or a particular selector; this is a generic wait.
        page.wait_for_navigation()
            .await
            .context("navigation did not complete")?;

        let html = page
            .content()
            .await
            .context("failed to retrieve page content")?;

        Ok(html)
    }
}

/// Public lightweight façade used by the crawler. Construction is cheap and
/// delegates to the shared `BrowserPool`.
pub struct WebVisitorBrowser;

impl WebVisitorBrowser {
    pub fn new() -> Self {
        Self {}
    }
}

impl WebVisitor for WebVisitorBrowser {
    async fn fetch_page(&self, url: &str) -> Result<String> {
        // Use the shared pool to fetch the page.
        let pool = BrowserPool::get().await;
        pool.fetch_page(url).await
    }
}
