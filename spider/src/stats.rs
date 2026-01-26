//! Statistics tracking for the crawler
//!
//! Provides thread-safe atomic counters for monitoring crawler performance.

use std::sync::atomic::{AtomicUsize, Ordering};
use serde::Serialize;

/// Thread-safe statistics counters for the crawler
#[derive(Default)]
pub struct CrawlStats {
    pub pages_crawled: AtomicUsize,
    pub pages_failed: AtomicUsize,
    pub pages_skipped_robots: AtomicUsize,
    pub pages_skipped_dedup: AtomicUsize,
    pub pages_skipped_depth: AtomicUsize,
    pub retries_attempted: AtomicUsize,
}

impl CrawlStats {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn inc_crawled(&self) {
        self.pages_crawled.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_failed(&self) {
        self.pages_failed.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_skipped_robots(&self) {
        self.pages_skipped_robots.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_skipped_dedup(&self) {
        self.pages_skipped_dedup.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_skipped_depth(&self) {
        self.pages_skipped_depth.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_retries(&self) {
        self.retries_attempted.fetch_add(1, Ordering::Relaxed);
    }

    /// Get a snapshot of current stats
    pub fn snapshot(&self) -> StatsSnapshot {
        StatsSnapshot {
            pages_crawled: self.pages_crawled.load(Ordering::Relaxed),
            pages_failed: self.pages_failed.load(Ordering::Relaxed),
            pages_skipped_robots: self.pages_skipped_robots.load(Ordering::Relaxed),
            pages_skipped_dedup: self.pages_skipped_dedup.load(Ordering::Relaxed),
            pages_skipped_depth: self.pages_skipped_depth.load(Ordering::Relaxed),
            retries_attempted: self.retries_attempted.load(Ordering::Relaxed),
        }
    }
}

/// Serializable snapshot of stats for the status endpoint
#[derive(Serialize, Clone)]
pub struct StatsSnapshot {
    pub pages_crawled: usize,
    pub pages_failed: usize,
    pub pages_skipped_robots: usize,
    pub pages_skipped_dedup: usize,
    pub pages_skipped_depth: usize,
    pub retries_attempted: usize,
}
