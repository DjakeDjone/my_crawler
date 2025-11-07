# Port Configuration Changes

## Overview

This document describes the changes made to centralize port configuration across all services in the crawler project.

## Date: 2024

## Changes Made

### 1. Created Global Configuration Files

- **`.env`**: Main configuration file containing all port settings (gitignored)
- **`.env.example`**: Template file for developers to create their own `.env`
- Both files include configurations for:
  - API Server (API_HOST, API_PORT)
  - Spider/Crawler Server (SPIDER_HOST, SPIDER_PORT)
  - Weaviate Database (WEAVIATE_HOST_PORT, WEAVIATE_GRPC_PORT, WEAVIATE_INTERNAL_PORT, WEAVIATE_URL)
  - Ollama (optional OLLAMA_HOST_PORT)

### 2. Updated Docker Compose Configuration

**File**: `database/docker-compose.yml`

- Added `env_file` directive to load `../.env` for both services
- Changed hardcoded ports to environment variables:
  - `8080` → `${WEAVIATE_HOST_PORT:-8080}`
  - `50051` → `${WEAVIATE_GRPC_PORT:-50051}`
  - Internal port: `${WEAVIATE_INTERNAL_PORT:-8080}`
- Uses default values if environment variables are not set

### 3. Updated Rust Applications

**File**: `api/src/main.rs`
- Changed `HOST` → `API_HOST`
- Changed `PORT` → `API_PORT`
- Still reads `WEAVIATE_URL` for database connection

**File**: `spider/src/main.rs`
- Changed `HOST` → `SPIDER_HOST`
- Changed `PORT` → `SPIDER_PORT`
- Still reads `WEAVIATE_URL` for database connection

### 4. Updated .gitignore

**File**: `.gitignore`
- Added `.env` to prevent committing sensitive configuration

### 5. Created Documentation

- **`PORT_CONFIGURATION.md`**: Comprehensive guide covering:
  - Environment variable reference
  - Setup instructions
  - Port conflict resolution
  - Multiple instance configuration
  - Security considerations
  - Troubleshooting

- **`README.md`**: New project README including:
  - Quick start guide
  - Port configuration overview
  - API documentation links
  - Project structure
  - Usage examples
  - Troubleshooting section

- **`API.md`**: Updated to reference new environment variable names

- **`CHANGES.md`**: This file documenting all changes

### 6. Created Utility Script

**File**: `show-ports.sh`
- Displays current port configuration from `.env`
- Checks port availability on the system
- Shows quick start commands
- Helps with troubleshooting
- Made executable with `chmod +x`

## Migration Guide

### For Existing Deployments

If you were previously using `HOST` and `PORT` environment variables:

**Old configuration:**
```bash
export HOST=127.0.0.1
export PORT=8000  # for api
export PORT=8001  # for spider
```

**New configuration in `.env`:**
```env
API_HOST=127.0.0.1
API_PORT=8000
SPIDER_HOST=127.0.0.1
SPIDER_PORT=8001
```

### For Docker Compose

If you were using custom ports in `docker-compose.yml`:

**Old:**
```yaml
ports:
  - 8080:8080
```

**New (in .env):**
```env
WEAVIATE_HOST_PORT=8080
```

## Benefits

1. **Single Source of Truth**: All ports configured in one place
2. **Easy Customization**: Change ports without editing multiple files
3. **Port Conflict Resolution**: Simple to resolve conflicts by editing `.env`
4. **Multiple Environments**: Easy to maintain different configurations (dev, staging, prod)
5. **Documentation**: Clear documentation of all available ports
6. **Developer Friendly**: `.env.example` provides a template for new developers
7. **Validation**: `show-ports.sh` helps verify configuration and check availability

## Backward Compatibility

- All environment variables have sensible defaults
- If `.env` is not present, services will use default ports:
  - API Server: 127.0.0.1:8000
  - Spider Server: 127.0.0.1:8001
  - Weaviate: 8080 (HTTP), 50051 (gRPC)

## Testing

After making these changes, verify everything works:

```bash
# 1. View configuration
./show-ports.sh

# 2. Start Docker services
cd database && docker-compose up -d && cd ..

# 3. Start Rust services
cargo run --bin api &
cargo run --bin spider &

# 4. Test endpoints
curl http://localhost:8000/health
curl http://localhost:8001/health
curl http://localhost:8080/v1/.well-known/ready
```

## Related Files

- `.env` - Main configuration file
- `.env.example` - Template configuration
- `PORT_CONFIGURATION.md` - Detailed documentation
- `README.md` - Project overview and quick start
- `API.md` - API endpoint documentation
- `show-ports.sh` - Configuration display script
- `database/docker-compose.yml` - Docker service definitions
- `api/src/main.rs` - API server implementation
- `spider/src/main.rs` - Spider server implementation

## Notes

- The `.env` file is gitignored for security
- Always use `.env.example` as the template
- Document any new environment variables in both `.env.example` and `PORT_CONFIGURATION.md`
- Update `show-ports.sh` if adding new services with ports