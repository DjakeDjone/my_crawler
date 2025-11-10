use actix_cors::Cors;
use actix_web::{web, App, HttpResponse, HttpServer, Responder};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use shared_crawler_api::util_fns::load_env;
use std::collections::HashSet;
use std::env;
use std::time::Duration;
use tracing::{error, info, warn};
use url::Url;
use weaviate_community::WeaviateClient;

mod index;
mod web_visitor;

use index::{ensure_schema, index_page_safe_with_client};

use crate::web_visitor::{extract_links, fetch_page};

const DB_PATH: &str = "../database/crawled_urls.db";
const USER_AGENT: &str = "PoliteWebCrawler";
const REQUEST_TIMEOUT_SECS: u64 = 30;

struct AppState {
    weaviate_client: WeaviateClient,
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

    // schema if not exists
    if let Err(e) = ensure_schema(&data.weaviate_client).await {
        error!("Failed to ensure Weaviate schema: {}", e);
        return HttpResponse::InternalServerError().json(ErrorResponse {
            error: format!("Failed to initialize database schema: {}", e),
        });
    }

    // crawling
    // TODO: extract into function
    let mut urls_to_crawl = vec![req.url.clone()];
    let mut crawled_urls = HashSet::new();
    let mut all_urls = Vec::new();
    let mut pages_indexed = 0;

    for current_depth in 0..req.depth {
        if urls_to_crawl.is_empty() {
            break;
        }

        let mut next_level_urls = Vec::new();

        for url in urls_to_crawl.iter() {
            if crawled_urls.len() >= req.max_pages {
                break;
            }
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
                    index_page_safe_with_client(&data.weaviate_client, url.clone(), html_content)
                        .await;

                    pages_indexed += 1;
                    crawled_urls.insert(url.clone());
                    all_urls.push(url.clone());
                }
                Err(e) => {
                    warn!("Failed to fetch {}: {}", url, e);
                }
            }

            // Rate limiting - simple delay
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        urls_to_crawl = next_level_urls;

        if crawled_urls.len() >= req.max_pages {
            break;
        }
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

fn is_same_domain(base_url: &Url, target_url: &str) -> bool {
    if let Ok(target) = Url::parse(target_url) {
        base_url.domain() == target.domain()
    } else {
        false
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt::init();

    load_env();

    let host = env::var("SPIDER_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let port = env::var("SPIDER_PORT").unwrap_or_else(|_| "8001".to_string());
    let bind_address = format!("{}:{}", host, port);
    let weaviate_url =
        env::var("WEAVIATE_URL").unwrap_or_else(|_| "http://localhost:8080".to_string());
    let allowed_origins =
        env::var("ALLOWED_ORIGINS").unwrap_or_else(|_| "http://localhost:3000".to_string());

    println!("🔒 CORS allowed origins: {}", allowed_origins);

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
        http_client,
    });

    HttpServer::new(move || {
        let cors = if allowed_origins.trim() == "*" {
            // Allow any origin
            Cors::default()
                .allow_any_origin()
                .allow_any_method()
                .allow_any_header()
                .expose_headers(vec![actix_web::http::header::CONTENT_TYPE])
                .max_age(3600)
        } else {
            // Parse allowed origins from comma-separated list
            let origins: Vec<&str> = allowed_origins.split(',').map(|s| s.trim()).collect();

            let mut cors = Cors::default()
                .allowed_methods(vec!["GET", "POST", "OPTIONS"])
                .allowed_headers(vec![
                    actix_web::http::header::CONTENT_TYPE,
                    actix_web::http::header::ACCEPT,
                    actix_web::http::header::AUTHORIZATION,
                ])
                .expose_headers(vec![actix_web::http::header::CONTENT_TYPE])
                .max_age(3600);

            // Add each origin
            for origin in origins {
                cors = cors.allowed_origin(origin);
            }

            cors
        };

        App::new()
            .wrap(cors)
            .app_data(app_state.clone())
            .route("/health", web::get().to(health_check))
            .route("/crawl", web::post().to(crawl))
    })
    .bind(&bind_address)?
    .run()
    .await
}
