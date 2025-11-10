use anyhow::Result;
use scraper::{ElementRef, Html, Selector};

use shared_crawler_api::{fields, WebPageChunk, WEAVIATE_CLASS_NAME};
use std::env;
use tracing::{error, info, warn};
use uuid::Uuid;
use weaviate_community::collections::objects::Object;
use weaviate_community::collections::schema::{Class, Properties, Property};
use weaviate_community::WeaviateClient;

use crate::extractor::{extract_description, extract_title};

const MIN_CHUNK_TOKENS: usize = 300;
const MAX_CHUNK_TOKENS: usize = 700;

/// Estimate token count from text (rough approximation: 1 token ≈ 0.75 words)
fn estimate_tokens(text: &str) -> usize {
    let word_count = text.split_whitespace().count();
    (word_count as f64 * 1.33) as usize // Inverse of 0.75
}

/// Represents a content block with optional heading
#[derive(Debug, Clone)]
pub struct ContentBlock {
    pub heading: Option<String>,
    pub text: String,
}

/// Extract structured content blocks from HTML
fn extract_content_blocks(document: &Html) -> Vec<ContentBlock> {
    let mut blocks = Vec::new();

    // Remove script and style tags
    let script_selector = Selector::parse("script, style, nav, header, footer").unwrap();

    // Try to get main content areas
    let content_selectors = vec![
        "article", "main", ".content", "#content", ".post", ".article", "body",
    ];

    let mut found_content = false;

    for selector_str in content_selectors {
        if let Ok(selector) = Selector::parse(selector_str) {
            if let Some(element) = document.select(&selector).next() {
                blocks = extract_blocks_from_element(element, &script_selector);
                if !blocks.is_empty() {
                    found_content = true;
                    break;
                }
            }
        }
    }

    // Fallback: extract headings and paragraphs
    if !found_content {
        // Extract all headings and paragraphs in document order
        let body_selector = Selector::parse("body").unwrap();
        if let Some(body) = document.select(&body_selector).next() {
            blocks = extract_blocks_from_element(body, &script_selector);
        }
    }

    blocks
}

/// Extract content blocks from an HTML element
fn extract_blocks_from_element(
    element: ElementRef,
    exclude_selector: &Selector,
) -> Vec<ContentBlock> {
    let mut blocks = Vec::new();
    let mut current_heading: Option<String> = None;
    // println!("extracting content blocks for {}", element.value().name());
    // println!("skipping: {:?}", exclude_selector);

    // Process children in order
    for child in element.children() {
        if let Some(child_element) = ElementRef::wrap(child) {
            let tag_name = child_element.value().name();

            // Skip excluded elements
            // if child_element.select(exclude_selector).next().is_some() {
            //     println!("Skipping excluded element: {}", tag_name);
            //     println!("Because it's excluded: {:?}", child_element);
            //     continue;
            // }

            // headings
            if tag_name.starts_with('h') && tag_name.len() == 2 {
                let heading_text = child_element.text().collect::<String>().trim().to_string();
                if !heading_text.is_empty() {
                    current_heading = Some(heading_text);
                }
            }
            // paragraph
            else if tag_name == "p" {
                let text = child_element.text().collect::<String>().trim().to_string();
                if !text.is_empty() {
                    blocks.push(ContentBlock {
                        heading: current_heading.clone(),
                        text,
                    });
                }
            }
            // Check if it's a container element, recurse
            else if tag_name == "div"
                || tag_name == "section"
                || tag_name == "article"
                || tag_name == "main"
            {
                let sub_blocks = extract_blocks_from_element(child_element, exclude_selector);
                blocks.extend(sub_blocks);
            } else if tag_name == "ul" || tag_name == "ol" {
                let sub_blocks = extract_blocks_from_element(child_element, exclude_selector);
                blocks.extend(sub_blocks);
            } else if tag_name == "table" {
                let sub_blocks = extract_blocks_from_element(child_element, exclude_selector);
                blocks.extend(sub_blocks);
            } else if tag_name == "pre" {
                let sub_blocks = extract_blocks_from_element(child_element, exclude_selector);
                blocks.extend(sub_blocks);
            } else if tag_name == "blockquote" {
                let sub_blocks = extract_blocks_from_element(child_element, exclude_selector);
                blocks.extend(sub_blocks);
            } else {
                let text = child_element.text().collect::<String>().trim().to_string();
                if !text.is_empty() {
                    blocks.push(ContentBlock {
                        heading: current_heading.clone(),
                        text,
                    });
                }
            }
        }
    }

    blocks
}

