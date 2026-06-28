use actix_cors::Cors;
use actix_web::{App, HttpResponse, HttpServer, Responder, web};
use qdrant_client::{
    Qdrant,
    qdrant::{
        Condition, CountPointsBuilder, DocumentBuilder, Filter, PrefetchQueryBuilder, Query,
        QueryPointsBuilder, RrfBuilder, ScrollPointsBuilder,
    },
};
use serde::{Deserialize, Serialize};
use shared_crawler_api::{QDRANT_COLLECTION_NAME, WebPageChunk, WebPageResult, util_fns::load_env};
use std::{
    collections::{HashMap, HashSet},
    env,
};

mod ranking;

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
    qdrant: Qdrant,
    http: reqwest::Client,
    tei_url: String,
    popularity: ranking::DomainPopularity,
}

async fn search(query: web::Query<SearchQuery>, data: web::Data<AppState>) -> impl Responder {
    match hybrid_search(&data, &query.query, query.limit.saturating_mul(4)).await {
        Ok(mut results) => {
            results.retain(|result| ranking::is_searchable_page(&result.data.source_url));
            ranking::apply_ranking_boosts(&mut results, &query.query, &data.popularity);
            let final_results = unique_pages(results, query.limit);
            HttpResponse::Ok().json(SearchResult {
                total: final_results.len(),
                results: final_results,
            })
        }
        Err(error) => HttpResponse::InternalServerError().json(ErrorResponse {
            error: error.to_string(),
        }),
    }
}

fn unique_pages(results: Vec<WebPageResult>, limit: usize) -> Vec<WebPageResult> {
    let mut seen_urls = HashSet::new();
    let unique = results
        .into_iter()
        .filter(|result| seen_urls.insert(result.data.source_url.clone()))
        .collect::<Vec<_>>();
    let mut seen_hosts = HashSet::new();
    let diverse = unique
        .iter()
        .enumerate()
        .filter(|(_, result)| {
            let url = &result.data.source_url;
            let source = url::Url::parse(url)
                .ok()
                .and_then(|url| url.host_str().map(str::to_owned))
                .unwrap_or_else(|| url.clone());
            seen_hosts.insert(source)
        })
        .map(|(index, _)| index)
        .take(limit.min(5))
        .collect::<Vec<_>>();
    let mut remaining = unique.into_iter().map(Some).collect::<Vec<_>>();
    let mut selected = diverse
        .into_iter()
        .map(|index| remaining[index].take().unwrap())
        .collect::<Vec<_>>();
    let slots = limit.saturating_sub(selected.len());
    selected.extend(remaining.into_iter().flatten().take(slots));
    selected
}

async fn hybrid_search(
    data: &AppState,
    text: &str,
    limit: usize,
) -> anyhow::Result<Vec<WebPageResult>> {
    let dense = embed(data, &format!("query: {text}")).await?;
    let lexical = bm25_document(text);
    let result = data
        .qdrant
        .query(
            QueryPointsBuilder::new(QDRANT_COLLECTION_NAME)
                .add_prefetch(
                    PrefetchQueryBuilder::default()
                        .query(dense)
                        .using("dense")
                        .limit(limit as u64),
                )
                .add_prefetch(
                    PrefetchQueryBuilder::default()
                        .query(Query::new_nearest(lexical.clone()))
                        .using("title_bm25")
                        .limit(limit as u64),
                )
                .add_prefetch(
                    PrefetchQueryBuilder::default()
                        .query(Query::new_nearest(lexical))
                        .using("body_bm25")
                        .limit(limit as u64),
                )
                .query(Query::new_rrf(
                    RrfBuilder::new().weights(vec![2.0, 2.0, 1.0]),
                ))
                .limit(limit as u64)
                .with_payload(true),
        )
        .await?;
    Ok(result
        .result
        .into_iter()
        .filter_map(|point| {
            WebPageChunk::from_payload_json(&payload_json(point.payload))
                .map(|data| WebPageResult::new(data, point.score))
        })
        .collect())
}

