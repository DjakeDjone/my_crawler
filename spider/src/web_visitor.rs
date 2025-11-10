use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::Client;
use scraper::{Html, Selector};
use url::Url;

use crate::{REQUEST_TIMEOUT_SECS, USER_AGENT};

pub async fn fetch_page(client: &Client, url: &str) -> Result<String> {
    println!("Fetching page: {}", url);
    // add 'http:// or 'https:// to the URL if it doesn't start with one
    let url = if !url.starts_with("http://") && !url.starts_with("https://") {
        format!("https://{}", url)
    } else {
        url.to_string()
    };

    let response = client
        .get(url)
        .header("User-Agent", USER_AGENT)
        .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .send()
        .await
        .context("Failed to send HTTP request")?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "HTTP request failed with status: {}",
            response.status()
        ));
    }

    let html = response
        .text()
        .await
        .context("Failed to read response body")?;

    Ok(html)
}

pub fn extract_links(html_content: &str, base_url: &Url) -> Vec<String> {
    let document = Html::parse_document(html_content);
    let link_selector = Selector::parse("a[href]").unwrap();
    let mut links = Vec::new();

    for element in document.select(&link_selector) {
        // Skip links that are hidden (display: none or visibility: hidden)
        if let Some(style) = element.value().attr("style") {
            let style_lower = style.to_lowercase();
            if style_lower.contains("display:none")
                || style_lower.contains("display: none")
                || style_lower.contains("visibility:hidden")
                || style_lower.contains("visibility: hidden")
            {
                continue;
            }
        }

        // Skip links inside <script> tags by checking parent elements
        let mut is_in_script = false;
        for ancestor in element.ancestors() {
            if let Some(elem) = ancestor.value().as_element() {
                if elem.name() == "script" {
                    is_in_script = true;
                    break;
                }
            }
        }
        if is_in_script {
            continue;
        }

        if let Some(href) = element.value().attr("href") {
            // Skip empty, javascript, mailto, and other non-http(s) schemes
            let href_trimmed = href.trim();
            if href_trimmed.is_empty()
                || href_trimmed.starts_with("javascript:")
                || href_trimmed.starts_with("mailto:")
                || href_trimmed.starts_with("tel:")
                || href_trimmed.starts_with("data:")
                || href_trimmed.contains("undefined")
            {
                continue;
            }

            // Try to resolve relative URLs
            if let Ok(absolute_url) = base_url.join(href_trimmed) {
                let url_str = absolute_url.to_string();

                // Filter out non-HTTP(S) URLs, fragments, and URLs containing 'undefined'
                if (url_str.starts_with("http://") || url_str.starts_with("https://"))
                    && !url_str.contains('#')
                    && !url_str.contains("undefined")
                {
                    links.push(url_str);
                }
            }
        }
    }

    // Remove duplicates
    // Normalize URLs by removing protocol and trailing slash
    links = links.into_iter().map(|url| normalize_url(&url)).collect();

    links.sort();
    links.dedup();
    links
}

pub fn normalize_url(url: &str) -> String {
    url.trim_end_matches('/').to_owned()
    // .replace("https://", "")
    // .replace("http://", "")
}
