use actix_cors::Cors;
use actix_web::{App, HttpResponse, HttpServer, Responder, web};
use serde::{Deserialize, Serialize};
use shared_crawler_api::{WEAVIATE_CLASS_NAME, WebPageData, WebPageResult, util_fns::load_env};
use std::env;
use weaviate_community::{WeaviateClient, collections::query::GetQuery};

#[derive(Debug, Deserialize)]
struct SearchQuery {
    query: String,
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_limit() -> usize {
    10
}

#[derive(Debug, Deserialize)]
struct PlagiatRequest {
    text: String,
    #[serde(default = "default_threshold")]
    threshold: f32,
}

fn default_threshold() -> f32 {
    0.6
}

#[derive(Debug, Serialize)]
struct SearchResult {
    results: Vec<WebPageResult>,
    total: usize,
}

#[derive(Debug, Serialize)]
struct PlagiatResult {
    is_plagiat: bool,
    similarity_score: f32,
    matched_documents: Vec<WebPageResult>,
}

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: String,
}

struct AppState {
    weaviate_client: WeaviateClient,
}

async fn search(query: web::Json<SearchQuery>, data: web::Data<AppState>) -> impl Responder {
    // Build the nearText query parameter
    let near_text = format!(r#"{{concepts: ["{}"]}}"#, query.query.replace("\"", "\\\""));

    // Build the GetQuery using the weaviate-community crate
    let weaviate_query = GetQuery::builder(WEAVIATE_CLASS_NAME, WebPageData::field_names())
        .with_limit(query.limit as u32)
        .with_near_text(&near_text)
        .with_additional(vec!["distance"])
        .build();

    // Execute the query
    let response = data.weaviate_client.query.get(weaviate_query).await;

    match response {
        Ok(resp) => {
            let mut results = Vec::new();

            // Parse the response JSON structure
            if let Some(data) = resp
                .get("data")
                .and_then(|d| d.get("Get"))
                .and_then(|g| g.get(WEAVIATE_CLASS_NAME))
                .and_then(|d| d.as_array())
            {
                for item in data {
                    if let Some(result) = WebPageResult::from_weaviate_json(item) {
                        results.push(result);
                    }
                }
            }

            let total = results.len();
            HttpResponse::Ok().json(SearchResult { results, total })
        }
        Err(e) => HttpResponse::InternalServerError().json(ErrorResponse {
            error: format!("Failed to query Weaviate: {}", e),
        }),
    }
}

async fn plagiat(req: web::Json<PlagiatRequest>, data: web::Data<AppState>) -> impl Responder {
    // Build the nearText query parameter
    let near_text = format!(r#"{{concepts: ["{}"]}}"#, req.text.replace("\"", "\\\""));

    // Build the GetQuery using the weaviate-community crate
    let weaviate_query = GetQuery::builder(WEAVIATE_CLASS_NAME, WebPageData::field_names())
        .with_limit(5)
        .with_near_text(&near_text)
        .with_additional(vec!["distance"])
        .build();

    // Execute the query
    let response = data.weaviate_client.query.get(weaviate_query).await;

    match response {
        Ok(resp) => {
            let mut matched_documents = Vec::new();
            let mut highest_similarity = 0.0_f32;

            // Parse the response JSON structure
            if let Some(data) = resp
                .get("data")
                .and_then(|d| d.get("Get"))
                .and_then(|g| g.get(WEAVIATE_CLASS_NAME))
                .and_then(|d| d.as_array())
            {
                for item in data {
                    if let Some(result) = WebPageResult::from_weaviate_json(item) {
                        if result.score > highest_similarity {
                            highest_similarity = result.score;
                        }

                        // Only include documents above threshold
                        if result.score >= req.threshold {
                            matched_documents.push(result);
                        }
                    }
                }
            }

            let is_plagiat = highest_similarity >= req.threshold;

            HttpResponse::Ok().json(PlagiatResult {
                is_plagiat,
                similarity_score: highest_similarity,
                matched_documents,
            })
        }
        Err(e) => HttpResponse::InternalServerError().json(ErrorResponse {
            error: format!("Failed to query Weaviate: {}", e),
        }),
    }
}

async fn health_check() -> impl Responder {
    HttpResponse::Ok().json(serde_json::json!({
        "status": "ok",
        "message": "API is running"
    }))
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Load environment variables from .env file
    load_env();

    let host = env::var("API_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let port = env::var("API_PORT").unwrap_or_else(|_| "8000".to_string());
    let bind_address = format!("{}:{}", host, port);
    let weaviate_url =
        env::var("WEAVIATE_URL").unwrap_or_else(|_| "http://localhost:8080".to_string());
    let allowed_origins =
        env::var("ALLOWED_ORIGINS").unwrap_or_else(|_| "http://localhost:3000".to_string());

    println!("🔒 CORS allowed origins: {}", allowed_origins);

    println!("🚀 Starting API server on http://{}", bind_address);
    println!("📝 Routes:");
    println!("   GET  /health         - Health check");
    println!("   POST /search         - Search vector database");
    println!("   POST /plagiat        - Check text for plagiarism");
    println!();
    println!("🔗 Connected to Weaviate at: {}", weaviate_url);

    // Create Weaviate client once at startup
    let weaviate_client = WeaviateClient::builder(&weaviate_url)
        .build()
        .expect("Failed to create Weaviate client");

    let app_state = web::Data::new(AppState { weaviate_client });

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
            .route("/search", web::get().to(search))
            .route("/plagiat", web::post().to(plagiat))
    })
    .bind(&bind_address)?
    .run()
    .await
}
