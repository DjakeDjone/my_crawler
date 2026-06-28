# Dockerized Qdrant Search Crawler

Rust crawler and search API using Qdrant hybrid retrieval and TEI dense embeddings.

Production operations: [DEPLOYMENT.md](DEPLOYMENT.md)

## Start

```bash
cp .env.example .env
# Set real CRAWLER_PRODUCT_TOKEN and CRAWLER_USER_AGENT values.
docker compose up -d --wait
```

Services:

- Search API: `http://localhost:8000`
- Crawler API: `http://localhost:8001`
- Qdrant and TEI: internal Compose network only

## Crawl

```bash
curl -X POST http://localhost:8001/crawl \
  -H 'content-type: application/json' \
  -d '{"url":"https://example.com","max_pages":50,"same_domain":true}'
```

## Search

```bash
curl 'http://localhost:8000/search?query=example&limit=10'
```

## Verify

```bash
cargo test --workspace
cargo clippy --workspace --all-targets
docker compose config
docker compose build
docker compose up -d --wait
```

The first startup downloads `intfloat/multilingual-e5-small` into the named
`model-cache` volume. Qdrant data is stored in `qdrant-data`.
