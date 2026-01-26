//! Content deduplication module
//!
//! Detects and skips duplicate page content to avoid re-indexing the same information.

use sha2::{Sha256, Digest};
use std::collections::HashSet;
use tokio::sync::RwLock;

/// Content deduplication using SHA256 hashes
pub struct ContentDedup {
    seen_hashes: RwLock<HashSet<[u8; 32]>>,
}

impl ContentDedup {
    pub fn new() -> Self {
        Self {
            seen_hashes: RwLock::new(HashSet::new()),
        }
    }

    /// Normalize content for hashing (lowercase, collapse whitespace)
    fn normalize(content: &str) -> String {
        content
            .to_lowercase()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Compute SHA256 hash of normalized content
    fn hash(content: &str) -> [u8; 32] {
        let normalized = Self::normalize(content);
        let mut hasher = Sha256::new();
        hasher.update(normalized.as_bytes());
        hasher.finalize().into()
    }

    /// Check if content is a duplicate. Returns true if we've seen this content before.
    /// If not a duplicate, adds the hash to the set.
    pub async fn is_duplicate(&self, content: &str) -> bool {
        let hash = Self::hash(content);

        // First check with read lock
        {
            let seen = self.seen_hashes.read().await;
            if seen.contains(&hash) {
                return true;
            }
        }

        // Not seen, add with write lock
        {
            let mut seen = self.seen_hashes.write().await;
            // Double-check after acquiring write lock
            if seen.contains(&hash) {
                return true;
            }
            seen.insert(hash);
        }

        false
    }

    /// Get the number of unique pages seen
    pub async fn unique_count(&self) -> usize {
        self.seen_hashes.read().await.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_duplicate_detection() {
        let dedup = ContentDedup::new();

        // First time seeing content - not a duplicate
        assert!(!dedup.is_duplicate("Hello World").await);

        // Same content again - is a duplicate
        assert!(dedup.is_duplicate("Hello World").await);

        // Same content with different whitespace - still duplicate
        assert!(dedup.is_duplicate("  Hello   World  ").await);

        // Same content with different case - still duplicate
        assert!(dedup.is_duplicate("HELLO WORLD").await);

        // Different content - not a duplicate
        assert!(!dedup.is_duplicate("Goodbye World").await);
    }

    #[tokio::test]
    async fn test_unique_count() {
        let dedup = ContentDedup::new();

        dedup.is_duplicate("Page 1").await;
        dedup.is_duplicate("Page 2").await;
        dedup.is_duplicate("Page 1").await; // duplicate

        assert_eq!(dedup.unique_count().await, 2);
    }
}
