#!/usr/bin/env python3
"""
Bulk URL Crawler Script

Safely submits URLs from a file to the crawler API with:
- Domain filtering (skips auth pages, search engines, localhost)
- Domain-aware scheduling (spreads requests across domains)
- Progress tracking (resume after interruption)
- Rate limiting (configurable delays between submissions)

Usage:
    python3 bulk_crawl.py pages_to_crawl.txt
    python3 bulk_crawl.py --dry-run pages_to_crawl.txt
    python3 bulk_crawl.py --limit 10 pages_to_crawl.txt
"""

import argparse
import json
import os
import sys
import time
from collections import defaultdict
from itertools import zip_longest
from pathlib import Path
from urllib.parse import urlparse
import requests

def load_config(config_path: str = None) -> dict:
    """Load checked-in defaults, then optional overrides."""
    default_config_path = Path(__file__).parent / "bulk_crawl_config.json"
    with open(default_config_path) as f:
        config = json.load(f)
    if config_path:
        with open(config_path) as f:
            config.update(json.load(f))
    return config


def parse_domain(url: str) -> str:
    """Extract domain from URL."""
    try:
        parsed = urlparse(url)
        return parsed.netloc.lower()
    except Exception:
        return ""


def should_skip_url(url: str, config: dict) -> tuple[bool, str]:
    """Check if URL should be skipped. Returns (should_skip, reason)."""
    domain = parse_domain(url)
    
    if not domain:
        return True, "invalid URL"
    
    # Check domain skip list
    for skip_domain in config["skip_domains"]:
        if skip_domain in domain or domain in skip_domain:
            return True, f"domain '{skip_domain}' in skip list"
    
    # Check URL patterns
    url_lower = url.lower()
    for pattern in config["skip_patterns"]:
        if pattern in url_lower:
            return True, f"matches skip pattern '{pattern}'"
    
    # Skip non-http(s) URLs
    if not url.startswith(("http://", "https://")):
        return True, "not HTTP/HTTPS"
    
    return False, ""


def needs_browser(url: str, config: dict) -> bool:
    """Check if URL needs browser rendering."""
    domain = parse_domain(url)
    return any(bd in domain for bd in config["browser_domains"])


def load_urls(filepath: str) -> list[str]:
    """Load URLs from file, one per line."""
    with open(filepath) as f:
        return [url for line in f if (url := line.strip()) and not url.startswith('#')]


def load_progress(progress_file: str) -> set[str]:
    """Load already-processed URLs from progress file."""
    if not os.path.exists(progress_file):
        return set()
    
    with open(progress_file, 'r') as f:
        return set(line.strip() for line in f if line.strip())


def save_progress(progress_file: str, url: str):
    """Append URL to progress file."""
    with open(progress_file, 'a') as f:
        f.write(url + '\n')


def submit_crawl(url: str, config: dict, dry_run: bool = False) -> bool:
    """Submit a single URL to the crawler API."""
    if dry_run:
        print(f"  [DRY-RUN] Would crawl: {url}")
        return True
    
    payload = {
        "url": url,
        "max_pages": config["max_pages_per_url"],
        "same_domain": config["same_domain"],
        "use_browser": needs_browser(url, config),
    }
    
    try:
        response = requests.post(
            f"{config['crawler_url']}/crawl",
            json=payload,
            timeout=10,
        )
        
        if response.status_code == 200:
            return True
        else:
            print(f"  ⚠️ Error submitting {url}: HTTP {response.status_code}")
            return False
    
    except requests.exceptions.RequestException as e:
        print(f"  ❌ Request failed for {url}: {e}")
        return False


def group_by_domain(urls: list[str]) -> dict[str, list[str]]:
    """Group URLs by their domain."""
    groups = defaultdict(list)
    for url in urls:
        domain = parse_domain(url)
        groups[domain].append(url)
    return dict(groups)


def interleave_domains(domain_groups: dict[str, list[str]]) -> list[str]:
    """Interleave URLs from different domains to spread load."""
    return [
        url
        for row in zip_longest(*domain_groups.values())
        for url in row
        if url is not None
    ]


