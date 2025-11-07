#!/bin/bash

# Test script for the crawler API
# Make sure the crawler service is running on localhost:8001

BASE_URL="http://localhost:8001"

echo "🧪 Testing Crawler API"
echo "======================"
echo ""

# Test 1: Health Check
echo "Test 1: Health Check"
echo "--------------------"
curl -s "${BASE_URL}/health" | jq '.'
echo ""
echo ""

# Test 2: Crawl a single page (depth 1)
echo "Test 2: Crawl Single Page (depth=1)"
echo "------------------------------------"
curl -s -X POST "${BASE_URL}/crawl" \
  -H "Content-Type: application/json" \
  -d '{"url": "https://example.com", "depth": 1}' | jq '.'
echo ""
echo ""

# Test 3: Crawl with depth 2
echo "Test 3: Crawl with Depth 2"
echo "--------------------------"
curl -s -X POST "${BASE_URL}/crawl" \
  -H "Content-Type: application/json" \
  -d '{"url": "https://example.com", "depth": 2}' | jq '.'
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

# Test 5: Default depth (omitted)
echo "Test 5: Default Depth (omitted, defaults to 1)"
echo "-----------------------------------------------"
curl -s -X POST "${BASE_URL}/crawl" \
  -H "Content-Type: application/json" \
  -d '{"url": "https://example.com"}' | jq '.'
echo ""
echo ""

echo "✅ All tests completed!"
