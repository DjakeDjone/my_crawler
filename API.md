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
  "max_pages": 50,
  "same_domain": true,
  "use_browser": false,
  "wait_for_selector": null,
  "wait_timeout_ms": 5000
}
```

**Parameters:**
- `url` (string, required): The starting URL to crawl. Must be a valid HTTP/HTTPS URL.
- `max_pages` (integer, required): Maximum number of pages to crawl.
- `same_domain` (boolean, optional): Only crawl pages from the same domain. Default is `true`.
- `use_browser` (boolean, optional): Force browser-based crawling for JavaScript-heavy sites. Default is `false`.
- `wait_for_selector` (string, optional): CSS selector to wait for before extracting content. Useful for dynamic SPAs. Default is `null`.
- `wait_timeout_ms` (integer, optional): Timeout in milliseconds for `wait_for_selector`. Default is `5000`.

**Browser Crawling Notes:**
- When `use_browser` is `true`, all pages are fetched using a headless Chromium browser
- When `use_browser` is `false` (default), HTTP client is used with automatic browser fallback for JS-rendered pages
- The `wait_for_selector` option is useful for SPAs where content loads asynchronously
- Set environment variable `SPIDER_BROWSER_HEADLESS=false` for debugging in headful mode

**Response (Success):**
```json
{
  "success": true,
  "message": "Queued crawl for https://example.com",
  "pages_crawled": 0,
  "pages_indexed": 0,
  "urls": ["https://example.com"]
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
- By default uses HTTP client; enable `use_browser` for JavaScript-rendered sites
- Very large websites with high max_pages values may take a long time to crawl

## Example Usage

### Using cURL

```bash
# Crawl up to 50 pages starting from example.com
curl -X POST http://localhost:8001/crawl \
  -H "Content-Type: application/json" \
  -d '{"url": "https://example.com", "max_pages": 50}'

# Crawl with browser rendering (for JavaScript-heavy sites)
curl -X POST http://localhost:8001/crawl \
  -H "Content-Type: application/json" \
  -d '{"url": "https://spa-app.com", "max_pages": 20, "use_browser": true}'

# Crawl SPA with selector waiting (wait for content to load)
curl -X POST http://localhost:8001/crawl \
  -H "Content-Type: application/json" \
  -d '{
    "url": "https://react-app.com",
    "max_pages": 10,
    "use_browser": true,
    "wait_for_selector": "#main-content",
    "wait_timeout_ms": 10000
  }'

# Crawl across domains
curl -X POST http://localhost:8001/crawl \
  -H "Content-Type: application/json" \
  -d '{"url": "https://example.com", "max_pages": 100, "same_domain": false}'
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