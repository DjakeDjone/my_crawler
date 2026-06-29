use anyhow::{Context, Result};
use chromiumoxide::browser::{Browser, BrowserConfig};
use futures::StreamExt;
use std::env;
use std::sync::Arc;
use tokio::sync::{OnceCell, Semaphore};

static BROWSER_POOL: OnceCell<Arc<BrowserPool>> = OnceCell::const_new();

pub struct BrowserPool {
    browser: Arc<Browser>,
    pages: Arc<Semaphore>,
    settle_ms: u64,
}

impl BrowserPool {
    async fn initialize() -> Result<Self> {
        let headless = env::var("SPIDER_BROWSER_HEADLESS")
            .map(|v| v.to_lowercase() != "false")
            .unwrap_or(true);

        let mut builder = BrowserConfig::builder();
        if !headless {
            builder = builder.with_head();
        }

        let config = builder
            .no_sandbox()
            .args([
                "--headless=new",
                "--disable-gpu",
                "--disable-dev-shm-usage",
                "--disable-software-rasterizer",
                "--no-first-run",
                "--disable-extensions",
                "--remote-debugging-port=0",
            ])
            .build()
            .map_err(|e| anyhow::anyhow!("failed to build BrowserConfig: {}", e))?;

        let (browser, mut handler) = Browser::launch(config)
            .await
            .context("failed to launch chromium browser")?;

        let browser = Arc::new(browser);

        tokio::spawn(async move {
            while let Some(res) = handler.next().await {
                if res.is_err() {
                    break;
                }
            }
        });

        Ok(BrowserPool {
            browser,
            pages: Arc::new(Semaphore::new(
                env_usize("SPIDER_BROWSER_MAX_PAGES", 1).max(1),
            )),
            settle_ms: env_u64("SPIDER_BROWSER_SETTLE_MS", 1_500),
        })
    }

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

    pub async fn fetch_page_with_options(
        url: &str,
        wait_for_selector: Option<&str>,
        timeout_ms: u64,
    ) -> Result<String> {
        Self::get()
            .await
            .fetch(url, wait_for_selector, timeout_ms)
            .await
    }

    async fn fetch(
        &self,
        url: &str,
        wait_for_selector: Option<&str>,
        timeout_ms: u64,
    ) -> Result<String> {
        // ponytail: one Chromium page by default; raise SPIDER_BROWSER_MAX_PAGES only after measuring RAM.
        let _permit = self
            .pages
            .acquire()
            .await
            .context("browser limiter closed")?;
        let page = self
            .browser
            .new_page("about:blank")
            .await
            .context("failed to create new page")?;

        let result = async {
            page.goto(url).await.context("failed to navigate to url")?;
            page.wait_for_navigation()
                .await
                .context("navigation did not complete")?;

            if let Some(selector) = wait_for_selector {
                let timeout = std::time::Duration::from_millis(timeout_ms);
                let start = std::time::Instant::now();
                loop {
                    let found = page
                        .evaluate(format!(
                            "document.querySelector('{}') !== null",
                            selector.replace('\'', "\\'")
                        ))
                        .await
                        .ok()
                        .and_then(|v| v.into_value::<bool>().ok())
                        .unwrap_or(false);
                    if found {
                        break;
                    }
                    if start.elapsed() >= timeout {
                        tracing::warn!("Timeout waiting for selector '{selector}' on {url}");
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
            } else if self.settle_ms > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(self.settle_ms)).await;
            }
            page.content()
                .await
                .context("failed to retrieve page content")
        }
        .await;
        let closed = page.close().await.context("failed to close browser page");
        match result {
            Ok(html) => {
                closed?;
                Ok(html)
            }
            Err(error) => {
                let _ = closed;
                Err(error)
            }
        }
    }
}

fn env_usize(name: &str, default: usize) -> usize {
    env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn env_u64(name: &str, default: u64) -> u64 {
    env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_browser_limits_from_env() {
        env::set_var("SPIDER_BROWSER_MAX_PAGES_TEST", "2");
        env::set_var("SPIDER_BROWSER_SETTLE_MS_TEST", "750");

        assert_eq!(env_usize("SPIDER_BROWSER_MAX_PAGES_TEST", 1), 2);
        assert_eq!(env_u64("SPIDER_BROWSER_SETTLE_MS_TEST", 1_500), 750);

        env::remove_var("SPIDER_BROWSER_MAX_PAGES_TEST");
        env::remove_var("SPIDER_BROWSER_SETTLE_MS_TEST");
    }
}