async fn plagiat(req: web::Json<PlagiatRequest>, data: web::Data<AppState>) -> impl Responder {
    let result = async {
        let dense = embed(&data, &format!("query: {}", req.text)).await?;
        let response = data
            .qdrant
            .query(
                QueryPointsBuilder::new(QDRANT_COLLECTION_NAME)
                    .query(dense)
                    .using("dense")
                    .limit(5)
                    .with_payload(true),
            )
            .await?;
        Ok::<_, anyhow::Error>(
            response
                .result
                .into_iter()
                .filter_map(|point| {
                    WebPageChunk::from_payload_json(&payload_json(point.payload))
                        .map(|data| WebPageResult::new(data, point.score))
                })
                .collect::<Vec<_>>(),
        )
    }
    .await;

    match result {
        Ok(results) => {
            let highest = results.first().map(|result| result.score).unwrap_or(0.0);
            let matched_documents = results
                .into_iter()
                .filter(|result| result.score >= req.threshold)
                .collect();
            HttpResponse::Ok().json(PlagiatResult {
                is_plagiat: highest >= req.threshold,
                similarity_score: highest,
                matched_documents,
            })
        }
        Err(error) => HttpResponse::InternalServerError().json(ErrorResponse {
            error: error.to_string(),
        }),
    }
}

async fn health_check() -> impl Responder {
    HttpResponse::Ok().json(serde_json::json!({"status": "ok", "message": "API is running"}))
}

async fn count(data: web::Data<AppState>) -> impl Responder {
    match data
        .qdrant
        .count(CountPointsBuilder::new(QDRANT_COLLECTION_NAME).exact(true))
        .await
    {
        Ok(response) => HttpResponse::Ok().json(serde_json::json!({
            "count": response.result.map(|value| value.count).unwrap_or(0)
        })),
        Err(error) => HttpResponse::InternalServerError().json(ErrorResponse {
            error: error.to_string(),
        }),
    }
}

#[derive(Debug, Deserialize)]
pub struct GetPageRequest {
    pub url: String,
}

async fn get_page(query: web::Query<GetPageRequest>, data: web::Data<AppState>) -> HttpResponse {
    match data
        .qdrant
        .scroll(
            ScrollPointsBuilder::new(QDRANT_COLLECTION_NAME)
                .filter(Filter::must([Condition::matches(
                    "source_url",
                    query.url.clone(),
                )]))
                .limit(10_000)
                .with_payload(true)
                .with_vectors(false),
        )
        .await
    {
        Ok(response) => {
            let mut chunks = response
                .result
                .into_iter()
                .filter_map(|point| {
                    let payload = payload_json(point.payload);
                    let index = payload
                        .get("chunk_index")
                        .and_then(|value| value.as_i64())
                        .unwrap_or(0);
                    WebPageChunk::from_payload_json(&payload).map(|chunk| (index, chunk))
                })
                .collect::<Vec<_>>();
            chunks.sort_by_key(|(index, _)| *index);
            HttpResponse::Ok().json(
                chunks
                    .into_iter()
                    .map(|(_, chunk)| chunk)
                    .collect::<Vec<_>>(),
            )
        }
        Err(error) => HttpResponse::InternalServerError().json(ErrorResponse {
            error: error.to_string(),
        }),
    }
}

async fn embed(data: &AppState, input: &str) -> anyhow::Result<Vec<f32>> {
    let mut response = data
        .http
        .post(format!("{}/embed", data.tei_url.trim_end_matches('/')))
        .json(&serde_json::json!({"inputs": [input]}))
        .send()
        .await?
        .error_for_status()?
        .json::<Vec<Vec<f32>>>()
        .await?;
    anyhow::ensure!(
        response.len() == 1 && response[0].len() == 384,
        "TEI returned invalid embedding dimensions"
    );
    Ok(response.remove(0))
}

fn bm25_document(text: &str) -> qdrant_client::qdrant::Document {
    DocumentBuilder::new(text, "qdrant/bm25")
        .options(HashMap::from([("language".to_string(), "none".into())]))
        .build()
}

