use crate::crawl_loop::CrawlLoop;
use crate::qdrant::PageIndexer;
use crate::stats::CrawlStats;
use actix_cors::Cors;
use actix_web::{web, App, HttpResponse, HttpServer, Responder};
use serde::Deserialize;
use shared_crawler_api::util_fns::load_env;
use std::env;
use std::sync::Arc;
use tokio::sync::Mutex;

pub mod crawl_loop;
pub mod extractor;
pub mod extractor_content;
pub mod index;
pub mod qdrant;
pub mod robots;
pub mod sitemap;
pub mod stats;
pub mod web_visitor;
pub mod web_visitor_browser;

const REQUEST_TIMEOUT_SECS: u64 = 30;

struct AppState {
    crawl_loop: Arc<Mutex<CrawlLoop>>,
    stats: Arc<CrawlStats>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct CrawlRequest {
    pub url: String,
    pub max_pages: usize,
    // same domain, per default true
    #[serde(default = "default_same_domain")]
    pub same_domain: bool, // if true, only crawl pages from the same root domain will be crawled
    /// Force browser-based crawling for all pages (bypasses HTTP client)
    #[serde(default)]
    pub use_browser: bool,
    /// CSS selector to wait for before extracting content (e.g., ".main-content")
    #[serde(default)]
    pub wait_for_selector: Option<String>,
    /// Timeout in milliseconds for wait_for_selector (default: 5000)
    #[serde(default = "default_wait_timeout")]
    pub wait_timeout_ms: u64,
    /// Maximum crawl depth (default: 10)
    #[serde(default = "default_max_depth")]
    pub max_depth: usize,
}

fn default_same_domain() -> bool {
    true
}

fn default_wait_timeout() -> u64 {
    5000
}

fn default_max_depth() -> usize {
    10
}

async fn health_check() -> impl Responder {
    HttpResponse::Ok().json(serde_json::json!({
        "status": "ok",
        "message": "Crawler API is running"
    }))
}

/// Status endpoint returning crawler metrics
async fn status(app_state: web::Data<AppState>) -> impl Responder {
    let stats = app_state.stats.snapshot();
    let queue_size = {
        let loop_lock = app_state.crawl_loop.lock().await;
        loop_lock.queue_size().await
    };

    HttpResponse::Ok().json(serde_json::json!({
        "status": "ok",
        "queue_size": queue_size,
        "pages_crawled": stats.pages_crawled,
        "pages_failed": stats.pages_failed,
        "pages_skipped_robots": stats.pages_skipped_robots,
        "pages_skipped_depth": stats.pages_skipped_depth,
        "retries_attempted": stats.retries_attempted,
    }))
}

async fn crawl(
    crawl_req: web::Json<CrawlRequest>,
    app_state: web::Data<AppState>,
) -> impl Responder {
    let req = crawl_req.into_inner();
    let url = req.url.clone();

    // enqueue the crawl request into the shared CrawlLoop
    {
        let loop_lock = app_state.crawl_loop.lock().await;
        if let Err(error) = loop_lock.add_crawl_request(req).await {
            return HttpResponse::BadRequest().json(serde_json::json!({
                "success": false,
                "message": error
            }));
        }
    }

    HttpResponse::Ok().json(serde_json::json!({
        "success": true,
        "message": format!("Queued crawl for {url}"),
    }))
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt::init();

    load_env();

    let host = env::var("SPIDER_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let port = env::var("SPIDER_PORT").unwrap_or_else(|_| "8001".to_string());
    let bind_address = format!("{}:{}", host, port);
    let product_token =
        env::var("CRAWLER_PRODUCT_TOKEN").expect("CRAWLER_PRODUCT_TOKEN must be configured");
    let user_agent = env::var("CRAWLER_USER_AGENT").expect("CRAWLER_USER_AGENT must be configured");
    let allowed_origins =
        env::var("ALLOWED_ORIGINS").unwrap_or_else(|_| "http://localhost:3000".to_string());

    println!("🔒 CORS allowed origins: {}", allowed_origins);

    let indexer = Arc::new(PageIndexer::from_env().expect("failed to create Qdrant client"));
    indexer
        .ensure_collection()
        .await
        .expect("Qdrant collection creation failed");

    println!("🚀 Starting Crawler server on http://{}", bind_address);
    println!("📝 Routes:");
    println!("   GET  /health         - Health check");
    println!("   GET  /status         - Crawler status and metrics");
    println!("   POST /crawl          - Crawl a URL");
    println!();
    let stats = Arc::new(CrawlStats::new());

    let mut crawl_loop = CrawlLoop::new(stats.clone(), indexer, product_token, user_agent);
    crawl_loop.run();
    let crawl_loop = Arc::new(Mutex::new(crawl_loop));

    let app_state = web::Data::new(AppState {
        crawl_loop: crawl_loop.clone(),
        stats: stats.clone(),
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
            .route("/status", web::get().to(status))
            .route("/crawl", web::post().to(crawl))
    })
    .bind(&bind_address)?
    .run()
    .await
}
