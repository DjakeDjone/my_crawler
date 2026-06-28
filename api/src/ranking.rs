use shared_crawler_api::WebPageResult;
use url::Url;

const URL_LENGTH_BOOST_FACTOR: f32 = 0.5;
const DOMAIN_ROOT_BOOST: f32 = 0.05;
const PATH_DEPTH_PENALTY: f32 = 0.03;
const EXACT_MATCH_BOOST: f32 = 3.0;

fn query_match_coverage(query: &str, title: &str, url: &str) -> f32 {
    let terms = query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|term| !term.is_empty())
        .map(str::to_lowercase)
        .collect::<Vec<_>>();
    if terms.is_empty() {
        return 0.0;
    }

    let words = title
        .split(|c: char| !c.is_alphanumeric())
        .chain(url.split(|c: char| !c.is_alphanumeric()))
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect::<Vec<_>>();
    terms.iter().filter(|term| words.contains(term)).count() as f32 / terms.len() as f32
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
/// 4. Query-term coverage boost (query words found in title/URL)
pub fn apply_ranking_boost(result: &mut WebPageResult, query: &str) {
    let url = &result.data.source_url;
    let title = &result.data.page_title;
    let url_len = url.len().max(1) as f32;

    // 1. URL length boost (inversely proportional to length)
    result.score += URL_LENGTH_BOOST_FACTOR / url_len;

    // 2. Domain root boost
    if is_domain_root(url) {
        result.score += DOMAIN_ROOT_BOOST;
    }

    // 3. Path depth penalty
    let depth = get_path_depth(url);
    if depth > 0 {
        result.score -= (depth as f32) * PATH_DEPTH_PENALTY;
    }

    result.score += EXACT_MATCH_BOOST * query_match_coverage(query, title, url);
}

/// Apply ranking boosts to all results and re-sort by score descending
pub fn apply_ranking_boosts(results: &mut [WebPageResult], query: &str) {
    for result in results.iter_mut() {
        apply_ranking_boost(result, query);
    }

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
        // internal helper to simulate a result
        let make_result = |url: &str| {
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
        apply_ranking_boost(&mut google, "");

        // Test Portfolio (longer url + root)
        let mut portfolio = make_result("https://home.fri3dl.dev");
        apply_ranking_boost(&mut portfolio, "");

        println!("Google Score: {}", google.score);
        println!("Portfolio Score: {}", portfolio.score);

        // With new values:
        // Google: base 0.5 + (0.5 / 10) + 0.05 = 0.5 + 0.05 + 0.05 = 0.60
        // Previous was ~0.85
        assert!(google.score < 0.65);
    }

    #[test]
    fn test_exact_match_boost() {
        let make_result = |url: &str, title: &str| WebPageResult {
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
        };

        // Case 1: Match in Title
        let mut res1 = make_result("https://example.com", "Hello Benjamin");
        apply_ranking_boost(&mut res1, "Benjamin");
        // base 0.5 + boost 3.0 + other small ranking factors
        assert!(res1.score > 3.0);

        // Case 2: Match in URL
        let mut res2 = make_result("https://benjamin.com", "Hello World");
        apply_ranking_boost(&mut res2, "Benjamin");
        assert!(res2.score > 3.0);

        // Case 3: No Match
        let mut res3 = make_result("https://example.com", "Hello World");
        apply_ranking_boost(&mut res3, "Benjamin");
        assert!(res3.score < 1.0);
    }

    #[test]
    fn query_boost_rewards_term_coverage_without_substring_matches() {
        assert_eq!(
            query_match_coverage(
                "rust async crawler",
                "Building an async Rust service",
                "https://example.com/guide"
            ),
            2.0 / 3.0
        );
        assert_eq!(
            query_match_coverage("rust", "A trustworthy guide", "https://example.com"),
            0.0
        );
    }

    #[test]
    fn score_outweighs_root_bonus() {
        let make_result = |url: &str, score| WebPageResult {
            score,
            data: WebPageChunk {
                source_url: url.to_string(),
                page_title: String::new(),
                chunk_content: String::new(),
                chunk_heading: None,
                description: String::new(),
                tags: vec![],
                categories: vec![],
                paid: 0.0,
                score: 0.0,
                crawled_at: 0,
            },
        };
        let mut results = [
            make_result("https://example.com/imprint", 4.70),
            make_result("https://example.com/", 4.57),
        ];

        apply_ranking_boosts(&mut results, "");

        assert_eq!(results[0].data.source_url, "https://example.com/imprint");
    }
}
