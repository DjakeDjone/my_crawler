use scraper::{Html, Selector};

use crate::index::ContentBlock;

pub fn extract_title(document: &Html) -> String {
    let title_selector = Selector::parse("title").unwrap();
    document
        .select(&title_selector)
        .next()
        .map(|el| el.text().collect::<String>().trim().to_string())
        .unwrap_or_else(|| "".to_string())
}

pub fn extract_description(document: &Html, content_blocks: &[ContentBlock]) -> String {
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

    // Generate from content blocks
    let mut description = String::new();
    let mut word_count = 0;

    for block in content_blocks.iter().take(5) {
        for word in block.text.split_whitespace() {
            if word_count >= 100 {
                break;
            }
            description.push_str(word);
            description.push(' ');
            word_count += 1;
        }
        if word_count >= 100 {
            break;
        }
    }

    let result = description.trim().to_string();
    if result.is_empty() {
        "No description available".to_string()
    } else {
        result
    }
}
