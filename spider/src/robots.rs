//! robots.txt compliance module
//!
//! Provides a cache for fetching, parsing, and checking robots.txt rules
//! to ensure the crawler respects website crawling policies.

use std::collections::HashMap;
use std::time::Duration;

use reqwest::Client;
use robots_txt::matcher::SimpleMatcher;
use robots_txt::Robots;
use tokio::sync::RwLock;
use url::Url;

use crate::USER_AGENT;

/// Result of fetching a robots.txt file - stores the raw content
#[derive(Clone)]
enum RobotsTxtResult {
    /// Successfully fetched robots.txt (stores raw content for parsing)
    Fetched(String),
    /// Failed to fetch (treat as allowed)
    Failed,
}

/// Cache for robots.txt files, keyed by domain (scheme://host:port)
pub struct RobotsCache {
    cache: RwLock<HashMap<String, RobotsTxtResult>>,
    client: Client,
}

impl RobotsCache {
    /// Create a new RobotsCache with a shared HTTP client
    pub fn new() -> Self {
        let client = Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(10))
            .build()
            .expect("Failed to create HTTP client for robots.txt");

        Self {
            cache: RwLock::new(HashMap::new()),
            client,
        }
    }

    /// Extract the origin (scheme://host:port) from a URL
    fn get_origin(url: &str) -> Option<String> {
        Url::parse(url).ok().map(|parsed| {
            let scheme = parsed.scheme();
            let host = parsed.host_str().unwrap_or("");
            match parsed.port() {
                Some(port) => format!("{}://{}:{}", scheme, host, port),
                None => format!("{}://{}", scheme, host),
            }
        })
    }

    /// Fetch robots.txt content for a given origin
    async fn fetch_robots(&self, origin: &str) -> RobotsTxtResult {
        let robots_url = format!("{}/robots.txt", origin);
        tracing::debug!("Fetching robots.txt from: {}", robots_url);

        match self.client.get(&robots_url).send().await {
            Ok(response) => {
                if response.status().is_success() {
                    match response.text().await {
                        Ok(body) => {
                            tracing::info!(
                                "Successfully fetched robots.txt for {} ({} bytes)",
                                origin,
                                body.len()
                            );
                            RobotsTxtResult::Fetched(body)
                        }
                        Err(e) => {
                            tracing::warn!(
                                "Failed to read robots.txt body for {}: {}",
                                origin,
                                e
                            );
                            RobotsTxtResult::Failed
                        }
                    }
                } else if response.status().as_u16() == 404 {
                    // No robots.txt means everything is allowed
                    tracing::debug!("No robots.txt found for {} (404)", origin);
                    RobotsTxtResult::Failed
                } else {
                    tracing::warn!(
                        "Failed to fetch robots.txt for {}: HTTP {}",
                        origin,
                        response.status()
                    );
                    RobotsTxtResult::Failed
                }
            }
            Err(e) => {
                tracing::warn!("Failed to fetch robots.txt for {}: {}", origin, e);
                RobotsTxtResult::Failed
            }
        }
    }

    /// Check if a URL is allowed to be crawled according to robots.txt
    ///
    /// Returns `true` if:
    /// - The URL is explicitly allowed
    /// - The robots.txt could not be fetched (fail-open)
    /// - The URL is malformed
    pub async fn is_allowed(&self, url: &str) -> bool {
        let origin = match Self::get_origin(url) {
            Some(o) => o,
            None => {
                tracing::warn!("Could not parse origin from URL: {}", url);
                return true; // Fail open for malformed URLs
            }
        };

        // Extract path early so we can use it later
        let path = Url::parse(url)
            .map(|u| u.path().to_string())
            .unwrap_or_else(|_| "/".to_string());

        // Check cache first (read lock)
        {
            let cache = self.cache.read().await;
            if let Some(result) = cache.get(&origin) {
                return Self::check_allowed(result, &path, url);
            }
        }

        // Not in cache, fetch robots.txt (outside any lock to avoid blocking)
        let result = self.fetch_robots(&origin).await;

        // Store in cache (write lock)
        let cached_result = {
            let mut cache = self.cache.write().await;
            // Double-check in case another task fetched it while we were waiting
            if let Some(existing) = cache.get(&origin) {
                existing.clone()
            } else {
                cache.insert(origin.clone(), result.clone());
                result
            }
        };

        Self::check_allowed(&cached_result, &path, url)
    }

    /// Check if a path is allowed based on a RobotsTxtResult
    fn check_allowed(result: &RobotsTxtResult, path: &str, url: &str) -> bool {
        match result {
            RobotsTxtResult::Fetched(content) => {
                // Parse robots.txt on-demand (borrows content)
                let robots = Robots::from_str_lossy(content);
                
                // Choose the section for our user-agent
                let section = robots.choose_section(USER_AGENT);
                
                // Create a matcher from the section's rules
                let matcher = SimpleMatcher::new(&section.rules);
                
                // Check if the path is allowed
                let allowed = matcher.check_path(path);

                if !allowed {
                    tracing::info!(
                        "robots.txt disallows {} for user-agent '{}'",
                        url,
                        USER_AGENT
                    );
                }

                allowed
            }
            RobotsTxtResult::Failed => {
                // Fail open: if we couldn't fetch robots.txt, assume allowed
                true
            }
        }
    }

    /// Get the number of cached domains (for metrics/debugging)
    pub async fn cache_size(&self) -> usize {
        self.cache.read().await.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_origin() {
        assert_eq!(
            RobotsCache::get_origin("https://example.com/page"),
            Some("https://example.com".to_string())
        );
        assert_eq!(
            RobotsCache::get_origin("http://example.com:8080/path"),
            Some("http://example.com:8080".to_string())
        );
        assert_eq!(
            RobotsCache::get_origin("https://sub.example.com/a/b/c"),
            Some("https://sub.example.com".to_string())
        );
    }

    #[test]
    fn test_check_allowed_with_rules() {
        let content = r#"
User-agent: *
Disallow: /admin/
Disallow: /private/

User-agent: PoliteWebCrawler
Disallow: /secret/
"#.to_string();

        let result = RobotsTxtResult::Fetched(content);
        
        // Allowed paths
        assert!(RobotsCache::check_allowed(&result, "/", "https://example.com/"));
        assert!(RobotsCache::check_allowed(&result, "/public", "https://example.com/public"));
        
        // Disallowed paths (for PoliteWebCrawler user-agent)
        assert!(!RobotsCache::check_allowed(&result, "/secret/file", "https://example.com/secret/file"));
    }

    #[test]
    fn test_check_allowed_failed_result() {
        let result = RobotsTxtResult::Failed;
        // Should always return true (fail open)
        assert!(RobotsCache::check_allowed(&result, "/admin/", "https://example.com/admin/"));
        assert!(RobotsCache::check_allowed(&result, "/anything", "https://example.com/anything"));
    }
}
