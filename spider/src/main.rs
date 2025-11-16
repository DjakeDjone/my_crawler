use crate::crawl_loop::CrawlLoop;
use actix_cors::Cors;
use actix_web::{web, App, HttpResponse, HttpServer, Responder};
use serde::{Deserialize, Serialize};
use shared_crawler_api::util_fns::load_env;
use std::env;
use std::sync::Arc;
use tokio::sync::Mutex;
use weaviate_community::WeaviateClient;

pub mod crawl_loop;
pub mod extractor;
pub mod extractor_content;
pub mod index;
pub mod weaviate;
pub mod web_visitor;

const USER_AGENT: &str = "PoliteWebCrawler";
const REQUEST_TIMEOUT_SECS: u64 = 30;

struct AppState {
    crawl_loop: Arc<Mutex<CrawlLoop>>,
}

#[derive(Debug, Deserialize)]
pub struct CrawlRequest {
    url: String,
    max_pages: usize,
    // same domain, per default true
    #[serde(default = "default_same_domain")]
    same_domain: bool, // if true, only crawl pages from the same root domain will be crawled
}

fn default_same_domain() -> bool {
    true
}

#[derive(Debug, Serialize)]
struct CrawlResponse {
    success: bool,
    message: String,
    pages_crawled: usize,
    pages_indexed: usize,
    urls: Vec<String>,
}

async fn health_check() -> impl Responder {
    HttpResponse::Ok().json(serde_json::json!({
        "status": "ok",
        "message": "Crawler API is running"
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
        let mut loop_lock = app_state.crawl_loop.lock().await;
        loop_lock.add_crawl_request(req).await;
    }

    HttpResponse::Ok().json(CrawlResponse {
        success: true,
        message: format!("Queued crawl for {}", url),
        pages_crawled: 0,
        pages_indexed: 0,
        urls: vec![url],
    })
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

    // Create and start a CrawlLoop. We wrap it in an Arc<Mutex<...>> so it can be shared safely
    // across actix handlers while still allowing mutable access.
    let crawl_loop = Arc::new(Mutex::new(CrawlLoop::new()));
    // Set the Weaviate client using the async mutex to avoid blocking the runtime.
    // We intentionally await the lock here because `main` is async under the actix runtime.
    {
        let mut loop_guard = crawl_loop.lock().await;
        loop_guard.set_weaviate_client(Arc::new(weaviate_client));
    }

    // Spawn a background task to start the crawl loop's runners. The CrawlLoop::run method
    // itself spawns background tasks for runners, so we only need to call run() once here.
    {
        let crawl_loop_clone = crawl_loop.clone();
        tokio::spawn(async move {
            let mut loop_guard = crawl_loop_clone.lock().await;
            loop_guard.run().await;
        });
    }

    let app_state = web::Data::new(AppState {
        crawl_loop: crawl_loop.clone(),
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
