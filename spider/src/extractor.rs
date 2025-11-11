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

pub fn extract_description(document: &Html) -> String {
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

    let desc_tag_priority = ["p", "h3", "h2", "h1", "div"];

    let description = get_content_by_priority_tag(document, &desc_tag_priority.to_vec(), 100);

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
        "No description available".to_string()
    } else {
        result
    }
}

pub fn get_content_by_priority_tag(
    document: &Html,
    prefered_tags: &Vec<&str>,
    min_content_length: usize,
) -> String {
    // important: keep the order
    if prefered_tags.is_empty() {
        return "".to_string();
    }

    // Build a selector that matches any of the preferred tags, e.g. "p, h1"
    let selector_str = prefered_tags.join(", ");
    let selector = match Selector::parse(&selector_str) {
        Ok(s) => s,
        Err(_) => return "".to_string(),
    };

    let mut collected: Vec<String> = Vec::new();
    let mut accumulated_len: usize = 0;

    // document.select yields elements in document order, so we preserve appearance order
    for el in document.select(&selector) {
        let text = el.text().collect::<String>().trim().to_string();
        if text.is_empty() {
            continue;
        }

        // Add the text block
        collected.push(text.clone());
        accumulated_len += text.len();

        // Stop once we've reached the requested minimum content length
        if accumulated_len >= min_content_length {
            break;
        }
    }

    collected.join("\n")
}

#[cfg(test)]
mod tests {
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

        assert_eq!(extract_description(&document), "This is a test page");
    }

    #[test]
    fn test_get_content_by_priority_tag() {
        let html = r#"
            <html>
                <head>
                    <title>Test Page</title>
                </head>
                <body>
                    <h1>This is a heading</h1>
                    <pre lang="rust">
                        <code>
                            println!("Hello, world!");
                        </code>
                    </pre>
                    <p>This is some content</p>
                </body>
            </html>
        "#;

        let document = Html::parse_document(html);
        let prio_tags = vec!["p", "h1"];
        let content_blocks = get_content_by_priority_tag(&document, &prio_tags, 25);
        assert_eq!(content_blocks, "This is a heading\nThis is some content");
    }
}