/// Split content blocks into chunks of appropriate token size
fn create_chunks(
    blocks: Vec<ContentBlock>,
    url: &str,
    title: &str,
    description: &str,
    crawled_at: i64,
) -> Vec<WebPageChunk> {
    let mut chunks = Vec::new();
    let mut current_chunk_text = String::new();
    let mut current_heading: Option<String> = None;
    let mut current_tokens = 0;

    for block in blocks {
        let block_tokens = estimate_tokens(&block.text);

        // If this block alone exceeds MAX_CHUNK_TOKENS, split it
        if block_tokens > MAX_CHUNK_TOKENS {
            // First, save any accumulated content
            if !current_chunk_text.is_empty() {
                chunks.push(WebPageChunk::new(
                    current_chunk_text.trim().to_string(),
                    current_heading.clone(),
                    url.to_string(),
                    title.to_string(),
                    description.to_string(),
                    crawled_at,
                ));
                current_chunk_text.clear();
                current_tokens = 0;
            }

            // Split the large block into sentences
            let sentences = split_into_sentences(&block.text);
            let mut sentence_chunk = String::new();
            let mut sentence_tokens = 0;

            for sentence in sentences {
                let sentence_tokens_count = estimate_tokens(&sentence);

                if sentence_tokens + sentence_tokens_count > MAX_CHUNK_TOKENS
                    && !sentence_chunk.is_empty()
                {
                    chunks.push(WebPageChunk::new(
                        sentence_chunk.trim().to_string(),
                        block.heading.clone(),
                        url.to_string(),
                        title.to_string(),
                        description.to_string(),
                        crawled_at,
                    ));
                    sentence_chunk.clear();
                    sentence_tokens = 0;
                }

                sentence_chunk.push_str(&sentence);
                sentence_chunk.push(' ');
                sentence_tokens += sentence_tokens_count;
            }

            if !sentence_chunk.is_empty() {
                chunks.push(WebPageChunk::new(
                    sentence_chunk.trim().to_string(),
                    block.heading.clone(),
                    url.to_string(),
                    title.to_string(),
                    description.to_string(),
                    crawled_at,
                ));
            }

            current_heading = block.heading;
            continue;
        }

        // Check if adding this block would exceed MAX_CHUNK_TOKENS
        if current_tokens + block_tokens > MAX_CHUNK_TOKENS {
            // If there's already accumulated content, flush it to start a new chunk.
            // Previously we only flushed when the accumulated chunk met the MIN_CHUNK_TOKENS,
            // which could cause us to append a block and exceed MAX_CHUNK_TOKENS.
            if !current_chunk_text.is_empty() && current_tokens > 0 {
                chunks.push(WebPageChunk::new(
                    current_chunk_text.trim().to_string(),
                    current_heading.clone(),
                    url.to_string(),
                    title.to_string(),
                    description.to_string(),
                    crawled_at,
                ));
                current_chunk_text.clear();
                current_tokens = 0;
            }
        }

        // Update heading if this block has one
        if block.heading.is_some() {
            current_heading = block.heading.clone();
        }

        // Add block to current chunk
        if !current_chunk_text.is_empty() {
            current_chunk_text.push(' ');
        }
        current_chunk_text.push_str(&block.text);
        current_tokens += block_tokens;

        // If we've reached a good chunk size, save it
        if current_tokens >= MIN_CHUNK_TOKENS {
            chunks.push(WebPageChunk::new(
                current_chunk_text.trim().to_string(),
                current_heading.clone(),
                url.to_string(),
                title.to_string(),
                description.to_string(),
                crawled_at,
            ));
            current_chunk_text.clear();
            current_tokens = 0;
        }
    }

    // Save any remaining content
    if !current_chunk_text.is_empty() {
        chunks.push(WebPageChunk::new(
            current_chunk_text.trim().to_string(),
            current_heading,
            url.to_string(),
            title.to_string(),
            description.to_string(),
            crawled_at,
        ));
    }
    chunks
}

/// Split text into sentences
fn split_into_sentences(text: &str) -> Vec<String> {
    let mut sentences = Vec::new();
    let mut current_sentence = String::new();

    for c in text.chars() {
        current_sentence.push(c);

        if c == '.' || c == '!' || c == '?' {
            // Check if next char is whitespace or end of string
            sentences.push(current_sentence.trim().to_string());
            current_sentence.clear();
        }
    }

    if !current_sentence.trim().is_empty() {
        sentences.push(current_sentence.trim().to_string());
    }

    sentences
}

/// Extract structured data from HTML content and return chunks
pub fn extract_webpage_data(url: String, html_content: String) -> Vec<WebPageChunk> {
    let document = Html::parse_document(&html_content);

    let title = extract_title(&document);

    let content_blocks = extract_content_blocks(&document);
    // println!("content blocks: {:?}", content_blocks);

    // Generate description from first few blocks if not in meta tags
    let description = extract_description(&document, &content_blocks);

    let crawled_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    let chunks = create_chunks(content_blocks, &url, &title, &description, crawled_at);

    // If no chunks were created, create a minimal one
    if chunks.is_empty() {
        vec![WebPageChunk::new(
            "".to_string(),
            None,
            url,
            title,
            description,
            crawled_at,
        )]
    } else {
        chunks
    }
}

