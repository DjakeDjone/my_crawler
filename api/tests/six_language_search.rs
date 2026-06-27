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
        assert!(response["results"].is_array(), "{query}");
    }
}
