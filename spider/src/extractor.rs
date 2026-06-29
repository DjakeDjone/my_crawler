use scraper::{Html, Selector};

use crate::index::ContentBlock;

pub fn extract_title(document: &Html) -> String {
    let title_selector = Selector::parse("title").unwrap();
    document
        .select(&title_selector)
        .next()
        .map(|el| el.text().collect::<String>().trim().to_string())
        .unwrap_or_default()
}

pub fn extract_description(document: &Html, content_blocks: &[ContentBlock]) -> String {
    let max_length = 247;
    let min_block_length = 20;
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
        .map(|block| clean_description_text(&block.text))
        .filter(|text| text.chars().count() >= min_block_length)
        .collect::<Vec<_>>()
        .join(" ");

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

fn clean_description_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
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

    #[test]
    fn fallback_description_skips_headings_and_short_noise() {
        let blocks = vec![
            ContentBlock {
                heading: Some("WP:ABT".into()),
                text: "Short".into(),
            },
            ContentBlock {
                heading: Some("Wikipedia:About".into()),
                text: "Wikipedia is a free online encyclopedia that anyone can edit.".into(),
            },
        ];
        let document = Html::parse_document("<html></html>");

        assert_eq!(
            extract_description(&document, &blocks),
            "Wikipedia is a free online encyclopedia that anyone can edit."
        );
    }

    #[test]
    fn fallback_description_truncates() {
        let blocks = vec![ContentBlock {
            heading: None,
            text: "word ".repeat(100),
        }];
        let document = Html::parse_document("<html></html>");
        let description = extract_description(&document, &blocks);

        assert!(description.ends_with("..."));
        assert!(description.len() > 247);
    }
}
