use scraper::{ElementRef, Html, Selector};

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

    let desc_tag_priority = Selector::parse("p, h3, h2, h1, div").unwrap();
    let desc_tag_exclude = Selector::parse("script, style, nav, footer, aside, header").unwrap();

    let description =
        get_content_by_priority_tag(document, &desc_tag_priority, &desc_tag_exclude, 100);

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
    prefered_tags: &Selector,
    exclude_selector: &Selector,
    min_content_length: usize,
) -> String {
    let mut collected: Vec<String> = Vec::new();
    let mut accumulated_len: usize = 0;

    // document.select yields elements in document order, so we preserve appearance order
    for el in document.select(&prefered_tags) {
        let text = el.text().collect::<String>().trim().to_string();
        if text.is_empty() {
            continue;
        }

        // Skip if element is inside an excluded ancestor
        let mut in_excluded = false;
        for ancestor in el.ancestors() {
            if let Some(anc_el) = ElementRef::wrap(ancestor) {
                if exclude_selector.matches(&anc_el) {
                    in_excluded = true;
                    break;
                }
            }
        }
        if in_excluded {
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
        let prio_tags = Selector::parse("h1, p").unwrap();
        let exclude_selector = Selector::parse("doesnotexist").unwrap();
        let content_blocks =
            get_content_by_priority_tag(&document, &prio_tags, &exclude_selector, 25);
        assert_eq!(content_blocks, "This is a heading\nThis is some content");
    }

    #[test]
    fn test_get_content_by_priority_tag_with_nested_tags() {
        let html = r#"
            <html>
                <head>
                    <title>Test Page</title>
                </head>
                <body>
                    <h1>This is a heading</h1>
                    <nav>
                        <ul>
                            <li>Item 1</li>
                            <li>Item 2</li>
                        </ul>
                        <p>
                            This should not be included
                        </p>
                    </nav>
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
        let prio_tags = Selector::parse("h1, p").unwrap();
        let exclude_selector = Selector::parse("nav").unwrap();
        let content_blocks =
            get_content_by_priority_tag(&document, &prio_tags, &exclude_selector, 100);
        assert_eq!(content_blocks, "This is a heading\nThis is some content");
    }
}
