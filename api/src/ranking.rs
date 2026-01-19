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
}

impl Default for RankingConfig {
    fn default() -> Self {
        Self {
            url_length_boost_factor: 2.0,
            domain_root_boost: 0.15,
            path_depth_penalty: 0.03,
        }
    }
}

impl RankingConfig {
    /// Create a new config with custom values
    pub fn new(url_length_boost_factor: f32, domain_root_boost: f32, path_depth_penalty: f32) -> Self {
        Self {
            url_length_boost_factor,
            domain_root_boost,
            path_depth_penalty,
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
pub fn apply_ranking_boost(result: &mut WebPageResult, config: &RankingConfig) {
    let url = &result.data.source_url;
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
}

/// Apply ranking boosts to all results and re-sort by score descending
pub fn apply_ranking_boosts(results: &mut Vec<WebPageResult>, config: &RankingConfig) {
    for result in results.iter_mut() {
        apply_ranking_boost(result, config);
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
}