/// Initialize Weaviate client
/// don't delete even if ide thinks it's unused
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
/// TODO: improve this code, maybe with loops through properties
pub async fn ensure_schema(client: &WeaviateClient) -> Result<()> {
    info!(
        "Checking if '{}' class exists in Weaviate",
        WEAVIATE_CLASS_NAME
    );

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

    // Create the WebPage class with properties using new field names
    let chunk_content_property = Property::builder(fields::CHUNK_CONTENT, vec!["text"])
        .with_description("The content of this chunk")
        .build();

    let chunk_heading_property = Property::builder(fields::CHUNK_HEADING, vec!["text"])
        .with_description("The heading context for this chunk")
        .build();

    let source_url_property = Property::builder(fields::SOURCE_URL, vec!["text"])
        .with_description("The source URL of the web page")
        .build();

    let page_title_property = Property::builder(fields::PAGE_TITLE, vec!["text"])
        .with_description("The title of the web page")
        .build();

    let description_property = Property::builder(fields::DESCRIPTION, vec!["text"])
        .with_description("The meta description of the web page")
        .build();

    let crawled_at_property = Property::builder(fields::CRAWLED_AT, vec!["int"])
        .with_description("Unix timestamp when the page was crawled")
        .build();

    let webpage_class = Class::builder(WEAVIATE_CLASS_NAME)
        .with_description("Web page chunks crawled from the internet")
        .with_vectorizer("text2vec-ollama")
        .with_module_config(serde_json::json!({
            "text2vec-ollama": {
                "apiEndpoint": "http://ollama:11434",
                "model": "embeddinggemma"
            }
        }))
        .with_properties(Properties::new(vec![
            chunk_content_property,
            chunk_heading_property,
            source_url_property,
            page_title_property,
            description_property,
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

/// Generate a deterministic UUID v5 from a URL and chunk index
fn generate_uuid_from_url_and_chunk(url: &str, chunk_index: usize) -> Uuid {
    // Use UUID v5 with a namespace to generate deterministic UUIDs from URLs
    // Using the URL namespace (6ba7b811-9dad-11d1-80b4-00c04fd430c8)
    const URL_NAMESPACE: Uuid = Uuid::from_bytes([
        0x6b, 0xa7, 0xb8, 0x11, 0x9d, 0xad, 0x11, 0xd1, 0x80, 0xb4, 0x00, 0xc0, 0x4f, 0xd4, 0x30,
        0xc8,
    ]);

    let url_with_chunk = format!("{}#chunk{}", url, chunk_index);
    Uuid::new_v5(&URL_NAMESPACE, url_with_chunk.as_bytes())
}

/// Index a web page into Weaviate using a provided client
pub async fn index_page_with_client(
    client: &WeaviateClient,
    url: String,
    html_content: String,
) -> Result<()> {
    let chunks = extract_webpage_data(url.clone(), html_content);

    info!("Indexing page: {} ({} chunks)", url, chunks.len());

    for (index, chunk) in chunks.iter().enumerate() {
        let object_id = generate_uuid_from_url_and_chunk(&url, index);

        match client
            .objects
            .delete(WEAVIATE_CLASS_NAME, &object_id, None, None)
            .await
        {
            Ok(_) => {
                info!("Deleted existing chunk {} for URL: {}", index, url);
            }
            Err(_) => {
                // Object doesn't exist -> skip deletion
            }
        }

        let properties = chunk.to_properties_json();

        // Insert into Weaviate with deterministic UUID
        match client
            .objects
            .create(
                &Object::builder(WEAVIATE_CLASS_NAME, properties)
                    .with_id(object_id)
                    .build(),
                None,
            )
            .await
        {
            Ok(response) => {
                info!(
                    "Successfully indexed chunk {} for page: {} (ID: {:?})",
                    index, url, response.id
                );
            }
            Err(e) => {
                error!("Failed to index chunk {} for page {}: {}", index, url, e);
                return Err(anyhow::anyhow!("Weaviate indexing error: {}", e));
            }
        }
    }

    info!(
        "Successfully indexed all {} chunks for page: {}",
        chunks.len(),
        url
    );
    Ok(())
}

/// Index a web page into Weaviate (creates a new client - use sparingly)
pub async fn index_page(url: String, html_content: String) -> Result<()> {
    let client = create_weaviate_client()?;

    if let Err(e) = ensure_schema(&client).await {
        warn!("Schema check warning: {}", e);
    }
    index_page_with_client(&client, url, html_content).await
}

/// Index a page without blocking the crawler on failure
pub async fn index_page_safe(url: String, html_content: String) {
    if let Err(e) = index_page(url.clone(), html_content).await {
        error!("Error indexing page {}: {}", url, e);
    }
}

/// Index a page with an existing client without blocking on failure
pub async fn index_page_safe_with_client(
    client: &WeaviateClient,
    url: String,
    html_content: String,
) {
    if let Err(e) = index_page_with_client(client, url.clone(), html_content).await {
        error!("Error indexing page {}: {}", url, e);
    }
}
