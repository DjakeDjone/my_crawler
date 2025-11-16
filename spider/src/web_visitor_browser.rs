use crate::web_visitor::WebVisitor;
use anyhow::{Context, Result};
use chromiumoxide::browser::{Browser, BrowserConfig};
use futures::StreamExt;

/// A small browser-backed `WebVisitor` implementation that uses `chromiumoxide`
/// to load a page and return its HTML content.
///
/// This implementation launches a short-lived browser per call. It's simple and
/// avoids shared-ownership / locking complexity. If you need to reuse a single
/// browser instance across calls, consider switching to an `Arc<Mutex<Browser>>`
/// or similar pattern.
pub struct WebVisitorBrowser;

impl WebVisitorBrowser {
    pub fn new() -> Self {
        Self {}
    }
}

impl WebVisitor for WebVisitorBrowser {
    async fn fetch_page(&self, url: &str) -> Result<String> {
        // Launch a browser instance (this returns a Browser and a Handler we must poll).
        let config = BrowserConfig::builder()
            // show UI while developing; remove `.with_head()` to run headless
            .with_head()
            .build()
            .map_err(|e| anyhow::anyhow!("failed to build BrowserConfig: {}", e))?;
        let (browser, mut handler) = Browser::launch(config)
            .await
            .context("failed to launch chromium browser")?;

        // Spawn a background task that continuously polls the handler so the
        // browser can process events. We intentionally ignore the task handle.
        tokio::spawn(async move {
            // `next()` is provided by `StreamExt` (imported above).
            while let Some(res) = handler.next().await {
                if res.is_err() {
                    // stop the background loop on error
                    break;
                }
            }
        });

        // Create a new page and navigate to the requested URL.
        // The chromiumoxide API exposes `new_page`, `goto`, `wait_for_navigation`,
        // and `content`. We call them in sequence and return the page HTML.
        let page = browser
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
