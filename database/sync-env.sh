#!/bin/bash

# sync-env.sh
# Syncs relevant environment variables from parent .env to local .env for docker-compose

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PARENT_ENV="${SCRIPT_DIR}/../.env"
LOCAL_ENV="${SCRIPT_DIR}/.env"

echo "Syncing environment variables..."
echo "Source: ${PARENT_ENV}"
echo "Target: ${LOCAL_ENV}"

# Check if parent .env exists
if [ ! -f "${PARENT_ENV}" ]; then
    echo "Error: Parent .env file not found at ${PARENT_ENV}"
    exit 1
fi

# Extract relevant variables for database services
grep -E "^(WEAVIATE_|OLLAMA_)" "${PARENT_ENV}" > "${LOCAL_ENV}" || {
    echo "Warning: No WEAVIATE_ or OLLAMA_ variables found in ${PARENT_ENV}"
    touch "${LOCAL_ENV}"
}

echo "✓ Environment variables synced successfully!"
echo ""
echo "Synced variables:"
cat "${LOCAL_ENV}"

exit 0
