use serde::{Deserialize, Serialize};

pub mod util_fns;

/// The name of the Weaviate class for web pages
pub const WEAVIATE_CLASS_NAME: &str = "WebPage2";

/// Field names for the WebPage schema
pub mod fields {
    pub const URL: &str = "url";
    pub const TITLE: &str = "title";
    pub const DESCRIPTION: &str = "description";
    pub const CONTENT: &str = "content";
    pub const CONTENT_HASH: &str = "content_hash";
    pub const SUB_PAGES: &str = "sub_pages";
    pub const CRAWLED_AT: &str = "crawled_at";
}

/// Shared data structure for web page data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebPageData {
    pub url: String,
    pub title: String,
    pub description: String,
    pub content: String,
    pub content_hash: String,
    pub sub_pages: Vec<String>,
    pub crawled_at: i64,
}

impl WebPageData {
    /// Create a new WebPageData instance
    pub fn new(
        url: String,
        title: String,
        description: String,
        content_hash: String,
        content: String,
        crawled_at: i64,
        sub_pages: Vec<String>,
    ) -> Self {
        Self {
            url,
            title,
            description,
            content_hash,
            content,
            crawled_at,
            sub_pages,
        }
    }

    /// Get all field names for Weaviate queries
    pub fn field_names() -> Vec<&'static str> {
        vec![
            fields::URL,
            fields::TITLE,
            fields::DESCRIPTION,
            fields::CONTENT,
            fields::SUB_PAGES,
            fields::CRAWLED_AT,
        ]
    }

    /// Convert to JSON for Weaviate object creation
    pub fn to_properties_json(&self) -> serde_json::Value {
        serde_json::json!({
            fields::URL: self.url,
            fields::TITLE: self.title,
            fields::DESCRIPTION: self.description,
            fields::CONTENT: self.content,
            fields::CONTENT_HASH: self.content_hash,
            fields::SUB_PAGES: self.sub_pages.clone(),
            fields::CRAWLED_AT: self.crawled_at,
        })
    }

    /// Parse from Weaviate response JSON
    pub fn from_weaviate_json(value: &serde_json::Value) -> Option<Self> {
        Some(Self {
            url: value
                .get(fields::URL)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            title: value
                .get(fields::TITLE)
                .and_then(|v| v.as_str())
                .unwrap_or("No Title")
                .to_string(),
            description: value
                .get(fields::DESCRIPTION)
                .and_then(|v| v.as_str())
                .unwrap_or("No description available")
                .to_string(),
            content_hash: value
                .get(fields::CONTENT_HASH)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            content: value
                .get(fields::CONTENT)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            sub_pages: value
                .get(fields::SUB_PAGES)
                .and_then(|v| v.as_array())
                .unwrap_or(&vec![])
                .iter()
                .map(|v| v.as_str().unwrap_or("").to_string())
                .collect(),
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
    pub data: WebPageData,
    pub score: f32,
}

impl WebPageResult {
    /// Create a new WebPageResult from WebPageData and a score
    pub fn new(data: WebPageData, score: f32) -> Self {
        Self { data, score }
    }

    /// Parse from Weaviate response JSON with distance
    pub fn from_weaviate_json(value: &serde_json::Value) -> Option<Self> {
        let data = WebPageData::from_weaviate_json(value)?;

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
