#!/bin/bash
set -a
[ -f .env ] && source .env
set +a

api_port=${API_PORT:-8000}
spider_port=${SPIDER_PORT:-8001}

echo "API:     http://${API_HOST:-127.0.0.1}:$api_port"
echo "Spider:  http://${SPIDER_HOST:-127.0.0.1}:$spider_port"
echo "Qdrant:  ${QDRANT_URL:-http://qdrant:6334}"
echo "TEI:     ${TEI_URL:-http://tei}"

if command -v ss >/dev/null; then
    for port in "$api_port" "$spider_port"; do
        if ss -ltn "sport = :$port" | grep -q LISTEN; then
            echo "Port $port: in use"
        else
            echo "Port $port: available"
        fi
    done
fi
