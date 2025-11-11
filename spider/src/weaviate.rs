use anyhow::Result;

use shared_crawler_api::{fields, WEAVIATE_CLASS_NAME};
use std::env;
use tracing::{error, info, warn};
use uuid::Uuid;
use weaviate_community::collections::objects::Object;
use weaviate_community::collections::schema::{Class, Properties, Property};
use weaviate_community::WeaviateClient;

use crate::index::extract_webpage_data;

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
