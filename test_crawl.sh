#!/bin/bash

# Test script for the crawler API
# Make sure the crawler service is running on localhost:8001

BASE_URL="${BASE_URL:-http://localhost:8001}"

echo "🧪 Testing Crawler API"
echo "======================"
echo ""

# Test 1: Health Check
echo "Test 1: Health Check"
echo "--------------------"
curl -s "${BASE_URL}/health" | jq '.'
echo ""
echo ""

# Test 2: Crawl a single page
echo "Test 2: Crawl Single Page"
echo "-------------------------"
curl -s -X POST "${BASE_URL}/crawl" \
  -H "Content-Type: application/json" \
  -d '{"url": "https://example.com", "max_pages": 1, "max_depth": 1}' | jq '.'
echo ""
echo ""

# Test 3: Crawl with max_depth 2
echo "Test 3: Crawl with Max Depth 2"
echo "------------------------------"
curl -s -X POST "${BASE_URL}/crawl" \
  -H "Content-Type: application/json" \
  -d '{"url": "https://example.com", "max_pages": 5, "max_depth": 2}' | jq '.'
echo ""
echo ""

# Test 4: Invalid URL
echo "Test 4: Invalid URL (should fail)"
echo "---------------------------------"
curl -s -X POST "${BASE_URL}/crawl" \
  -H "Content-Type: application/json" \
  -d '{"url": "not-a-valid-url"}' | jq '.'
echo ""
echo ""

# Test 5: Default max_depth
echo "Test 5: Default Max Depth"
echo "-------------------------"
curl -s -X POST "${BASE_URL}/crawl" \
  -H "Content-Type: application/json" \
  -d '{"url": "https://example.com", "max_pages": 1}' | jq '.'
echo ""
echo ""

echo "✅ All tests completed!"
