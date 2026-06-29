use scraper::Html;
use shared_crawler_api::WebPageChunk;
use url::Url;

use crate::{
    extractor::{extract_description, extract_title},
    extractor_content::extract_content_blocks,
    web_visitor::extract_links,
};

const TARGET_CHARS: usize = 800;
const MAX_CHARS: usize = 1_200;
const UNBROKEN_CHARS: usize = 450;

#[derive(Debug, Clone)]
pub struct ContentBlock {
    pub heading: Option<String>,
    pub text: String,
}

pub struct ExtractedPage {
    pub chunks: Vec<WebPageChunk>,
    pub links: Vec<Url>,
}

pub fn extract_page(url: &Url, html: &str) -> ExtractedPage {
    let document = Html::parse_document(html);
    let title = extract_title(&document);
    let blocks = extract_content_blocks(&document);
    let description = extract_description(&document, &blocks);
    let crawled_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let chunks = create_chunks(blocks, url.as_str(), &title, &description, crawled_at);
    ExtractedPage {
        chunks,
        links: extract_links(&document, url),
    }
}

fn create_chunks(
    blocks: Vec<ContentBlock>,
    url: &str,
    title: &str,
    description: &str,
    crawled_at: i64,
) -> Vec<WebPageChunk> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut heading = None;

    for block in blocks {
        for piece in split_text(&block.text) {
            let added = piece.chars().count() + usize::from(!current.is_empty());
            if !current.is_empty() && current.chars().count() + added > MAX_CHARS {
                push_chunk(
                    &mut chunks,
                    std::mem::take(&mut current),
                    heading.clone(),
                    url,
                    title,
                    description,
                    crawled_at,
                );
            }
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(&piece);
            heading = block.heading.clone().or(heading);
            if current.chars().count() >= TARGET_CHARS {
                push_chunk(
                    &mut chunks,
                    std::mem::take(&mut current),
                    heading.clone(),
                    url,
                    title,
                    description,
                    crawled_at,
                );
            }
        }
    }
    if !current.trim().is_empty() {
        push_chunk(
            &mut chunks,
            current,
            heading,
            url,
            title,
            description,
            crawled_at,
        );
    }
    chunks
}

fn split_text(text: &str) -> Vec<String> {
    if text.chars().count() <= MAX_CHARS {
        return vec![text.to_string()];
    }
    let chars = text.chars().collect::<Vec<_>>();
    let mut pieces = Vec::new();
    let mut start = 0;
    while start < chars.len() {
        let max_end = (start + MAX_CHARS).min(chars.len());
        let end = if max_end == chars.len() {
            max_end
        } else {
            (start..max_end)
                .rev()
                .find(|index| chars[*index].is_whitespace())
                .filter(|index| *index > start)
                .unwrap_or((start + UNBROKEN_CHARS).min(chars.len()))
        };
        pieces.push(
            chars[start..end]
                .iter()
                .collect::<String>()
                .trim()
                .to_string(),
        );
        start = end;
        while start < chars.len() && chars[start].is_whitespace() {
            start += 1;
        }
    }
    pieces
}

#[allow(clippy::too_many_arguments)]
fn push_chunk(
    chunks: &mut Vec<WebPageChunk>,
    content: String,
    heading: Option<String>,
    url: &str,
    title: &str,
    description: &str,
    crawled_at: i64,
) {
    if content.trim().is_empty() {
        return;
    }
    chunks.push(WebPageChunk::new(
        content.trim().to_string(),
        heading,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_latin_arabic_and_cjk_by_characters() {
        for text in [
            "word ".repeat(500),
            "مرحبا".repeat(300),
            "你好世界".repeat(400),
        ] {
            let chunks = create_chunks(
                vec![ContentBlock {
                    heading: None,
                    text,
                }],
                "https://example.com",
                "title",
                "",
                0,
            );
            assert!(!chunks.is_empty());
            assert!(chunks
                .iter()
                .all(|chunk| chunk.chunk_content.chars().count() <= MAX_CHARS));
        }
    }

    #[test]
    fn unbroken_text_uses_small_splits() {
        let pieces = split_text(&"界".repeat(1300));
        assert_eq!(pieces[0].chars().count(), UNBROKEN_CHARS);
    }

    #[test]
    fn keeps_fetched_url_when_canonical_points_to_root() {
        let base = Url::parse("https://example.com/subpage").unwrap();
        let page = extract_page(&base, r#"<link rel="canonical" href="/"><p>content</p>"#);
        assert_eq!(page.chunks[0].source_url, "https://example.com/subpage");
    }
}
