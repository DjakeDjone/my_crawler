# Quick Start Guide

Get the crawler up and running in 5 minutes.

## Prerequisites

- Docker & Docker Compose installed
- Rust toolchain installed
- 8GB+ RAM available

## Steps

### 1. Setup Configuration (30 seconds)

```bash
cd crawler
cp .env.example .env
```

**Optional**: Edit `.env` to change ports if needed.

### 2. Start Docker Services (2 minutes)

```bash
cd database
docker-compose up -d
cd ..
```

Wait for services to initialize:
```bash
# Check if Weaviate is ready
curl http://localhost:8080/v1/.well-known/ready
```

Expected response: `{"status":"healthy"}`

### 3. Start API Server (1 minute)

In terminal 1:
```bash
cargo run --bin api
```

Wait for: `🚀 Starting API server on http://127.0.0.1:8000`

### 4. Start Crawler Server (1 minute)

In terminal 2:
```bash
cargo run --bin spider
```

Wait for: `🚀 Starting Crawler server on http://127.0.0.1:8001`

### 5. Test Everything (30 seconds)

```bash
# Test API server
curl http://localhost:8000/health

# Test Crawler server
curl http://localhost:8001/health
```

Both should return: `{"status":"ok",...}`

## First Crawl

```bash
curl -X POST http://localhost:8001/crawl \
  -H "Content-Type: application/json" \
  -d '{"url": "https://example.com", "depth": 1}'
```

## First Search

```bash
curl -X POST http://localhost:8000/search \
  -H "Content-Type: application/json" \
  -d '{"query": "example domain", "limit": 5}'
```

## View Your Configuration

```bash
./show-ports.sh
```

## Troubleshooting

### Port already in use?
```bash
./show-ports.sh  # Check which ports are in use
nano .env        # Change conflicting ports
```

### Docker issues?
```bash
cd database
docker-compose logs      # View logs
docker-compose restart   # Restart services
```

### Can't build Rust?
```bash
cargo clean
cargo build
```

## Next Steps

- Read [`README.md`](README.md) for full documentation
- See [`API.md`](API.md) for API details
- Check [`PORT_CONFIGURATION.md`](PORT_CONFIGURATION.md) for advanced port setup

## All Commands in One Script

```bash
# Save this as start.sh and run: chmod +x start.sh && ./start.sh

#!/bin/bash
set -e

echo "🔧 Setting up environment..."
cp -n .env.example .env || echo ".env already exists"

echo "🐳 Starting Docker services..."
cd database && docker-compose up -d && cd ..

echo "⏳ Waiting for Weaviate to be ready..."
for i in {1..30}; do
  if curl -s http://localhost:8080/v1/.well-known/ready > /dev/null 2>&1; then
    echo "✅ Weaviate is ready!"
    break
  fi
  echo "Waiting... ($i/30)"
  sleep 2
done

echo "🦀 Building Rust services..."
cargo build --bins

echo ""
echo "✅ Setup complete!"
echo ""
echo "Start the services:"
echo "  Terminal 1: cargo run --bin api"
echo "  Terminal 2: cargo run --bin spider"
echo ""
echo "Or run in background:"
echo "  cargo run --bin api > api.log 2>&1 &"
echo "  cargo run --bin spider > spider.log 2>&1 &"
```

## Stop Everything

```bash
# Stop Rust services
# Press Ctrl+C in each terminal

# Stop Docker services
cd database
docker-compose down
```

## Need Help?

- Run `./show-ports.sh` to check configuration
- Check logs: `docker-compose logs` (in database/)
- See full docs: [`README.md`](README.md)