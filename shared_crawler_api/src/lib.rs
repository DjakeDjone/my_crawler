use serde::{Deserialize, Serialize};

pub mod util_fns;

/// The name of the Weaviate class for web pages
pub const WEAVIATE_CLASS_NAME: &str = "WebPage";

/// Field names for the WebPage schema
pub mod fields {
    pub const CHUNK_CONTENT: &str = "chunk_content";
    pub const CHUNK_HEADING: &str = "chunk_heading";
    pub const SOURCE_URL: &str = "source_url";
    pub const PAGE_TITLE: &str = "page_title";
    pub const DESCRIPTION: &str = "description";
    pub const SCORE: &str = "score";
    pub const CRAWLED_AT: &str = "crawled_at";
}

/// Shared data structure for web page data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebPageChunk {
    //
    pub chunk_content: String,
    pub chunk_heading: Option<String>,

    // for the search results
    // will be redundant for all chunks of the same page but ok for performance
    pub source_url: String,
    pub page_title: String,
    pub description: String,

    pub score: f64,
    pub crawled_at: i64,
}

impl WebPageChunk {
    /// Create a new WebPageData instance
    pub fn new(
        chunk_content: String,
        chunk_heading: Option<String>,
        source_url: String,
        page_title: String,
        description: String,
        score: f64,
        crawled_at: i64,
    ) -> Self {
        Self {
            chunk_content,
            chunk_heading,
            source_url,
            page_title,
            description,
            score,
            crawled_at,
        }
    }

    /// Get all field names for Weaviate queries
    pub fn field_names() -> Vec<&'static str> {
        vec![
            fields::CHUNK_CONTENT,
            fields::CHUNK_HEADING,
            fields::SOURCE_URL,
            fields::PAGE_TITLE,
            fields::DESCRIPTION,
            fields::SCORE,
            fields::CRAWLED_AT,
        ]
    }

    /// Convert to JSON for Weaviate object creation
    pub fn to_properties_json(&self) -> serde_json::Value {
        serde_json::json!({
            fields::CHUNK_CONTENT: self.chunk_content,
            fields::CHUNK_HEADING: self.chunk_heading,
            fields::SOURCE_URL: self.source_url,
            fields::PAGE_TITLE: self.page_title,
            fields::DESCRIPTION: self.description,
            fields::SCORE: self.score,
            fields::CRAWLED_AT: self.crawled_at,
        })
    }

    /// Parse from Weaviate response JSON
    pub fn from_weaviate_json(value: &serde_json::Value) -> Option<Self> {
        Some(Self {
            chunk_content: value
                .get(fields::CHUNK_CONTENT)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            chunk_heading: value
                .get(fields::CHUNK_HEADING)
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            source_url: value
                .get(fields::SOURCE_URL)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            page_title: value
                .get(fields::PAGE_TITLE)
                .and_then(|v| v.as_str())
                .unwrap_or("No Title")
                .to_string(),
            description: value
                .get(fields::DESCRIPTION)
                .and_then(|v| v.as_str())
                .unwrap_or("No description available")
                .to_string(),
            score: value
                .get(fields::SCORE)
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0),
            crawled_at: value
                .get(fields::CRAWLED_AT)
                .and_then(|v| v.as_i64())
                .unwrap_or(0),
        })
    }
}

/// Result structure for search queries with similarity score
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebPageResult {
    #[serde(flatten)]
    pub data: WebPageChunk,
    pub score: f32,
}

impl WebPageResult {
    pub fn new(data: WebPageChunk, score: f32) -> Self {
        Self { data, score }
    }

    /// Parse from Weaviate response JSON with distance
    pub fn from_weaviate_json(value: &serde_json::Value) -> Option<Self> {
        let data = WebPageChunk::from_weaviate_json(value)?;

        let distance = value
            .get("_additional")
            .and_then(|a| a.get("distance"))
            .and_then(|d| d.as_f64())
            .unwrap_or(1.0) as f32;

        // Convert distance to similarity score (0-1 range, higher is better)
        let score = 1.0 - distance;

        Some(Self { data, score })
    }
}
