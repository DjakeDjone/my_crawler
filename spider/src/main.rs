use actix_web::{web, App, HttpResponse, HttpServer, Responder};
use anyhow::{Context, Result};
use reqwest::Client;
use rocksdb::{Options, DB};
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::env;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::{error, info, warn};
use url::Url;
use weaviate_community::WeaviateClient;

mod index;
mod third_party_search;
mod web_visitor;

use index::{ensure_schema, index_page_safe_with_client};

const DB_PATH: &str = "../database/crawled_urls.db";
const USER_AGENT: &str = "PoliteWebCrawler";
const REQUEST_TIMEOUT_SECS: u64 = 30;

struct AppState {
    weaviate_client: WeaviateClient,
    db: Arc<Mutex<DB>>,
    http_client: Client,
}

#[derive(Debug, Deserialize)]
struct CrawlRequest {
    url: String,
    #[serde(default = "default_depth")]
    depth: u8,
    max_pages: usize,
}

fn default_depth() -> u8 {
    1
}

#[derive(Debug, Serialize)]
struct CrawlResponse {
    success: bool,
    message: String,
    pages_crawled: usize,
    pages_indexed: usize,
    urls: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: String,
}

async fn health_check() -> impl Responder {
    HttpResponse::Ok().json(serde_json::json!({
        "status": "ok",
        "message": "Crawler API is running"
    }))
}

async fn crawl(req: web::Json<CrawlRequest>, data: web::Data<AppState>) -> impl Responder {
    info!(
        "Received crawl request for URL: {} with depth: {}",
        req.url, req.depth
    );

    // Validate URL
    let base_url = match Url::parse(&req.url) {
        Ok(url) => url,
        Err(e) => {
            return HttpResponse::BadRequest().json(ErrorResponse {
                error: format!("Invalid URL: {}", e),
            });
        }
    };

    // Ensure Weaviate schema exists
    if let Err(e) = ensure_schema(&data.weaviate_client).await {
        error!("Failed to ensure Weaviate schema: {}", e);
        return HttpResponse::InternalServerError().json(ErrorResponse {
            error: format!("Failed to initialize database schema: {}", e),
        });
    }

    // Start crawling
    let mut urls_to_crawl = vec![req.url.clone()];
    let mut crawled_urls = HashSet::new();
    let mut all_urls = Vec::new();
    let mut pages_indexed = 0;

    for current_depth in 0..req.depth {
        if urls_to_crawl.is_empty() {
            break;
        }

        let mut next_level_urls = Vec::new();

        // Process URLs at current depth
        for url in urls_to_crawl.iter() {
            if crawled_urls.contains(url) {
                continue;
            }

            info!("Crawling URL (depth {}): {}", current_depth + 1, url);

            // Fetch the page
            match fetch_page(&data.http_client, url).await {
                Ok(html_content) => {
                    // Extract links if we need to go deeper
                    let links = if current_depth + 1 < req.depth {
                        extract_links(&html_content, &base_url)
                    } else {
                        Vec::new()
                    };

                    // Add new links for next level
                    for link in &links {
                        // print if link includes undefined
                        if link.contains("undefined") {
                            println!("Link includes undefined: {} on page {}", link, url);
                        }

                        if !crawled_urls.contains(link) && is_same_domain(&base_url, link) {
                            next_level_urls.push(link.clone());
                        }
                    }

                    // Index the page
                    index_page_safe_with_client(
                        &data.weaviate_client,
                        url.clone(),
                        html_content,
                        links,
                    )
                    .await;

                    pages_indexed += 1;
                    crawled_urls.insert(url.clone());
                    all_urls.push(url.clone());

                    // Store in RocksDB
                    let db = data.db.lock().await;
                    let timestamp = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs();
                    let _ = db.put(url.as_bytes(), timestamp.to_string().as_bytes());
                }
                Err(e) => {
                    warn!("Failed to fetch {}: {}", url, e);
                }
            }

            // Rate limiting - simple delay
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        // Move to next depth level
        urls_to_crawl = next_level_urls;
    }

    let pages_crawled = crawled_urls.len();

    info!(
        "Crawl complete: {} pages crawled, {} pages indexed",
        pages_crawled, pages_indexed
    );

    HttpResponse::Ok().json(CrawlResponse {
        success: true,
        message: format!(
            "Successfully crawled {} page(s) at depth {}",
            pages_crawled, req.depth
        ),
        pages_crawled,
        pages_indexed,
        urls: all_urls,
    })
}

async fn fetch_page(client: &Client, url: &str) -> Result<String> {
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

fn extract_links(html_content: &str, base_url: &Url) -> Vec<String> {
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
    links.sort();
    links.dedup();
    links
}

fn is_same_domain(base_url: &Url, target_url: &str) -> bool {
    if let Ok(target) = Url::parse(target_url) {
        base_url.domain() == target.domain()
    } else {
        false
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    let host = env::var("HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let port = env::var("PORT").unwrap_or_else(|_| "8001".to_string());
    let bind_address = format!("{}:{}", host, port);
    let weaviate_url =
        env::var("WEAVIATE_URL").unwrap_or_else(|_| "http://localhost:8080".to_string());

    // Open RocksDB
    let mut db_opts = Options::default();
    db_opts.create_if_missing(true);
    let db = DB::open(&db_opts, DB_PATH).expect("Failed to open RocksDB");
    let db = Arc::new(Mutex::new(db));

    // Create HTTP client
    let http_client = Client::builder()
        .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .build()
        .expect("Failed to create HTTP client");

    // Create Weaviate client
    let weaviate_client = WeaviateClient::builder(&weaviate_url)
        .build()
        .expect("Failed to create Weaviate client");

    println!("🚀 Starting Crawler server on http://{}", bind_address);
    println!("📝 Routes:");
    println!("   GET  /health         - Health check");
    println!("   POST /crawl          - Crawl a URL");
    println!();
    println!("🔗 Connected to Weaviate at: {}", weaviate_url);
    println!("💾 Database path: {}", DB_PATH);

    let app_state = web::Data::new(AppState {
        weaviate_client,
        db,
        http_client,
    });

    HttpServer::new(move || {
        App::new()
            .app_data(app_state.clone())
            .route("/health", web::get().to(health_check))
            .route("/crawl", web::post().to(crawl))
    })
    .bind(&bind_address)?
    .run()
    .await
}
