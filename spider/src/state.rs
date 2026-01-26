//! State persistence module for crash recovery
//!
//! Persists the crawl queue to SQLite so the crawler can resume after restarts.

use rusqlite::{Connection, params};
use std::path::Path;
use std::sync::Mutex;
use anyhow::Result;

/// Represents a URL in the pending queue
#[derive(Debug, Clone)]
pub struct PendingUrl {
    pub url: String,
    pub retry_count: usize,
    pub depth: usize,
    pub domain: String,
}

/// State persistence using SQLite
pub struct CrawlState {
    conn: Mutex<Connection>,
}

impl CrawlState {
    /// Create or open a state database at the given path
    pub fn new<P: AsRef<Path>>(db_path: P) -> Result<Self> {
        let conn = Connection::open(db_path)?;

        // Create tables if they don't exist
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS pending_urls (
                id INTEGER PRIMARY KEY,
                url TEXT NOT NULL UNIQUE,
                retry_count INTEGER DEFAULT 0,
                depth INTEGER DEFAULT 0,
                domain TEXT NOT NULL,
                created_at INTEGER DEFAULT (strftime('%s', 'now'))
            );

            CREATE TABLE IF NOT EXISTS visited_urls (
                url TEXT PRIMARY KEY,
                visited_at INTEGER DEFAULT (strftime('%s', 'now'))
            );

            CREATE INDEX IF NOT EXISTS idx_pending_domain ON pending_urls(domain);
            CREATE INDEX IF NOT EXISTS idx_pending_depth ON pending_urls(depth);
            "
        )?;

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Create an in-memory database (for testing or ephemeral use)
    pub fn in_memory() -> Result<Self> {
        Self::new(":memory:")
    }

    /// Add a URL to the pending queue
    pub fn add_pending(&self, url: &str, retry_count: usize, depth: usize, domain: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO pending_urls (url, retry_count, depth, domain) VALUES (?1, ?2, ?3, ?4)",
            params![url, retry_count as i64, depth as i64, domain],
        )?;
        Ok(())
    }

    /// Add multiple URLs to pending queue in a transaction
    pub fn add_pending_batch(&self, urls: &[(String, usize, usize, String)]) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;

        for (url, retry_count, depth, domain) in urls {
            tx.execute(
                "INSERT OR REPLACE INTO pending_urls (url, retry_count, depth, domain) VALUES (?1, ?2, ?3, ?4)",
                params![url, *retry_count as i64, *depth as i64, domain],
            )?;
        }

        tx.commit()?;
        Ok(())
    }

    /// Remove a URL from pending (after processing)
    pub fn remove_pending(&self, url: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM pending_urls WHERE url = ?1", params![url])?;
        Ok(())
    }

    /// Mark a URL as visited
    pub fn mark_visited(&self, url: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO visited_urls (url) VALUES (?1)",
            params![url],
        )?;
        Ok(())
    }

    /// Check if a URL has been visited
    pub fn is_visited(&self, url: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM visited_urls WHERE url = ?1",
            params![url],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    /// Load all pending URLs
    pub fn load_pending(&self) -> Result<Vec<PendingUrl>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT url, retry_count, depth, domain FROM pending_urls ORDER BY depth ASC, id ASC"
        )?;

        let urls = stmt
            .query_map([], |row| {
                Ok(PendingUrl {
                    url: row.get(0)?,
                    retry_count: row.get::<_, i64>(1)? as usize,
                    depth: row.get::<_, i64>(2)? as usize,
                    domain: row.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(urls)
    }

    /// Get count of pending URLs
    pub fn pending_count(&self) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM pending_urls",
            [],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }

    /// Get count of visited URLs
    pub fn visited_count(&self) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM visited_urls",
            [],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }

    /// Clear all pending URLs (after successful completion)
    pub fn clear_pending(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM pending_urls", [])?;
        Ok(())
    }

    /// Clear visited URLs (for fresh start)
    pub fn clear_visited(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM visited_urls", [])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pending_urls() {
        let state = CrawlState::in_memory().unwrap();

        state.add_pending("https://example.com/", 0, 0, "example.com").unwrap();
        state.add_pending("https://example.com/page", 1, 1, "example.com").unwrap();

        let pending = state.load_pending().unwrap();
        assert_eq!(pending.len(), 2);
        assert_eq!(pending[0].url, "https://example.com/");
        assert_eq!(pending[1].depth, 1);

        state.remove_pending("https://example.com/").unwrap();
        assert_eq!(state.pending_count().unwrap(), 1);
    }

    #[test]
    fn test_visited_urls() {
        let state = CrawlState::in_memory().unwrap();

        assert!(!state.is_visited("https://example.com/").unwrap());

        state.mark_visited("https://example.com/").unwrap();
        assert!(state.is_visited("https://example.com/").unwrap());

        // Idempotent
        state.mark_visited("https://example.com/").unwrap();
        assert_eq!(state.visited_count().unwrap(), 1);
    }
}
