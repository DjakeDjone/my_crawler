# Quick Start

```bash
cp .env.example .env
```

Set production crawler identity in `.env`:

```env
CRAWLER_PRODUCT_TOKEN=MySearchBot
CRAWLER_USER_AGENT=MySearchBot/1.0 (+https://example.com/bot; contact@example.com)
```

Start and verify:

```bash
docker compose up -d --wait
docker compose ps
curl http://localhost:8000/health
curl http://localhost:8001/health
```

Stop with `docker compose down`.
