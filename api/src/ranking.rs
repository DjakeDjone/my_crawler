use shared_crawler_api::WebPageResult;
use std::{collections::HashMap, env, fs::File, io::Read, time::Duration};
use url::Url;
use zip::ZipArchive;

const URL_LENGTH_BOOST_FACTOR: f32 = 0.5;
const DOMAIN_ROOT_BOOST: f32 = 1.25;
const PATH_DEPTH_PENALTY: f32 = 0.12;
const EXACT_MATCH_BOOST: f32 = 3.0;
const ROOT_HOST_MATCH_BOOST: f32 = 2.0;
const POPULARITY_BOOST: f32 = 1.0;
const TRANCO_MAX_RANK: u32 = 1_000_000;
const DEFAULT_TRANCO_URL: &str = "https://tranco-list.eu/top-1m.csv.zip";

const FILE_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "webp", "avif", "svg", "ico", "pdf", "zip", "tar", "gz", "7z",
    "mp3", "wav", "ogg", "mp4", "webm", "mov", "avi", "doc", "docx", "xls", "xlsx", "ppt", "pptx",
];

#[derive(Clone, Default)]
pub struct DomainPopularity {
    ranks: HashMap<String, u32>,
}

impl DomainPopularity {
    fn from_csv(csv: &str) -> Self {
        let ranks = csv
            .lines()
            .filter_map(|line| {
                let (rank, domain) = line.split_once(',')?;
                Some((normalize_host(domain), rank.parse().ok()?))
            })
            .collect();
        Self { ranks }
    }

    fn rank(&self, url: &str) -> Option<u32> {
        let host = Url::parse(url).ok()?.host_str().map(normalize_host)?;
        self.ranks.get(&host).copied()
    }
}

pub async fn load_domain_popularity(http: &reqwest::Client) -> anyhow::Result<DomainPopularity> {
    if let Ok(path) = env::var("TRANCO_LIST_PATH") {
        let mut archive = ZipArchive::new(File::open(path)?)?;
        return read_tranco_zip(&mut archive);
    }

    let url = env::var("TRANCO_LIST_URL").unwrap_or_else(|_| DEFAULT_TRANCO_URL.to_string());
    let bytes = tokio::time::timeout(Duration::from_secs(20), async {
        http.get(url)
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await
    })
    .await??;
    let cursor = std::io::Cursor::new(bytes);
    let mut archive = ZipArchive::new(cursor)?;
    read_tranco_zip(&mut archive)
}

fn read_tranco_zip<R: Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
) -> anyhow::Result<DomainPopularity> {
    let mut csv = String::new();
    archive.by_index(0)?.read_to_string(&mut csv)?;
    Ok(DomainPopularity::from_csv(&csv))
}

fn normalize_host(host: &str) -> String {
    let host = host.trim().trim_end_matches('.');
    host.strip_prefix("www.")
        .unwrap_or(host)
        .to_ascii_lowercase()
}

fn popularity_boost(rank: u32) -> f32 {
    let rank = rank.clamp(1, TRANCO_MAX_RANK) as f32;
    POPULARITY_BOOST * (1.0 - (rank.ln() / (TRANCO_MAX_RANK as f32).ln()))
}

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

fn query_terms(query: &str) -> Vec<String> {
    query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|term| !term.is_empty())
        .map(str::to_lowercase)
        .collect()
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

