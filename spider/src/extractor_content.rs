use scraper::{ElementRef, Html};

use crate::index::ContentBlock;

pub fn extract_content_blocks(document: &Html) -> Vec<ContentBlock> {
    walk(document.root_element(), None).0
}

fn walk(
    element: ElementRef<'_>,
    mut heading: Option<String>,
) -> (Vec<ContentBlock>, Option<String>) {
    let mut blocks = Vec::new();
    for child in element.children().filter_map(ElementRef::wrap) {
        let name = child.value().name();
        if is_excluded(&child) {
            continue;
        }
        if matches!(name, "h1" | "h2" | "h3" | "h4" | "h5" | "h6") {
            let text = clean_text(&child);
            if !text.is_empty() {
                heading = Some(text);
            }
        } else if matches!(name, "p" | "li" | "pre" | "blockquote" | "td" | "th") {
            let text = clean_text(&child);
            if !text.is_empty() {
                blocks.push(ContentBlock {
                    heading: heading.clone(),
                    text,
                });
            }
        } else {
            let (nested, nested_heading) = walk(child, heading.clone());
            blocks.extend(nested);
            heading = nested_heading.or(heading);
        }
    }
    (blocks, heading)
}

fn clean_text(element: &ElementRef<'_>) -> String {
    let mut parts = Vec::new();
    collect_text(*element, &mut parts);
    parts
        .into_iter()
        .flat_map(str::split_whitespace)
        .collect::<Vec<_>>()
        .join(" ")
}

fn collect_text<'a>(element: ElementRef<'a>, parts: &mut Vec<&'a str>) {
    for child in element.children() {
        if let Some(child) = ElementRef::wrap(child) {
            if !is_excluded(&child) {
                collect_text(child, parts);
            }
        } else if let Some(text) = child.value().as_text() {
            parts.push(text);
        }
    }
}

fn is_excluded(element: &ElementRef<'_>) -> bool {
    let name = element.value().name();
    if matches!(
        name,
        "nav" | "script" | "style" | "form" | "header" | "footer" | "aside" | "noscript"
    ) {
        return true;
    }
    ["class", "id", "role"].iter().any(|attribute| {
        element.value().attr(attribute).is_some_and(|value| {
            let value = value.to_ascii_lowercase();
            ["nav", "menu", "sidebar", "footer", "header"]
                .iter()
                .any(|word| value.contains(word))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_supported_tags_and_ignores_chrome() {
        let document = Html::parse_document(
            "<body><nav><p>skip</p></nav><h1>Title</h1><p>One</p><ul><li>Two</li></ul><table><tr><td>Three</td></tr></table></body>",
        );
        let blocks = extract_content_blocks(&document);
        assert_eq!(
            blocks.iter().map(|b| b.text.as_str()).collect::<Vec<_>>(),
            ["One", "Two", "Three"]
        );
        assert!(blocks.iter().all(|b| b.heading.as_deref() == Some("Title")));
    }

    #[test]
    fn ignores_nested_style_text() {
        let document = Html::parse_document(
            "<table><tr><td>WP:SHORTCUTS<style>.mw-parser-output .hlist{margin:0}</style></td></tr></table>",
        );
        let blocks = extract_content_blocks(&document);

        assert_eq!(blocks[0].text, "WP:SHORTCUTS");
    }
}
