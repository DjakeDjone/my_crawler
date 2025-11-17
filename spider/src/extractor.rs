use scraper::{Html, Selector};
use shared_crawler_api::WebPageChunk;

use crate::index::ContentBlock;

pub fn extract_title(document: &Html) -> String {
    let title_selector = Selector::parse("title").unwrap();
    document
        .select(&title_selector)
        .next()
        .map(|el| el.text().collect::<String>().trim().to_string())
        .unwrap_or_else(|| "".to_string())
}

pub fn extract_description(document: &Html, content_blocks: &Vec<ContentBlock>) -> String {
    let max_length = 247;
    let meta_selector = Selector::parse("meta[name='description']").unwrap();
    if let Some(meta) = document.select(&meta_selector).next() {
        if let Some(content) = meta.value().attr("content") {
            let desc = content.trim().to_string();
            if !desc.is_empty() {
                return desc;
            }
        }
    }

    // Try og:description as fallback
    let og_desc_selector = Selector::parse("meta[property='og:description']").unwrap();
    if let Some(meta) = document.select(&og_desc_selector).next() {
        if let Some(content) = meta.value().attr("content") {
            let desc = content.trim().to_string();
            if !desc.is_empty() {
                return desc;
            }
        }
    }

    let description = content_blocks
        .iter()
        .map(|f| {
            if f.heading.is_none() {
                f.text.clone()
            } else if f.text.is_empty() {
                f.heading.clone().unwrap()
            } else {
                format!("{}: {}", f.heading.clone().unwrap(), f.text)
            }
        })
        .collect::<Vec<String>>()
        .join("\n");

    let mut result = description
        .trim()
        .to_string()
        .chars()
        .take(max_length)
        .collect::<String>();

    if result.len() >= max_length - 1 {
        result.push_str("...");
    }
    if result.is_empty() {
        "".to_string()
    } else {
        result
    }
}

pub fn calculate_chunk_score(chunk: &WebPageChunk) -> f64 {
    let mut score = 1.0;
    // increase score if page has description
    if chunk.description.len() > 5 {
        score += 1.0;
    }
    let prio_urls = ["wikipedia", "youtube", "reddit"];
    if prio_urls.iter().any(|url| chunk.source_url.contains(url)) {
        score += 1.0;
    }
    if chunk.source_url.contains("php") {
        score -= 1.0;
    }

    score
}

#[cfg(test)]
mod tests {
    use crate::extractor_content::extract_content_blocks;

    use super::*;

    #[test]
    fn test_extract_description() {
        let html = r#"
            <html>
                <head>
                    <title>Test Page</title>
                    <meta name="description" content="This is a test page">
                    <meta property="og:description" content="This is an Open Graph test page">
                </head>
                <body>
                    <p>This is some content</p>
                    <p>This is some more content</p>
                </body>
            </html>
        "#;

        let document = Html::parse_document(html);

        let content_blocks = extract_content_blocks(&document);

        assert_eq!(
            extract_description(&document, &content_blocks),
            "This is a test page"
        );
    }
}
