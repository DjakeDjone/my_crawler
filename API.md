# Crawler API Documentation

## Base URL

The crawler service runs on `http://localhost:8001` by default (configurable via `SPIDER_HOST` and `SPIDER_PORT` environment variables).

## Endpoints

### Health Check

Check if the crawler service is running.

**Endpoint:** `GET /health`

**Response:**
```json
{
  "status": "ok",
  "message": "Crawler API is running"
}
```

---

### Crawl URL

Crawl a website starting from a given URL and index the pages into Weaviate.

**Endpoint:** `POST /crawl`

**Request Body:**
```json
{
  "url": "https://example.com",
  "depth": 2
}
```

**Parameters:**
- `url` (string, required): The starting URL to crawl. Must be a valid HTTP/HTTPS URL.
- `depth` (integer, optional): How deep to crawl. Default is `1`.
  - `1`: Only crawl the provided URL
  - `2`: Crawl the URL and all links found on that page
  - `3`: Crawl the URL, its links, and links found on those pages
  - And so on...

**Response (Success):**
```json
{
  "success": true,
  "message": "Successfully crawled 5 page(s) at depth 2",
  "pages_crawled": 5,
  "pages_indexed": 5,
  "urls": [
    "https://example.com",
    "https://example.com/about",
    "https://example.com/contact",
    "https://example.com/blog",
    "https://example.com/services"
  ]
}
```

**Response (Error - Invalid URL):**
```json
{
  "error": "Invalid URL: relative URL without a base"
}
```

**Response (Error - Schema Initialization):**
```json
{
  "error": "Failed to initialize database schema: connection error"
}
```

## Crawling Behavior

### Features

1. **Domain Restriction**: The crawler only follows links within the same domain as the starting URL to prevent crawling the entire internet.

2. **Duplicate Prevention**: URLs are only crawled once, even if they appear multiple times across different pages.

3. **Link Extraction**: The crawler extracts all `<a href>` links from HTML pages and resolves relative URLs to absolute URLs.

4. **Rate Limiting**: A 500ms delay is enforced between requests to be polite to web servers.

5. **Data Extraction**: For each page, the crawler extracts:
   - Page title
   - Meta description
   - Main content text
   - All links found on the page
   - Crawl timestamp

6. **Indexing**: All crawled pages are automatically indexed into Weaviate with vector embeddings for semantic search.

7. **Persistence**: Crawled URLs are stored in RocksDB to track crawling history.

### Limitations

- Only HTTP and HTTPS URLs are supported
- Fragment identifiers (#) in URLs are ignored
- The crawler does not execute JavaScript, so it only sees static HTML content
- Very large websites with high depth values may take a long time to crawl

## Example Usage

### Using cURL

```bash
# Crawl a single page
curl -X POST http://localhost:8001/crawl \
  -H "Content-Type: application/json" \
  -d '{"url": "https://example.com"}'

# Crawl a page and its immediate links
curl -X POST http://localhost:8001/crawl \
  -H "Content-Type: application/json" \
  -d '{"url": "https://example.com", "depth": 2}'

# Deep crawl (3 levels)
curl -X POST http://localhost:8001/crawl \
  -H "Content-Type: application/json" \
  -d '{"url": "https://example.com", "depth": 3}'
```

### Using JavaScript/Fetch

```javascript
async function crawlWebsite(url, depth = 1) {
  const response = await fetch('http://localhost:8001/crawl', {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
    },
    body: JSON.stringify({ url, depth }),
  });
  
  const result = await response.json();
  return result;
}

// Usage
crawlWebsite('https://example.com', 2)
  .then(result => console.log('Crawl complete:', result))
  .catch(error => console.error('Crawl failed:', error));
```

### Using Python

```python
import requests

def crawl_website(url, depth=1):
    response = requests.post(
        'http://localhost:8001/crawl',
        json={'url': url, 'depth': depth}
    )
    return response.json()

# Usage
result = crawl_website('https://example.com', depth=2)
print(f"Crawled {result['pages_crawled']} pages")
```

## Environment Variables

All environment variables can be configured in the `.env` file at the project root. See `PORT_CONFIGURATION.md` for detailed documentation.

- `SPIDER_HOST`: The host to bind the spider server to (default: `127.0.0.1`)
- `SPIDER_PORT`: The port to bind the spider server to (default: `8001`)
- `API_HOST`: The host to bind the API server to (default: `127.0.0.1`)
- `API_PORT`: The port to bind the API server to (default: `8000`)
- `WEAVIATE_URL`: The URL of the Weaviate instance (default: `http://localhost:8080`)
- `WEAVIATE_HOST_PORT`: The port to expose Weaviate on (default: `8080`)
- `WEAVIATE_GRPC_PORT`: The gRPC port for Weaviate (default: `50051`)

For more details on port configuration, run `./show-ports.sh` or see `PORT_CONFIGURATION.md`.

## Notes

- The crawler respects HTTP status codes and will skip pages that return errors
- Pages are indexed asynchronously, so the crawler doesn't block on indexing failures
- The User-Agent is set to `PoliteWebCrawler`
- Request timeout is set to 30 seconds per page