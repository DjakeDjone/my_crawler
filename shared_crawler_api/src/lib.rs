use serde::{Deserialize, Serialize};

pub mod util_fns;

pub const QDRANT_COLLECTION_NAME: &str = "web_pages";

/// Shared data structure for web page data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebPageChunk {
    #[serde(default)]
    pub chunk_content: String,
    #[serde(default)]
    pub chunk_heading: Option<String>,

    #[serde(default)]
    pub source_url: String,
    #[serde(default = "default_title")]
    pub page_title: String,
    #[serde(default = "default_description")]
    pub description: String,

    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub categories: Vec<String>,

    #[serde(default)]
    pub paid: f64,
    #[serde(default)]
    pub score: f64,
    #[serde(default)]
    pub crawled_at: i64,
}

fn default_title() -> String {
    "No Title".into()
}

fn default_description() -> String {
    "No description available".into()
}

impl WebPageChunk {
    /// Create a new WebPageData instance
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        chunk_content: String,
        chunk_heading: Option<String>,
        source_url: String,
        page_title: String,
        description: String,
        tags: Vec<String>,
        categories: Vec<String>,
        paid: f64,
        score: f64,
        crawled_at: i64,
    ) -> Self {
        Self {
            chunk_content,
            chunk_heading,
            source_url,
            page_title,
            description,
            tags,
            categories,
            paid,
            score,
            crawled_at,
        }
    }

    pub fn to_payload_json(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("WebPageChunk is serializable")
    }

    pub fn from_payload_json(value: &serde_json::Value) -> Option<Self> {
        serde_json::from_value(value.clone()).ok()
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
}
