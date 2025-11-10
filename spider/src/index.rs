use anyhow::Result;
use scraper::{Html, Selector};
use sha2::{Digest, Sha256};
use shared_crawler_api::{fields, WebPageData, WEAVIATE_CLASS_NAME};
use std::env;
use tracing::{error, info, warn};
use weaviate_community::collections::objects::Object;
use weaviate_community::collections::schema::{Class, Properties, Property};
use weaviate_community::WeaviateClient;

/// Extract structured data from HTML content
pub fn extract_webpage_data(
    url: String,
    html_content: String,
    sub_pages: Vec<String>,
) -> WebPageData {
    let document = Html::parse_document(&html_content);

    // Extract title
    let title = extract_title(&document);

    // Extract main text content
    let content = extract_content(&document);
    let content_hash = {
        let mut hasher = Sha256::new();
        hasher.update(&content);
        format!("{:x}", hasher.finalize())
    };

    // Extract meta description
    let description = extract_description(&document, &content);

    // Get current timestamp
    let crawled_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    WebPageData::new(
        url,
        title,
        description,
        content_hash,
        content,
        crawled_at,
        sub_pages,
    )
}

fn extract_title(document: &Html) -> String {
    let title_selector = Selector::parse("title").unwrap();
    document
        .select(&title_selector)
        .next()
        .map(|el| el.text().collect::<String>().trim().to_string())
        .unwrap_or_else(|| "No Title".to_string())
}

fn extract_description(document: &Html, content: &str) -> String {
    let meta_selector = Selector::parse("meta[name='description']").unwrap();
    if let Some(meta) = document.select(&meta_selector).next() {
        if let Some(content) = meta.value().attr("content") {
            return content.trim().to_string();
        }
    }

    // Try og:description as fallback
    let og_desc_selector = Selector::parse("meta[property='og:description']").unwrap();
    if let Some(meta) = document.select(&og_desc_selector).next() {
        if let Some(content) = meta.value().attr("content") {
            return content.trim().to_string();
        }
    }

    // generate from content
    let mut description = String::new();
    let words = content.split_whitespace().take(100);
    for word in words {
        description.push_str(word);
        description.push(' ');
    }
    description.trim().to_string()
}

fn extract_content(document: &Html) -> String {
    // Remove script, style, and other non-content tags
    let mut text_content = Vec::new();

    // Remove script and style tags from the document
    let script_selector = Selector::parse("script").unwrap();
    let style_selector = Selector::parse("style").unwrap();

    // Try to get main content areas
    let content_selectors = vec![
        "article", "main", ".content", "#content", ".post", ".article", "body",
    ];

    for selector_str in content_selectors {
        if let Ok(selector) = Selector::parse(selector_str) {
            if let Some(element) = document.select(&selector).next() {
                let mut parts = Vec::new();

                // Process text nodes, excluding script and style content
                for text_node in element.text() {
                    let trimmed = text_node.trim();
                    if !trimmed.is_empty() {
                        parts.push(trimmed.to_string());
                    }
                }

                // Filter out any text that came from script or style tags
                let filtered_parts: Vec<String> = parts
                    .into_iter()
                    .filter(|part| {
                        // Check if this text is inside a script or style tag
                        let is_script = document.select(&script_selector).any(|script_elem| {
                            script_elem
                                .text()
                                .any(|script_text| script_text.contains(part))
                        });
                        let is_style = document.select(&style_selector).any(|style_elem| {
                            style_elem
                                .text()
                                .any(|style_text| style_text.contains(part))
                        });
                        !is_script && !is_style
                    })
                    .collect();

                let text = filtered_parts.join(" ");
                if !text.trim().is_empty() {
                    text_content.push(text);
                    break;
                }
            }
        }
    }

    // If no content found, extract all paragraph text and headings
    if text_content.is_empty() {
        // Extract headings with newlines
        for i in 1..=6 {
            let h_selector = Selector::parse(&format!("h{}", i)).unwrap();
            for element in document.select(&h_selector) {
                let text = element.text().collect::<String>().trim().to_string();
                if !text.is_empty() {
                    text_content.push(format!("{}\n", text));
                }
            }
        }

        // Extract paragraphs
        let p_selector = Selector::parse("p").unwrap();
        for element in document.select(&p_selector) {
            let text = element.text().collect::<String>();
            if !text.trim().is_empty() {
                text_content.push(text);
            }
        }
    }

    // Clean up the content
    text_content
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(5000) // Limit content length
        .collect()
}

