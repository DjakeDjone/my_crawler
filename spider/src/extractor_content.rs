use scraper::{ElementRef, Html, Selector};

use crate::index::ContentBlock;

/// Extract structured content blocks from HTML
pub fn extract_content_blocks(document: &Html) -> Vec<ContentBlock> {
    let mut blocks = Vec::new();

    // Remove script and style tags
    let script_selector = Selector::parse("script, style").unwrap();

    // Try to get main content areas
    let content_selectors = vec![
        "article", "main", ".content", "#content", ".post", ".article", "body",
    ];

    let mut found_content = false;

    for selector_str in content_selectors {
        if let Ok(selector) = Selector::parse(selector_str) {
            if let Some(element) = document.select(&selector).next() {
                blocks = extract_blocks_from_element(element, &script_selector, None);
                if !blocks.is_empty() {
                    found_content = true;
                    break;
                }
            }
        }
    }

    // Fallback: use the document root (covers HTML fragments without a <body>)
    if !found_content {
        let root = document.root_element();
        blocks = extract_blocks_from_element(root, &script_selector, None);
    }

    blocks
}

/// Extract content blocks from an HTML element
pub fn extract_blocks_from_element(
    element: ElementRef,
    exclude_selector: &Selector,
    parent_heading: Option<String>,
) -> Vec<ContentBlock> {
    let mut blocks = Vec::new();
    let mut current_heading: Option<String> = parent_heading.clone();

    // Process children in order
    for child in element.children() {
        if let Some(child_element) = ElementRef::wrap(child) {
            let tag_name = child_element.value().name();

            // Skip excluded elements
            if child_element.select(exclude_selector).next().is_some() {
                continue;
            }

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
            // Check if it's a container element, recurse (propagate current heading)
            else if tag_name == "div"
                || tag_name == "section"
                || tag_name == "article"
                || tag_name == "main"
            {
                let sub_blocks = extract_blocks_from_element(
                    child_element,
                    exclude_selector,
                    current_heading.clone(),
                );
                blocks.extend(sub_blocks);
            } else if tag_name == "ul" || tag_name == "ol" {
                let sub_blocks = extract_blocks_from_element(
                    child_element,
                    exclude_selector,
                    current_heading.clone(),
                );
                blocks.extend(sub_blocks);
            } else if tag_name == "table" {
                let sub_blocks = extract_blocks_from_element(
                    child_element,
                    exclude_selector,
                    current_heading.clone(),
                );
                blocks.extend(sub_blocks);
            } else if tag_name == "pre" {
                let sub_blocks = extract_blocks_from_element(
                    child_element,
                    exclude_selector,
                    current_heading.clone(),
                );
                blocks.extend(sub_blocks);
            } else if tag_name == "blockquote" {
                let sub_blocks = extract_blocks_from_element(
                    child_element,
                    exclude_selector,
                    current_heading.clone(),
                );
                blocks.extend(sub_blocks);
            }
            // else {
            //     let text = child_element.text().collect::<String>().trim().to_string();
            //     if !text.is_empty() {
            //         blocks.push(ContentBlock {
            //             heading: current_heading.clone(),
            //             text,
            //         });
            //     }
            // }
        }
    }

    blocks
}

// tests
//

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_content_blocks() {
        let html = r#"
                <div>
                    <h1>Title</h1>
                    <p>Paragraph 1</p>
                    <p>Paragraph 2</p>
                </div>
            "#;
        let document = Html::parse_document(html);
        let blocks = extract_content_blocks(&document);
        println!("{:?}", blocks);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].heading.as_deref().unwrap_or(""), "Title");
        assert_eq!(blocks[0].text, "Paragraph 1");
        assert_eq!(blocks[1].heading.as_deref().unwrap_or(""), "Title");
        assert_eq!(blocks[1].text, "Paragraph 2");
    }

    #[test]
    fn test_extract_content_blocks_with_multiple_headings() {
        let html = r#"
                <div>
                    <h1>Title 1</h1>
                    <p>Paragraph 1</p>
                    <h2>Title 2</h2>
                    <p>Paragraph 2</p>
                </div>
            "#;
        let document = Html::parse_document(html);
        let blocks = extract_content_blocks(&document);
        println!("{:?}", blocks);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].heading.as_deref().unwrap_or(""), "Title 1");
        assert_eq!(blocks[0].text, "Paragraph 1");
        assert_eq!(blocks[1].heading.as_deref().unwrap_or(""), "Title 2");
        assert_eq!(blocks[1].text, "Paragraph 2");
    }

    #[test]
    fn test_extract_content_blocks_ignore_nav() {
        let html = r##"
                <div>
                    <h1>Title</h1>
                    <p>Paragraph 1</p>
                    <nav>
                        <a href="#">Link 1</a>
                        <a href="#">Link 2</a>
                        <p>Paragraph 3</p>
                    </nav>
                    <p>Paragraph 2</p>
                </div>
            "##;
        let document = Html::parse_document(html);
        let blocks = extract_content_blocks(&document);
        println!("{:?}", blocks);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].heading.as_deref().unwrap_or(""), "Title");
        assert_eq!(blocks[0].text, "Paragraph 1");
        assert_eq!(blocks[1].heading.as_deref().unwrap_or(""), "Title");
        assert_eq!(blocks[1].text, "Paragraph 2");
    }

    #[test]
    fn test_extract_content_blocks_nested_divs() {
        let html = r##"
                <div>
                    <h1>Title</h1>
                    <p>Paragraph 1</p>
                    <nav>
                        <a href="#">Link 1</a>
                        <a href="#">Link 2</a>
                        <p>Paragraph 3</p>
                    </nav>
                    <div>
                        <p>Paragraph 2</p>
                        <div class="nested">
                            <h2>Nested Heading</h2>
                            <p>Paragraph 3</p>
                        </div>
                    </div>
                </div>
            "##;
        let document = Html::parse_document(html);
        let blocks = extract_content_blocks(&document);
        println!("{:?}", blocks);
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[0].heading.as_deref().unwrap_or(""), "Title");
        assert_eq!(blocks[0].text, "Paragraph 1");
        assert_eq!(blocks[1].heading.as_deref().unwrap_or(""), "Title");
        assert_eq!(blocks[1].text, "Paragraph 2");
        assert_eq!(blocks[2].heading.as_deref().unwrap_or(""), "Nested Heading");
        assert_eq!(blocks[2].text, "Paragraph 3");
    }
}
