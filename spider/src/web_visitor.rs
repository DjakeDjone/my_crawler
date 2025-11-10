use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::Client;
use scraper::{Html, Selector};
use url::Url;

use crate::{REQUEST_TIMEOUT_SECS, USER_AGENT};

pub async fn fetch_page(client: &Client, url: &str) -> Result<String> {
    println!("Fetching page: {}", url);

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
    // first strip 'www.' and 'http(s)://' from the beginning of the URL
    links.iter_mut().for_each(|url| {
        if url.starts_with("http://") {
            *url = url.replacen("http://", "", 1);
        } else if url.starts_with("https://") {
            *url = url.replacen("https://", "", 1);
        }

        if url.starts_with("www.") {
            *url = url.replacen("www.", "", 1);
        }

        // ends with '/'
        if url.ends_with('/') {
            *url = url.strip_suffix('/').unwrap_or(url).to_owned();
        }
    });
    links.sort();
    links.dedup();
    links
}