/// Initialize Weaviate client
pub fn create_weaviate_client() -> Result<WeaviateClient> {
    let weaviate_url =
        env::var("WEAVIATE_URL").unwrap_or_else(|_| "http://localhost:8080".to_string());

    let client = WeaviateClient::builder(&weaviate_url)
        .build()
        .map_err(|e| anyhow::anyhow!("Failed to create Weaviate client: {}", e))?;

    info!("Connected to Weaviate at {}", weaviate_url);
    Ok(client)
}

/// Create the WebPage schema in Weaviate if it doesn't exist
pub async fn ensure_schema(client: &WeaviateClient) -> Result<()> {
    info!(
        "Checking if '{}' class exists in Weaviate",
        WEAVIATE_CLASS_NAME
    );

    // Check if the class already exists
    match client.schema.get_class(WEAVIATE_CLASS_NAME).await {
        Ok(_) => {
            info!("Schema '{}' already exists", WEAVIATE_CLASS_NAME);
            return Ok(());
        }
        Err(_) => {
            info!(
                "Schema '{}' doesn't exist, creating it...",
                WEAVIATE_CLASS_NAME
            );
        }
    }

    // Create the WebPage class with properties
    let url_property = Property::builder(fields::URL, vec!["text"])
        .with_description("The URL of the web page")
        .build();

    let title_property = Property::builder(fields::TITLE, vec!["text"])
        .with_description("The title of the web page")
        .build();

    let description_property = Property::builder(fields::DESCRIPTION, vec!["text"])
        .with_description("The meta description of the web page")
        .build();

    let content_property = Property::builder(fields::CONTENT, vec!["text"])
        .with_description("The main text content of the web page")
        .build();

    let crawled_at_property = Property::builder(fields::CRAWLED_AT, vec!["int"])
        .with_description("Unix timestamp when the page was crawled")
        .build();

    let webpage_class = Class::builder(WEAVIATE_CLASS_NAME)
        .with_description("Web pages crawled from the internet")
        .with_vectorizer("text2vec-ollama")
        .with_module_config(serde_json::json!({
            "text2vec-ollama": {
                "apiEndpoint": "http://ollama:11434",
                "model": "embeddinggemma"
            }
        }))
        .with_properties(Properties::new(vec![
            url_property,
            title_property,
            description_property,
            content_property,
            crawled_at_property,
        ]))
        .build();

    match client.schema.create_class(&webpage_class).await {
        Ok(_) => {
            info!("Successfully created '{}' schema", WEAVIATE_CLASS_NAME);
            Ok(())
        }
        Err(e) => {
            error!("Failed to create schema: {}", e);
            Err(anyhow::anyhow!("Schema creation error: {}", e))
        }
    }
}

/// Index a web page into Weaviate using a provided client
pub async fn index_page_with_client(
    client: &WeaviateClient,
    url: String,
    html_content: String,
    sub_pages: Vec<String>,
) -> Result<()> {
    // Extract structured data from HTML
    let page_data = extract_webpage_data(url.clone(), html_content, sub_pages);

    info!(
        "Indexing page: {} (title: {})",
        page_data.url, page_data.title
    );

    // Prepare the object data using shared types method
    let properties = page_data.to_properties_json();

    // Insert into Weaviate
    match client
        .objects
        .create(
            &Object::builder(WEAVIATE_CLASS_NAME, properties).build(),
            None,
        )
        .await
    {
        Ok(response) => {
            info!("Successfully indexed page: {} (ID: {:?})", url, response.id);
            Ok(())
        }
        Err(e) => {
            error!("Failed to index page {}: {}", url, e);
            Err(anyhow::anyhow!("Weaviate indexing error: {}", e))
        }
    }
}

/// Index a web page into Weaviate (creates a new client - use sparingly)
pub async fn index_page(url: String, html_content: String, sub_pages: Vec<String>) -> Result<()> {
    // Create Weaviate client
    let client = create_weaviate_client()?;

    // Ensure schema exists
    if let Err(e) = ensure_schema(&client).await {
        warn!("Schema check warning: {}", e);
    }

    // Index the page
    index_page_with_client(&client, url, html_content, sub_pages).await
}

/// Index a page without blocking the crawler on failure
pub async fn index_page_safe(url: String, html_content: String, sub_pages: Vec<String>) {
    if let Err(e) = index_page(url.clone(), html_content, sub_pages).await {
        error!("Error indexing page {}: {}", url, e);
    }
}

/// Index a page with an existing client without blocking on failure
pub async fn index_page_safe_with_client(
    client: &WeaviateClient,
    url: String,
    html_content: String,
    sub_pages: Vec<String>,
) {
    if let Err(e) = index_page_with_client(client, url.clone(), html_content, sub_pages).await {
        error!("Error indexing page {}: {}", url, e);
    }
}