def main():
    parser = argparse.ArgumentParser(
        description="Bulk submit URLs to crawler API safely"
    )
    parser.add_argument("input_file", help="File containing URLs (one per line)")
    parser.add_argument("--config", help="JSON config file")
    parser.add_argument("--dry-run", action="store_true", 
                        help="Don't actually submit, just show what would be done")
    parser.add_argument("--limit", type=int, 
                        help="Limit number of URLs to process")
    parser.add_argument("--no-resume", action="store_true",
                        help="Start fresh, ignore previous progress")
    
    args = parser.parse_args()
    
    # Load config
    config = load_config(args.config)
    
    # Check crawler health (unless dry run)
    if not args.dry_run:
        try:
            response = requests.get(f"{config['crawler_url']}/health", timeout=5)
            if response.status_code != 200:
                print(f"❌ Crawler not healthy: {response.status_code}")
                sys.exit(1)
            print(f"✅ Crawler is running at {config['crawler_url']}")
        except requests.exceptions.RequestException as e:
            print(f"❌ Cannot connect to crawler at {config['crawler_url']}: {e}")
            print("   Make sure the spider is running: cargo run --bin spider")
            sys.exit(1)
    
    # Load URLs
    print(f"📂 Loading URLs from {args.input_file}...")
    all_urls = load_urls(args.input_file)
    print(f"   Found {len(all_urls)} URLs")
    
    # Load progress
    progress_file = args.input_file + ".progress"
    if args.no_resume:
        processed = set()
        if os.path.exists(progress_file):
            os.remove(progress_file)
    else:
        processed = load_progress(progress_file)
        if processed:
            print(f"   Resuming: {len(processed)} already processed")
    
    # Filter URLs
    print("🔍 Filtering URLs...")
    filtered_urls = []
    skipped_count = 0
    skip_reasons = defaultdict(int)
    
    for url in all_urls:
        if url in processed:
            continue
        
        should_skip, reason = should_skip_url(url, config)
        if should_skip:
            skipped_count += 1
            skip_reasons[reason] += 1
        else:
            filtered_urls.append(url)
    
    print(f"   Skipped {skipped_count} URLs:")
    for reason, count in sorted(skip_reasons.items(), key=lambda x: -x[1])[:5]:
        print(f"     - {reason}: {count}")
    print(f"   Remaining: {len(filtered_urls)} URLs to crawl")
    
    # Apply limit
    if args.limit:
        filtered_urls = filtered_urls[:args.limit]
        print(f"   Limited to: {len(filtered_urls)} URLs")
    
    if not filtered_urls:
        print("✅ Nothing to do!")
        return
    
    # Group and interleave by domain for better spreading
    print("📊 Organizing by domain...")
    domain_groups = group_by_domain(filtered_urls)
    print(f"   {len(domain_groups)} unique domains")
    
    ordered_urls = interleave_domains(domain_groups)
    
    # Submit URLs
    print()
    print("🚀 Starting crawl submissions...")
    print(f"   Delay between submissions: {config['delay_between_submissions_ms']}ms")
    print(f"   Batch size: {config['batch_size']}")
    print()
    
    submitted = 0
    failed = 0
    start_time = time.time()
    
    for i, url in enumerate(ordered_urls, 1):
        domain = parse_domain(url)
        browser_mode = "🌐" if needs_browser(url, config) else "📄"
        print(f"[{i}/{len(ordered_urls)}] {browser_mode} {domain}: {url[:60]}...")
        
        success = submit_crawl(url, config, dry_run=args.dry_run)
        
        if success:
            submitted += 1
            if not args.dry_run:
                save_progress(progress_file, url)
        else:
            failed += 1
        
        if i < len(ordered_urls):
            if i % config["batch_size"] == 0:
                print(f"   ⏸️ Batch pause ({config['delay_between_batches_ms']}ms)...")
                time.sleep(config["delay_between_batches_ms"] / 1000)
            else:
                time.sleep(config["delay_between_submissions_ms"] / 1000)
    
    # Summary
    elapsed = time.time() - start_time
    print()
    print("=" * 50)
    print("📈 Summary")
    print("=" * 50)
    print(f"   Total URLs processed: {len(ordered_urls)}")
    print(f"   Submitted: {submitted}")
    print(f"   Failed: {failed}")
    print(f"   Time: {elapsed:.1f}s")
    if not args.dry_run:
        print(f"   Progress saved to: {progress_file}")
    print()
    print("✅ Done! The crawler is processing pages in the background.")
    print("   Monitor progress with: tail -f (spider logs)")


if __name__ == "__main__":
    main()
