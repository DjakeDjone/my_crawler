# Web Crawler & Semantic Search System

A Rust-based web crawler with semantic search capabilities using Weaviate vector database and Ollama embeddings.

## 🚀 Features

- **Web Crawling**: Recursively crawl websites with configurable depth
- **Semantic Search**: Vector-based search using embeddings
- **Plagiarism Detection**: Check text similarity against indexed content
- **Configurable Ports**: All ports managed from a single `.env` file
- **Persistent Storage**: RocksDB for crawl history, Weaviate for vector data
- **REST APIs**: Separate services for crawling and searching

## 📋 Prerequisites

- Rust (latest stable version)
- Docker and Docker Compose
- 8GB+ RAM recommended for Ollama and Weaviate

## 🔧 Installation & Setup

### 1. Clone and Setup Environment

```bash
# Navigate to project directory
cd crawler

# Copy environment configuration
cp .env.example .env

# (Optional) Edit .env to customize ports
nano .env
```

### 2. Start Docker Services

```bash
cd database
docker-compose up -d
cd ..
```

This starts:
- **Weaviate** vector database (port 8080 by default)
- **Ollama** for embeddings (internal only)

### 3. Start Rust Services

```bash
# Start the API server (search & plagiarism detection)
cargo run --bin api

# In another terminal, start the Spider/Crawler server
cargo run --bin spider
```

## 🎯 Port Configuration

All ports are configured in the `.env` file at the project root:

```env
# API Server
API_HOST=127.0.0.1
API_PORT=8000

# Spider/Crawler Server
SPIDER_HOST=127.0.0.1
SPIDER_PORT=8001

# Weaviate Vector Database
WEAVIATE_HOST_PORT=8080
WEAVIATE_GRPC_PORT=50051
WEAVIATE_URL=http://localhost:8080
```

### View Current Configuration

```bash
./show-ports.sh
```

This script displays:
- All configured ports
- Port availability status
- Quick start commands

### Detailed Port Documentation

See [`PORT_CONFIGURATION.md`](PORT_CONFIGURATION.md) for:
- Complete environment variable reference
- Port conflict resolution
- Multiple instance setup
- Security considerations

## 📚 API Documentation

### API Server (Default: http://localhost:8000)

#### Health Check
```bash
curl http://localhost:8000/health
```

#### Search
```bash
curl -X POST http://localhost:8000/search \
  -H "Content-Type: application/json" \
  -d '{"query": "machine learning", "limit": 10}'
```

#### Plagiarism Check
```bash
curl -X POST http://localhost:8000/plagiat \
  -H "Content-Type: application/json" \
  -d '{"text": "Your text here", "threshold": 0.6}'
```

### Spider/Crawler Server (Default: http://localhost:8001)

#### Health Check
```bash
curl http://localhost:8001/health
```

#### Crawl Website
```bash
curl -X POST http://localhost:8001/crawl \
  -H "Content-Type: application/json" \
  -d '{"url": "https://example.com", "depth": 2, "max_pages": 100}'
```

For detailed API documentation, see [`API.md`](API.md).

## 🏗️ Project Structure

```
crawler/
├── .env                    # Port configuration (create from .env.example)
├── .env.example           # Template for environment variables
├── show-ports.sh          # Script to display port configuration
├── PORT_CONFIGURATION.md  # Detailed port configuration guide
├── API.md                 # API endpoint documentation
├── api/                   # Search & plagiarism API server
│   └── src/
│       └── main.rs
├── spider/                # Web crawler service
│   └── src/
│       └── main.rs
├── types/                 # Shared data types
├── database/              # Docker Compose setup
│   ├── docker-compose.yml
│   ├── weaviate_data/     # Weaviate persistent data
│   └── ollama_data/       # Ollama model data
└── target/                # Rust build artifacts
```

## 🛠️ Development

### Building

```bash
# Build all binaries
cargo build

# Build in release mode (optimized)
cargo build --release
```

### Running Tests

```bash
cargo test
```

### Checking Code

```bash
# Check for compilation errors
cargo check

# Format code
cargo fmt

# Lint code
cargo clippy
```

## 🔍 Usage Examples

### Crawl and Index a Website

```bash
# Crawl example.com and all linked pages (depth 2)
curl -X POST http://localhost:8001/crawl \
  -H "Content-Type: application/json" \
  -d '{
    "url": "https://example.com",
    "depth": 2,
    "max_pages": 50
  }'
```

### Search Indexed Content

```bash
# Search for pages about "rust programming"
curl -X POST http://localhost:8000/search \
  -H "Content-Type: application/json" \
  -d '{
    "query": "rust programming tutorials",
    "limit": 5
  }'
```

### Check for Plagiarism

```bash
# Check if text is similar to indexed content
curl -X POST http://localhost:8000/plagiat \
  -H "Content-Type: application/json" \
  -d '{
    "text": "Your article or text content here...",
    "threshold": 0.7
  }'
```

## 🐛 Troubleshooting

### Port Already in Use

Check which process is using the port:
```bash
# Linux/Mac
lsof -i :8080
netstat -tulpn | grep 8080

# Or use the show-ports.sh script
./show-ports.sh
```

Then either stop that process or change the port in `.env`.

### Docker Services Won't Start

```bash
# Check Docker status
docker ps

# View logs
cd database
docker-compose logs

# Restart services
docker-compose restart
```

### Weaviate Connection Issues

1. Ensure Weaviate is running: `docker-compose ps`
2. Check `WEAVIATE_URL` in `.env` matches the exposed port
3. Test connection: `curl http://localhost:8080/v1/.well-known/ready`

### Rust Build Errors

```bash
# Clean build cache
cargo clean

# Update dependencies
cargo update

# Rebuild
cargo build
```

## 📊 Performance Considerations

- **Crawling Rate**: 500ms delay between requests (configurable in code)
- **Memory Usage**: Weaviate requires ~2GB, Ollama ~4GB for models
- **Disk Space**: Vector database grows with indexed content
- **Concurrent Crawls**: Not currently supported (single-threaded crawler)

## 🔒 Security Notes

- `.env` file is gitignored - never commit it
- Default configuration binds to `127.0.0.1` (localhost only)
- For external access, update `API_HOST`/`SPIDER_HOST` in `.env`
- Add authentication before exposing to the internet
- Use HTTPS in production environments

## 🤝 Contributing

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Run tests and linting
5. Submit a pull request

## 📝 License

[Add your license here]

## 🙏 Acknowledgments

- [Weaviate](https://weaviate.io/) - Vector database
- [Ollama](https://ollama.ai/) - Local embeddings
- [Actix-web](https://actix.rs/) - Rust web framework
- [RocksDB](https://rocksdb.org/) - Persistent key-value store

## 📧 Contact

[Add your contact information here]

---

**Quick Start Summary:**
```bash
# 1. Setup
cp .env.example .env

# 2. Start Docker services
cd database && docker-compose up -d && cd ..

# 3. Start API server
cargo run --bin api &

# 4. Start Spider server
cargo run --bin spider &

# 5. Test
curl http://localhost:8000/health
curl http://localhost:8001/health

# 6. View configuration
./show-ports.sh
```