fn payload_json(payload: HashMap<String, qdrant_client::qdrant::Value>) -> serde_json::Value {
    serde_json::Value::Object(
        payload
            .into_iter()
            .map(|(key, value)| (key, value.into_json()))
            .collect(),
    )
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    load_env();
    let host = env::var("API_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let port = env::var("API_PORT").unwrap_or_else(|_| "8000".to_string());
    let bind_address = format!("{host}:{port}");
    let allowed_origins =
        env::var("ALLOWED_ORIGINS").unwrap_or_else(|_| "http://localhost:3000".to_string());
    let qdrant_url = env::var("QDRANT_URL").unwrap_or_else(|_| "http://localhost:6334".to_string());
    let http = reqwest::Client::new();
    let popularity = match ranking::load_domain_popularity(&http).await {
        Ok(popularity) => popularity,
        Err(error) => {
            eprintln!("failed to load Tranco popularity list: {error}");
            ranking::DomainPopularity::default()
        }
    };
    let state = web::Data::new(AppState {
        qdrant: Qdrant::from_url(&qdrant_url)
            .build()
            .expect("failed to create Qdrant client"),
        http,
        tei_url: env::var("TEI_URL").unwrap_or_else(|_| "http://localhost:8080".to_string()),
        popularity,
    });

    HttpServer::new(move || {
        let cors = if allowed_origins.trim() == "*" {
            Cors::default()
                .allow_any_origin()
                .allow_any_method()
                .allow_any_header()
        } else {
            allowed_origins
                .split(',')
                .fold(Cors::default(), |cors, origin| {
                    cors.allowed_origin(origin.trim())
                })
        };
        App::new()
            .wrap(cors)
            .app_data(state.clone())
            .route("/health", web::get().to(health_check))
            .route("/search", web::get().to(search))
            .route("/plagiat", web::post().to(plagiat))
            .route("/count", web::get().to(count))
            .route("/page", web::get().to(get_page))
    })
    .bind(bind_address)?
    .run()
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(url: &str) -> WebPageResult {
        WebPageResult::new(
            WebPageChunk {
                chunk_content: String::new(),
                chunk_heading: None,
                source_url: url.to_string(),
                page_title: "Same title".to_string(),
                description: String::new(),
                tags: vec![],
                categories: vec![],
                paid: 0.0,
                score: 0.0,
                crawled_at: 0,
            },
            0.0,
        )
    }

    #[test]
    fn keeps_same_title_pages_and_removes_duplicate_chunks() {
        let pages = unique_pages(
            vec![
                result("https://example.com/"),
                result("https://example.com/a"),
                result("https://example.com/a"),
            ],
            10,
        );

        assert_eq!(pages.len(), 2);
        assert_eq!(pages[1].data.source_url, "https://example.com/a");
    }

    #[test]
    fn diversifies_first_five_then_preserves_rank_order() {
        let urls = [
            "https://a.example/1",
            "https://a.example/2",
            "https://b.example/1",
            "https://c.example/1",
            "https://d.example/1",
            "https://e.example/1",
            "https://f.example/1",
        ];
        let pages = unique_pages(urls.into_iter().map(result).collect(), 7);
        let actual = pages
            .iter()
            .map(|page| page.data.source_url.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            actual,
            [
                urls[0], urls[2], urls[3], urls[4], urls[5], urls[1], urls[6]
            ]
        );
    }

    #[test]
    fn fills_from_repeated_hosts_and_handles_malformed_urls() {
        let urls = [
            "not a url",
            "also not a url",
            "https://example.com/1",
            "https://example.com/2",
        ];
        let pages = unique_pages(urls.into_iter().map(result).collect(), 4);

        assert_eq!(
            pages
                .iter()
                .map(|page| page.data.source_url.as_str())
                .collect::<Vec<_>>(),
            urls
        );
    }

    #[test]
    fn diversifies_limits_below_five() {
        let pages = unique_pages(
            vec![
                result("https://a.example/1"),
                result("https://a.example/2"),
                result("https://b.example/1"),
            ],
            2,
        );

        assert_eq!(pages[1].data.source_url, "https://b.example/1");
    }
}
