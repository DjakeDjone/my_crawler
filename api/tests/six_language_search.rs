use serde_json::Value;

#[tokio::test]
#[ignore = "requires a running populated API; set SEARCH_BENCHMARK_URL"]
async fn six_language_search_benchmark() {
    let base = std::env::var("SEARCH_BENCHMARK_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8000".to_string());
    let client = reqwest::Client::new();
    for query in [
        "rust web crawler",
        "Web-Crawler auf Rust",
        "поисковый робот Rust",
        "زاحف ويب بلغة رست",
        "Rust 网络爬虫",
        "Rastreador web en Rust",
    ] {
        let response = client
            .get(format!("{}/search", base.trim_end_matches('/')))
            .query(&[("query", query), ("limit", "5")])
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json::<Value>()
            .await
            .unwrap();
        let results = response["results"].as_array().unwrap();
        assert!(!results.is_empty(), "{query}");
        assert!(
            results
                .iter()
                .any(|result| result_text(result).contains("rust")),
            "{query}: expected at least one top result to mention Rust"
        );
    }
}

fn result_text(result: &Value) -> String {
    [
        "source_url",
        "page_title",
        "description",
        "chunk_heading",
        "chunk_content",
    ]
    .into_iter()
    .filter_map(|key| result[key].as_str())
    .collect::<Vec<_>>()
    .join(" ")
    .to_lowercase()
}
