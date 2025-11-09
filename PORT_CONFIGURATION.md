# Port Configuration Guide

This document explains how to configure all ports used by the crawler project from a single `.env` file.

## Overview

All port configurations for the crawler project are centralized in the `.env` file located at the project root. This includes:

- **API Server** (Rust service for search and plagiarism detection)
- **Spider/Crawler Server** (Rust service for web crawling)
- **Weaviate Vector Database** (Docker container)
- **Ollama** (Docker container for embeddings)

## Setup Instructions

1. **Copy the example configuration:**

   ```bash
   cp .env.example .env
   ```

2. **Edit `.env` to customize ports:**

   ```bash
   nano .env  # or use your preferred editor
   ```

3. **Start the services:**

   ```bash
   # Start Docker services (Weaviate + Ollama)
   cd database
   docker-compose up -d
   
   # Start API server (from project root)
   cargo run --bin api
   
   # Start Spider/Crawler server (from project root)
   cargo run --bin spider
   ```

## Environment Variables Reference

### API Server Configuration

- `API_HOST` - Host address for the API server (default: `127.0.0.1`)
- `API_PORT` - Port for the API server (default: `8000`)

**Example:**

```env
API_HOST=127.0.0.1
API_PORT=8000
```

### Spider/Crawler Server Configuration

- `SPIDER_HOST` - Host address for the crawler server (default: `127.0.0.1`)
- `SPIDER_PORT` - Port for the crawler server (default: `8001`)

**Example:**

```env
SPIDER_HOST=127.0.0.1
SPIDER_PORT=8001
```

### Weaviate Database Configuration

- `WEAVIATE_HOST_PORT` - External port to access Weaviate (default: `8080`)
- `WEAVIATE_GRPC_PORT` - External gRPC port for Weaviate (default: `50051`)
- `WEAVIATE_INTERNAL_PORT` - Internal port Weaviate listens on (default: `8080`)
- `WEAVIATE_URL` - Full URL used by Rust services to connect to Weaviate

**Example:**

```env
WEAVIATE_HOST_PORT=8080
WEAVIATE_GRPC_PORT=50051
WEAVIATE_INTERNAL_PORT=8080
WEAVIATE_URL=http://localhost:8080
```

### Ollama Configuration

- `OLLAMA_HOST_PORT` - Optional port to expose Ollama (commented out by default)

**Example (if you want to expose Ollama):**

```env
OLLAMA_HOST_PORT=11434
```

Then update `docker-compose.yml` to expose the port:

```yaml
ollama:
  ports:
    - "${OLLAMA_HOST_PORT:-11434}:11434"
```

## Port Conflict Resolution

If you encounter port conflicts, simply change the port numbers in your `.env` file:

### Example: Running multiple instances

**Instance 1 (.env):**

```env
API_PORT=8000
SPIDER_PORT=8001
WEAVIATE_HOST_PORT=8080
WEAVIATE_GRPC_PORT=50051
```

**Instance 2 (.env.dev):**

```env
API_PORT=9000
SPIDER_PORT=9001
WEAVIATE_HOST_PORT=9080
WEAVIATE_GRPC_PORT=50052
WEAVIATE_URL=http://localhost:9080
```

To use a different env file with Docker Compose:

```bash
docker-compose --env-file ../.env.dev up -d
```

## Default Port Allocation

| Service | Default Port | Purpose |
|---------|-------------|---------|
| API Server | 8000 | REST API for search and plagiarism detection |
| Spider Server | 8001 | Crawler API for indexing web pages |
| Weaviate HTTP | 8080 | Vector database HTTP API |
| Weaviate gRPC | 50051 | Vector database gRPC interface |

## Troubleshooting

### Port Already in Use

If you see an error like "Address already in use", check which process is using the port:

```bash
# Linux/Mac
lsof -i :8080
netstat -tulpn | grep 8080

# Windows
netstat -ano | findstr :8080
```

Then either stop that process or change the port in `.env`.

### Docker Compose Not Reading .env

Make sure:

1. The `.env` file exists in the `crawler/` directory
2. You're running `docker-compose` from the `crawler/database/` directory
3. The `env_file` directive points to the correct path: `../.env`

### Rust Services Not Using New Ports

After changing `.env`, you need to restart the Rust services:

```bash
# Stop the running service (Ctrl+C)
# Then restart
cargo run --bin api
# or
cargo run --bin spider
```

## Security Notes

- The `.env` file is excluded from version control via `.gitignore`
- Never commit the `.env` file with sensitive data
- Use `.env.example` as a template for other developers
- Consider using different hosts (not just `127.0.0.1`) if you need external access
- When exposing services externally, use proper authentication and HTTPS

## Testing Configuration

To verify your configuration is working:

```bash
# Test API server
curl http://localhost:${API_PORT}/health

# Test Spider server
curl http://localhost:${SPIDER_PORT}/health

# Test Weaviate
curl http://localhost:${WEAVIATE_HOST_PORT}/v1/.well-known/ready
```

Replace `${VARIABLE}` with your actual port numbers from `.env`.
