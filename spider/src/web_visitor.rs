use url::Url;

pub async fn visit_webpage(url: &str) -> Result<String, String> {
    // print the url
    println!("Visiting webpage: {}", url);
    let url = Url::parse(url).expect("Invalid URL");
    let client = reqwest::Client::new();
    let response = client
        .get(url)
        .send()
        .await
        .expect("Failed to send request");
    let body = response.text().await.expect("Failed to read response body");

    Ok(body)
}
