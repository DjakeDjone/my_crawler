use scraper::Html;

use shared_crawler_api::WebPageChunk;

use crate::{
    extractor::{extract_description, extract_title},
    extractor_content::extract_content_blocks,
};

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
                    Vec::new(),
                    Vec::new(),
                    0.0,
                    0.0,
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
                        Vec::new(),
                        Vec::new(),
                        0.0,
                        0.0,
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
                    Vec::new(),
                    Vec::new(),
                    0.0,
                    0.0,
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
                    Vec::new(),
                    Vec::new(),
                    0.0,
                    0.0,
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
                Vec::new(),
                Vec::new(),
                0.0,
                0.0,
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
            Vec::new(),
            Vec::new(),
            0.0,
            0.0,
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
            Vec::new(),
            Vec::new(),
            0.0,
            0.0,
            crawled_at,
        )]
    } else {
        chunks
    }
}