fn looks_like_file_url(url: &Url) -> bool {
    url.path_segments()
        .and_then(Iterator::last)
        .and_then(|segment| segment.rsplit_once('.').map(|(_, ext)| ext))
        .is_some_and(|ext| FILE_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
}

pub fn is_searchable_page(url: &str) -> bool {
    Url::parse(url)
        .map(|url| !looks_like_file_url(&url))
        .unwrap_or(true)
}

fn root_host_query_match(query: &str, title: &str, url: &str) -> bool {
    if !is_domain_root(url) {
        return false;
    }
    let terms = query_terms(query);
    if terms.is_empty() {
        return false;
    }
    let Ok(parsed) = Url::parse(url) else {
        return false;
    };
    let words = parsed
        .host_str()
        .unwrap_or_default()
        .split(|c: char| !c.is_alphanumeric())
        .chain(title.split(|c: char| !c.is_alphanumeric()))
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect::<Vec<_>>();
    terms.iter().all(|term| words.contains(term))
}

/// Apply all ranking boosts/penalties to a single result
///
/// This modifies the result's score in place based on:
/// 1. URL length boost (shorter URLs rank higher)
/// 2. Domain root boost (root pages get bonus)
/// 3. Path depth penalty (deeper pages get penalized)
/// 4. Query-term coverage boost (query words found in title/URL)
pub fn apply_ranking_boost(result: &mut WebPageResult, query: &str, popularity: &DomainPopularity) {
    let url = &result.data.source_url;
    let title = &result.data.page_title;
    let url_len = url.len().max(1) as f32;

    // 1. URL length boost (inversely proportional to length)
    result.score += URL_LENGTH_BOOST_FACTOR / url_len;

    // 2. Domain root boost
    if is_domain_root(url) {
        result.score += DOMAIN_ROOT_BOOST;
    }

    if root_host_query_match(query, title, url) {
        result.score += ROOT_HOST_MATCH_BOOST;
    }

    // 3. Path depth penalty
    let depth = get_path_depth(url);
    if depth > 0 {
        result.score -= (depth as f32) * PATH_DEPTH_PENALTY;
    }

    result.score += EXACT_MATCH_BOOST * query_match_coverage(query, title, url);

    if let Some(rank) = popularity.rank(url) {
        result.score += popularity_boost(rank);
    }
}

/// Apply ranking boosts to all results and re-sort by score descending
pub fn apply_ranking_boosts(
    results: &mut [WebPageResult],
    query: &str,
    popularity: &DomainPopularity,
) {
    for result in results.iter_mut() {
        apply_ranking_boost(result, query, popularity);
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
        apply_ranking_boost(&mut google, "", &DomainPopularity::default());

        // Test Portfolio (longer url + root)
        let mut portfolio = make_result("https://home.fri3dl.dev");
        apply_ranking_boost(&mut portfolio, "", &DomainPopularity::default());

        println!("Google Score: {}", google.score);
        println!("Portfolio Score: {}", portfolio.score);

        assert!(google.score > portfolio.score);
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
        apply_ranking_boost(&mut res1, "Benjamin", &DomainPopularity::default());
        // base 0.5 + boost 3.0 + other small ranking factors
        assert!(res1.score > 3.0);

        // Case 2: Match in URL
        let mut res2 = make_result("https://benjamin.com", "Hello World");
        apply_ranking_boost(&mut res2, "Benjamin", &DomainPopularity::default());
        assert!(res2.score > 3.0);

        // Case 3: No Match
        let mut res3 = make_result("https://example.com", "Hello World");
        apply_ranking_boost(&mut res3, "Benjamin", &DomainPopularity::default());
        assert!(res3.score < res1.score);
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
    fn root_host_query_wins_navigational_searches() {
        let make_result = |url: &str, score| WebPageResult {
            score,
            data: WebPageChunk {
                source_url: url.to_string(),
                page_title: "Wikipedia".to_string(),
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
            make_result("https://en.wikipedia.org/wiki/Wikipedia:About", 4.10),
            make_result("https://en.wikipedia.org/", 3.50),
        ];

        apply_ranking_boosts(&mut results, "wikipedia", &DomainPopularity::default());

        assert_eq!(results[0].data.source_url, "https://en.wikipedia.org/");
    }

    #[test]
    fn high_semantic_score_can_still_win_unrelated_queries() {
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
            make_result("https://example.com/deep/page", 6.00),
            make_result("https://example.com/", 3.50),
        ];

        apply_ranking_boosts(&mut results, "rust crawler", &DomainPopularity::default());

        assert_eq!(results[0].data.source_url, "https://example.com/deep/page");
    }

    #[test]
    fn detects_searchable_pages() {
        assert!(!is_searchable_page("https://example.com/image.jpg"));
        assert!(!is_searchable_page("https://example.com/file.pdf"));
        assert!(is_searchable_page(
            "https://en.wikipedia.org/wiki/Wikipedia:About"
        ));
        assert!(is_searchable_page("https://en.wikipedia.org/"));
        assert!(is_searchable_page("https://example.com/index.html"));
    }

    #[test]
    fn popularity_boost_rewards_tranco_rank() {
        let make_result = |url: &str| WebPageResult {
            score: 1.0,
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
        let popularity = DomainPopularity::from_csv("1,popular.example\n900000,unknown.example\n");
        let mut results = [
            make_result("https://unknown.example/"),
            make_result("https://www.popular.example/"),
        ];

        apply_ranking_boosts(&mut results, "", &popularity);

        assert_eq!(results[0].data.source_url, "https://www.popular.example/");
    }
}
