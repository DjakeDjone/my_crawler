//! Page Ranking Module
//!
//! This module contains the ranking algorithm for search results.
//! Modify the `RankingConfig` defaults to tune ranking behavior.

use shared_crawler_api::WebPageResult;
use url::Url;

/// Configuration for ranking adjustments.
/// 
/// Modify these defaults to tune how search results are ranked.
/// Higher values = stronger effect on final score.
#[derive(Debug, Clone)]
pub struct RankingConfig {
    /// Boost factor divided by URL length (shorter URLs score higher)
    /// Example: factor=2.0, URL length=25 chars → boost of 0.08
    pub url_length_boost_factor: f32,

    /// Bonus score added to domain root pages (no path after domain)
    /// Example: https://example.com gets this boost, https://example.com/page does not
    pub domain_root_boost: f32,

    /// Penalty subtracted per path segment depth
    /// Example: /a = -0.03, /a/b = -0.06, /a/b/c = -0.09
    pub path_depth_penalty: f32,

    /// Boost added if the query is found in the page title or URL
    pub exact_match_boost: f32,
}

impl Default for RankingConfig {
    fn default() -> Self {
        Self {
            url_length_boost_factor: 0.5,
            domain_root_boost: 0.05,
            path_depth_penalty: 0.03,
            exact_match_boost: 3.0,
        }
    }
}

impl RankingConfig {
    /// Create a new config with custom values
    #[allow(dead_code)]
    pub fn new(
        url_length_boost_factor: f32,
        domain_root_boost: f32,
        path_depth_penalty: f32,
        exact_match_boost: f32,
    ) -> Self {
        Self {
            url_length_boost_factor,
            domain_root_boost,
            path_depth_penalty,
            exact_match_boost,
        }
    }
}

/// Calculate the path depth of a URL (number of non-empty path segments)
/// 
/// Examples:
/// - "https://example.com" → 0
/// - "https://example.com/" → 0
/// - "https://example.com/page" → 1
/// - "https://example.com/a/b/c" → 3
fn get_path_depth(url: &str) -> usize {
    if let Ok(parsed) = Url::parse(url) {
        parsed
            .path_segments()
            .map(|segments| segments.filter(|s| !s.is_empty()).count())
            .unwrap_or(0)
    } else {
        0
    }
}

/// Check if a URL is a domain root (no meaningful path)
/// 
/// Returns true for:
/// - "https://example.com"
/// - "https://example.com/"
/// 
/// Returns false for:
/// - "https://example.com/page"
/// - "https://example.com/a/b"
fn is_domain_root(url: &str) -> bool {
    get_path_depth(url) == 0
}

/// Apply all ranking boosts/penalties to a single result
/// 
/// This modifies the result's score in place based on:
/// 1. URL length boost (shorter URLs rank higher)
/// 2. Domain root boost (root pages get bonus)
/// 3. Path depth penalty (deeper pages get penalized)
/// 4. Exact match boost (query found in title/URL)
pub fn apply_ranking_boost(result: &mut WebPageResult, config: &RankingConfig, query: &str) {
    let url = &result.data.source_url;
    let title = &result.data.page_title;
    let url_len = url.len().max(1) as f32;

    // 1. URL length boost (inversely proportional to length)
    result.score += config.url_length_boost_factor / url_len;

    // 2. Domain root boost
    if is_domain_root(url) {
        result.score += config.domain_root_boost;
    }

    // 3. Path depth penalty
    let depth = get_path_depth(url);
    if depth > 0 {
        result.score -= (depth as f32) * config.path_depth_penalty;
    }

    // 4. Exact match boost
    // Simple case-insensitive check
    let query_lower = query.to_lowercase();
    if !query_lower.is_empty() {
        if url.to_lowercase().contains(&query_lower) || title.to_lowercase().contains(&query_lower) {
            result.score += config.exact_match_boost;
        }
    }
}

/// Apply ranking boosts to all results and re-sort by score descending
pub fn apply_ranking_boosts(results: &mut Vec<WebPageResult>, config: &RankingConfig, query: &str) {
    for result in results.iter_mut() {
        apply_ranking_boost(result, config, query);
    }

    // Re-sort by score descending
    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared_crawler_api::WebPageChunk;

    #[test]
    fn test_path_depth() {
        assert_eq!(get_path_depth("https://example.com"), 0);
        assert_eq!(get_path_depth("https://example.com/"), 0);
        assert_eq!(get_path_depth("https://example.com/page"), 1);
        assert_eq!(get_path_depth("https://example.com/a/b"), 2);
        assert_eq!(get_path_depth("https://example.com/a/b/c"), 3);
    }

    #[test]
    fn test_is_domain_root() {
        assert!(is_domain_root("https://example.com"));
        assert!(is_domain_root("https://example.com/"));
        assert!(!is_domain_root("https://example.com/page"));
        assert!(!is_domain_root("https://example.com/a/b"));
    }

    #[test]
    fn test_ranking_impact() {
        let config = RankingConfig::default();
        
        // internal helper to simulate a result
        let mut make_result = |url: &str| {
            WebPageResult {
                score: 0.5, // base score
                data: WebPageChunk {
                    source_url: url.to_string(),
                    chunk_content: "".to_string(),
                    chunk_heading: None,
                    page_title: "".to_string(),
                    description: "".to_string(),
                    tags: vec![],
                    categories: vec![],
                    paid: 0.0,
                    // Note: this score field inside data is separate from the search result score
                    score: 0.0,
                    crawled_at: 0,
                },
            }
        };

        // Test Google (short url + root)
        let mut google = make_result("google.com");
        apply_ranking_boost(&mut google, &config, "");
        
        // Test Portfolio (longer url + root)
        let mut portfolio = make_result("https://home.fri3dl.dev");
        apply_ranking_boost(&mut portfolio, &config, "");

        println!("Google Score: {}", google.score);
        println!("Portfolio Score: {}", portfolio.score);

        // With new values:
        // Google: base 0.5 + (0.5 / 10) + 0.05 = 0.5 + 0.05 + 0.05 = 0.60
        // Previous was ~0.85
        assert!(google.score < 0.65);
    }

    #[test]
    fn test_exact_match_boost() {
        let config = RankingConfig::default();
        
        let mut make_result = |url: &str, title: &str| {
            WebPageResult {
                score: 0.5,
                data: WebPageChunk {
                    source_url: url.to_string(),
                    page_title: title.to_string(),
                    chunk_content: "".to_string(),
                    chunk_heading: None,
                    description: "".to_string(),
                    tags: vec![],
                    categories: vec![],
                    paid: 0.0,
                    score: 0.0,
                    crawled_at: 0,
                },
            }
        };

        // Case 1: Match in Title
        let mut res1 = make_result("https://example.com", "Hello Benjamin");
        apply_ranking_boost(&mut res1, &config, "Benjamin");
        // base 0.5 + boost 3.0 + other small ranking factors
        assert!(res1.score > 3.0);

        // Case 2: Match in URL
        let mut res2 = make_result("https://benjamin.com", "Hello World");
        apply_ranking_boost(&mut res2, &config, "Benjamin");
        assert!(res2.score > 3.0);

        // Case 3: No Match
        let mut res3 = make_result("https://example.com", "Hello World");
        apply_ranking_boost(&mut res3, &config, "Benjamin");
        assert!(res3.score < 1.0);
    }
}
