#!/bin/bash

# Script to display current port configuration from .env file
# Usage: ./show-ports.sh

# Colors for output
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Get script directory
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
ENV_FILE="$SCRIPT_DIR/.env"

echo -e "${BLUE}=== Crawler Project Port Configuration ===${NC}\n"

# Check if .env exists
if [ ! -f "$ENV_FILE" ]; then
    echo -e "${YELLOW}Warning: .env file not found!${NC}"
    echo "Creating from .env.example..."
    if [ -f "$SCRIPT_DIR/.env.example" ]; then
        cp "$SCRIPT_DIR/.env.example" "$ENV_FILE"
        echo -e "${GREEN}Created .env file${NC}\n"
    else
        echo -e "${YELLOW}Error: .env.example not found. Please create .env manually.${NC}"
        exit 1
    fi
fi

# Source the .env file
export $(grep -v '^#' "$ENV_FILE" | grep -v '^$' | xargs)

# Display configuration
echo -e "${GREEN}API Server:${NC}"
echo "  Host: ${API_HOST:-127.0.0.1}"
echo "  Port: ${API_PORT:-8000}"
echo "  URL:  http://${API_HOST:-127.0.0.1}:${API_PORT:-8000}"
echo ""

echo -e "${GREEN}Spider/Crawler Server:${NC}"
echo "  Host: ${SPIDER_HOST:-127.0.0.1}"
echo "  Port: ${SPIDER_PORT:-8001}"
echo "  URL:  http://${SPIDER_HOST:-127.0.0.1}:${SPIDER_PORT:-8001}"
echo ""

echo -e "${GREEN}Weaviate Vector Database:${NC}"
echo "  HTTP Port: ${WEAVIATE_HOST_PORT:-8080}"
echo "  gRPC Port: ${WEAVIATE_GRPC_PORT:-50051}"
echo "  Internal Port: ${WEAVIATE_INTERNAL_PORT:-8080}"
echo "  Connection URL: ${WEAVIATE_URL:-http://localhost:8080}"
echo ""

echo -e "${GREEN}Ollama:${NC}"
if [ -z "$OLLAMA_HOST_PORT" ]; then
    echo "  Port: Not exposed (internal only)"
else
    echo "  Port: $OLLAMA_HOST_PORT"
fi
echo ""

# Check for port conflicts
echo -e "${BLUE}=== Checking Port Availability ===${NC}\n"

check_port() {
    local port=$1
    local service=$2

    if command -v lsof > /dev/null 2>&1; then
        if lsof -Pi :$port -sTCP:LISTEN -t >/dev/null 2>&1 ; then
            echo -e "${YELLOW}⚠ Port $port ($service) is already in use${NC}"
        else
            echo -e "${GREEN}✓ Port $port ($service) is available${NC}"
        fi
    elif command -v netstat > /dev/null 2>&1; then
        if netstat -tuln | grep -q ":$port " ; then
            echo -e "${YELLOW}⚠ Port $port ($service) is already in use${NC}"
        else
            echo -e "${GREEN}✓ Port $port ($service) is available${NC}"
        fi
    else
        echo -e "${YELLOW}⚠ Cannot check port $port (no lsof or netstat available)${NC}"
    fi
}

check_port "${API_PORT:-8000}" "API Server"
check_port "${SPIDER_PORT:-8001}" "Spider Server"
check_port "${WEAVIATE_HOST_PORT:-8080}" "Weaviate HTTP"
check_port "${WEAVIATE_GRPC_PORT:-50051}" "Weaviate gRPC"

if [ ! -z "$OLLAMA_HOST_PORT" ]; then
    check_port "$OLLAMA_HOST_PORT" "Ollama"
fi

echo ""
echo -e "${BLUE}=== Quick Start Commands ===${NC}\n"
echo "Start Docker services:"
echo "  cd database && docker-compose up -d"
echo ""
echo "Start API server:"
echo "  cargo run --bin api"
echo ""
echo "Start Spider server:"
echo "  cargo run --bin spider"
echo ""
echo "Test endpoints:"
echo "  curl http://${API_HOST:-127.0.0.1}:${API_PORT:-8000}/health"
echo "  curl http://${SPIDER_HOST:-127.0.0.1}:${SPIDER_PORT:-8001}/health"
echo ""
