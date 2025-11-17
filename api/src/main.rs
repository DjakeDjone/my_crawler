use actix_cors::Cors;
use actix_web::{App, HttpResponse, HttpServer, Responder, web};
use serde::{Deserialize, Serialize};
use shared_crawler_api::{WEAVIATE_CLASS_NAME, WebPageChunk, WebPageResult, util_fns::load_env};
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

async fn search(query: web::Query<SearchQuery>, data: web::Data<AppState>) -> impl Responder {
    // Build the hybrid query parameter (combines vector search with a lexical/text match)
    // Adjust `alpha` to control weight between vector and lexical scoring (0.0 = only lexical, 1.0 = only vector).
    let hybrid = format!(
        r#"{{query: "{}", alpha: 0.5}}"#,
        query.query.replace("\"", "\\\"")
    );

    // Build the GetQuery using the weaviate-community crate, using hybrid search
    let weaviate_query = GetQuery::builder(WEAVIATE_CLASS_NAME, WebPageChunk::field_names())
        .with_limit(query.limit as u32)
        .with_hybrid(&hybrid)
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
    // Build the hybrid query parameter (combines vector search with a lexical/text match)
    // Use a moderate alpha so both embedding similarity and lexical matching influence results.
    let hybrid = format!(
        r#"{{query: "{}", alpha: 0.5}}"#,
        req.text.replace("\"", "\\\"")
    );

    // Build the GetQuery using the weaviate-community crate, using hybrid search
    let weaviate_query = GetQuery::builder(WEAVIATE_CLASS_NAME, WebPageChunk::field_names())
        .with_limit(5)
        .with_hybrid(&hybrid)
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

async fn count(_data: web::Data<AppState>) -> impl Responder {
    // Determine Weaviate base URL from environment (fall back to default)
    let weaviate_url =
        env::var("WEAVIATE_URL").unwrap_or_else(|_| "http://localhost:8080".to_string());
    let graphql_url = format!("{}/v1/graphql", weaviate_url.trim_end_matches('/'));

    // Construct the GraphQL query to aggregate count for the class
    let graphql_query = format!(
        "query {{ Aggregate {{ {} {{ meta {{ count }} }} }} }}",
        WEAVIATE_CLASS_NAME
    );

    // Send request
    let client = reqwest::Client::new();
    let resp = match client
        .post(&graphql_url)
        .json(&serde_json::json!({ "query": graphql_query }))
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            return HttpResponse::InternalServerError().json(ErrorResponse {
                error: format!("Failed to contact Weaviate GraphQL endpoint: {}", e),
            });
        }
    };

    let json: serde_json::Value = match resp.json().await {
        Ok(j) => j,
        Err(e) => {
            return HttpResponse::InternalServerError().json(ErrorResponse {
                error: format!("Invalid JSON response from Weaviate: {}", e),
            });
        }
    };

    // Expected shape: { "data": { "Aggregate": { "<ClassName>": [ { "meta": { "count": <n> } } ] } } }
    let count = json
        .get("data")
        .and_then(|d| d.get("Aggregate"))
        .and_then(|a| a.get(WEAVIATE_CLASS_NAME))
        .and_then(|arr| arr.get(0))
        .and_then(|obj| obj.get("meta"))
        .and_then(|m| m.get("count"))
        .and_then(|c| c.as_u64())
        .unwrap_or(0);

    HttpResponse::Ok().json(serde_json::json!({ "count": count }))
}

#[derive(Debug, Deserialize)]
pub struct GetPageRequest {
    pub url: String,
}

/// returns all chunks for a given url (sorted)
async fn get_page(query: web::Query<GetPageRequest>, _data: web::Data<AppState>) -> HttpResponse {
    // Determine Weaviate base URL from environment (fall back to default)
    println!("Getting page for {}", query.url);
    let weaviate_url =
        env::var("WEAVIATE_URL").unwrap_or_else(|_| "http://localhost:8080".to_string());
    let graphql_url = format!("{}/v1/graphql", weaviate_url.trim_end_matches('/'));

    // Escape quotes in the URL for embedding into the GraphQL string
    let url_escaped = query.url.replace('"', "\\\"");

    // Build GraphQL query: filter by source_url and sort by crawled_at ascending
    let fields = WebPageChunk::field_names().join(" ");
    let graphql_query = format!(
        "query {{ Get {{ {}(where: {{path: \"source_url\", operator: Equal, valueString: \"{}\"}}, sort: [{{path: \"crawled_at\", order: asc}}]) {{ {} }} }} }}",
        WEAVIATE_CLASS_NAME, url_escaped, fields
    );

    // Send request
    let client = reqwest::Client::new();
    let resp = match client
        .post(&graphql_url)
        .json(&serde_json::json!({ "query": graphql_query }))
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            return HttpResponse::InternalServerError().json(ErrorResponse {
                error: format!("Failed to contact Weaviate GraphQL endpoint: {}", e),
            });
        }
    };

    let json: serde_json::Value = match resp.json().await {
        Ok(j) => j,
        Err(e) => {
            return HttpResponse::InternalServerError().json(ErrorResponse {
                error: format!("Invalid JSON response from Weaviate: {}", e),
            });
        }
    };

    // Parse results: expected shape { data: { Get: { "<Class>": [ { ... }, ... ] } } }
    let mut chunks: Vec<WebPageChunk> = Vec::new();
    if let Some(items) = json
        .get("data")
        .and_then(|d| d.get("Get"))
        .and_then(|g| g.get(WEAVIATE_CLASS_NAME))
        .and_then(|a| a.as_array())
    {
        for item in items {
            if let Some(chunk) = WebPageChunk::from_weaviate_json(item) {
                chunks.push(chunk);
            }
        }
    }

    // Ensure stable sort: first by crawled_at asc, then by chunk_content asc
    chunks.sort_by(|a, b| {
        a.crawled_at
            .cmp(&b.crawled_at)
            .then(a.chunk_content.cmp(&b.chunk_content))
    });

    HttpResponse::Ok().json(chunks)
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
    println!("   GET  /search         - Search vector database");
    println!("   POST /plagiat        - Check text for plagiarism");
    println!("   GET  /count          - Document count");
    println!("   GET  /page           - Get all chunks for a page");
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
            .route("/count", web::get().to(count))
            .route("/page", web::get().to(get_page))
    })
    .bind(&bind_address)?
    .run()
    .await
}
