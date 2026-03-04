//! Writeback data cache for the FUSE client.
//!
//! Accumulates dirty pages from client writes and manages their flush
//! to the storage backend. Supports per-inode dirty tracking, size-based
//! and time-based flush triggers, and explicit writeback control.

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// A single dirty page awaiting writeback.
#[derive(Debug, Clone)]
pub struct DirtyPage {
    /// The inode this page belongs to.
    pub inode: u64,
    /// Byte offset within the file.
    pub offset: u64,
    /// The dirty data bytes.
    pub data: Vec<u8>,
    /// When this page was first dirtied.
    pub dirtied_at: Instant,
    /// How many times this page has been written to (write amplification metric).
    pub write_count: u32,
}

impl DirtyPage {
    /// Creates a new dirty page.
    pub fn new(inode: u64, offset: u64, data: Vec<u8>) -> Self {
        DirtyPage {
            inode,
            offset,
            data,
            dirtied_at: Instant::now(),
            write_count: 1,
        }
    }

    /// Returns the size of this dirty page's data in bytes.
    pub fn size(&self) -> usize {
        self.data.len()
    }

    /// Returns the age of this dirty page in seconds.
    pub fn age_secs(&self) -> u64 {
        self.dirtied_at.elapsed().as_secs()
    }

    /// Returns true if this page is older than `max_age_secs`.
    pub fn is_stale(&self, max_age_secs: u64) -> bool {
        self.dirtied_at.elapsed() > Duration::from_secs(max_age_secs)
    }

    /// Merges new data into this dirty page (updates write_count).
    pub fn merge(&mut self, data: Vec<u8>) {
        self.data = data;
        self.write_count += 1;
    }
}

/// Configuration for the writeback cache.
#[derive(Debug, Clone)]
pub struct WritebackConfig {
    /// Total dirty data limit in bytes before forced flush.
    pub max_dirty_bytes: u64,
    /// Maximum age of a dirty page before it must be flushed, in seconds.
    pub max_dirty_age_secs: u64,
    /// Maximum dirty bytes per inode.
    pub max_dirty_per_inode: u64,
    /// Maximum number of pages to flush in one writeback batch.
    pub max_writeback_pages: usize,
}

impl Default for WritebackConfig {
    fn default() -> Self {
        WritebackConfig {
            max_dirty_bytes: 64 * 1024 * 1024, // 64 MB
            max_dirty_age_secs: 30,
            max_dirty_per_inode: 16 * 1024 * 1024, // 16 MB
            max_writeback_pages: 256,
        }
    }
}

/// Statistics for the writeback cache.
#[derive(Debug, Default, Clone)]
pub struct WritebackStats {
    /// Total dirty bytes currently in the cache.
    pub dirty_bytes: u64,
    /// Total pages currently dirty.
    pub dirty_pages: usize,
    /// Number of inodes with dirty data.
    pub dirty_inodes: usize,
    /// Total pages flushed since last reset.
    pub pages_flushed: u64,
    /// Total bytes flushed since last reset.
    pub bytes_flushed: u64,
    /// Number of forced flushes (triggered by limit).
    pub forced_flushes: u64,
}

/// The writeback data cache.
pub struct WritebackCache {
    pages: HashMap<(u64, u64), DirtyPage>, // (inode, offset) -> page
    per_inode_bytes: HashMap<u64, u64>,
    config: WritebackConfig,
    stats: WritebackStats,
}

impl WritebackCache {
    /// Creates a new writeback cache with the given configuration.
    pub fn new(config: WritebackConfig) -> Self {
        WritebackCache {
            pages: HashMap::new(),
            per_inode_bytes: HashMap::new(),
            config,
            stats: WritebackStats::default(),
        }
    }

    /// Adds or updates a dirty page for the given inode at the given offset.
    ///
    /// If a page at the same (inode, offset) already exists, its data is merged.
    /// Returns `true` if this caused the global dirty limit to be reached.
    pub fn write(&mut self, inode: u64, offset: u64, data: Vec<u8>) -> bool {
        let size = data.len() as u64;
        let key = (inode, offset);
        if let Some(existing) = self.pages.get_mut(&key) {
            let old_size = existing.size() as u64;
            existing.merge(data);
            let new_size = existing.size() as u64;
            let delta = new_size as i64 - old_size as i64;
            if delta > 0 {
                self.stats.dirty_bytes += delta as u64;
                *self.per_inode_bytes.entry(inode).or_insert(0) += delta as u64;
            } else if delta < 0 {
                let reduction = (-delta) as u64;
                self.stats.dirty_bytes = self.stats.dirty_bytes.saturating_sub(reduction);
                let inode_bytes = self.per_inode_bytes.entry(inode).or_insert(0);
                *inode_bytes = inode_bytes.saturating_sub(reduction);
            }
        } else {
            self.pages.insert(key, DirtyPage::new(inode, offset, data));
            self.stats.dirty_bytes += size;
            self.stats.dirty_pages += 1;
            *self.per_inode_bytes.entry(inode).or_insert(0) += size;
        }
        self.stats.dirty_inodes = self.per_inode_bytes.len();
        self.is_over_limit()
    }

    /// Returns true if the global dirty limit is exceeded.
    pub fn is_over_limit(&self) -> bool {
        self.stats.dirty_bytes >= self.config.max_dirty_bytes
    }

    /// Returns true if the per-inode dirty limit is exceeded.
    pub fn is_inode_over_limit(&self, inode: u64) -> bool {
        self.per_inode_bytes.get(&inode).copied().unwrap_or(0) >= self.config.max_dirty_per_inode
    }

    /// Returns a list of pages that should be flushed now.
    ///
    /// Pages are selected if they are stale (exceed max_dirty_age_secs)
    /// or if the cache is over the global limit. Up to `max_writeback_pages`
    /// are returned.
    pub fn flush_candidates(&self) -> Vec<(u64, u64)> {
        let max_age = self.config.max_dirty_age_secs;
        let over_limit = self.is_over_limit();
        let max_pages = self.config.max_writeback_pages;
        self.pages
            .iter()
            .filter(|(_, p)| p.is_stale(max_age) || over_limit)
            .take(max_pages)
            .map(|((ino, off), _)| (*ino, *off))
            .collect()
    }

    /// Marks a page as flushed and removes it from the dirty cache.
    ///
    /// Updates stats accordingly.
    pub fn mark_flushed(&mut self, inode: u64, offset: u64) -> bool {
        let key = (inode, offset);
        if let Some(page) = self.pages.remove(&key) {
            let size = page.size() as u64;
            self.stats.dirty_bytes = self.stats.dirty_bytes.saturating_sub(size);
            self.stats.dirty_pages = self.stats.dirty_pages.saturating_sub(1);
            self.stats.pages_flushed += 1;
            self.stats.bytes_flushed += size;
            let inode_bytes = self.per_inode_bytes.entry(inode).or_insert(0);
            *inode_bytes = inode_bytes.saturating_sub(size);
            if *inode_bytes == 0 {
                self.per_inode_bytes.remove(&inode);
            }
            self.stats.dirty_inodes = self.per_inode_bytes.len();
            true
        } else {
            false
        }
    }

    /// Discards all dirty pages for the given inode (e.g., on file close/eviction).
    pub fn discard_inode(&mut self, inode: u64) {
        let keys: Vec<_> = self
            .pages
            .keys()
            .filter(|(ino, _)| *ino == inode)
            .cloned()
            .collect();
        for key in keys {
            if let Some(page) = self.pages.remove(&key) {
                self.stats.dirty_bytes = self.stats.dirty_bytes.saturating_sub(page.size() as u64);
                self.stats.dirty_pages = self.stats.dirty_pages.saturating_sub(1);
            }
        }
        self.per_inode_bytes.remove(&inode);
        self.stats.dirty_inodes = self.per_inode_bytes.len();
    }

    /// Returns the total dirty bytes across all inodes.
    pub fn dirty_bytes(&self) -> u64 {
        self.stats.dirty_bytes
    }

    /// Returns the dirty bytes for a specific inode.
    pub fn inode_dirty_bytes(&self, inode: u64) -> u64 {
        self.per_inode_bytes.get(&inode).copied().unwrap_or(0)
    }

    /// Returns the total number of dirty pages.
    pub fn dirty_pages(&self) -> usize {
        self.stats.dirty_pages
    }

    /// Returns the number of inodes with dirty data.
    pub fn dirty_inodes(&self) -> usize {
        self.stats.dirty_inodes
    }

    /// Returns a snapshot of writeback statistics.
    pub fn stats(&self) -> WritebackStats {
        self.stats.clone()
    }

    /// Returns a reference to a dirty page if it exists.
    pub fn get_page(&self, inode: u64, offset: u64) -> Option<&DirtyPage> {
        self.pages.get(&(inode, offset))
    }
}

impl Default for WritebackCache {
    fn default() -> Self {
        Self::new(WritebackConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_cache_is_clean() {
        let cache = WritebackCache::new(WritebackConfig::default());
        assert_eq!(cache.dirty_bytes(), 0);
        assert_eq!(cache.dirty_pages(), 0);
        assert_eq!(cache.dirty_inodes(), 0);
    }

    #[test]
    fn test_write_adds_dirty_page() {
        let mut cache = WritebackCache::new(WritebackConfig::default());
        cache.write(1, 0, vec![0u8; 4096]);
        assert_eq!(cache.dirty_bytes(), 4096);
        assert_eq!(cache.dirty_pages(), 1);
        assert_eq!(cache.dirty_inodes(), 1);
    }

    #[test]
    fn test_write_same_offset_merges() {
        let mut cache = WritebackCache::new(WritebackConfig::default());
        cache.write(1, 0, vec![1u8; 4096]);
        cache.write(1, 0, vec![2u8; 4096]);
        assert_eq!(cache.dirty_pages(), 1);
        assert_eq!(cache.dirty_bytes(), 4096);
        let page = cache.get_page(1, 0).unwrap();
        assert_eq!(page.write_count, 2);
        assert_eq!(page.data[0], 2);
    }

    #[test]
    fn test_mark_flushed_removes_page() {
        let mut cache = WritebackCache::new(WritebackConfig::default());
        cache.write(1, 0, vec![0u8; 1024]);
        cache.mark_flushed(1, 0);
        assert_eq!(cache.dirty_bytes(), 0);
        assert_eq!(cache.dirty_pages(), 0);
        assert_eq!(cache.dirty_inodes(), 0);
    }

    #[test]
    fn test_mark_flushed_nonexistent_returns_false() {
        let mut cache = WritebackCache::new(WritebackConfig::default());
        assert!(!cache.mark_flushed(99, 0));
    }

    #[test]
    fn test_discard_inode_removes_all_pages() {
        let mut cache = WritebackCache::new(WritebackConfig::default());
        cache.write(1, 0, vec![0u8; 1024]);
        cache.write(1, 4096, vec![0u8; 1024]);
        cache.write(2, 0, vec![0u8; 512]);
        cache.discard_inode(1);
        assert_eq!(cache.dirty_inodes(), 1);
        assert_eq!(cache.inode_dirty_bytes(1), 0);
        assert_eq!(cache.inode_dirty_bytes(2), 512);
    }

    #[test]
    fn test_is_over_limit_when_exceeded() {
        let config = WritebackConfig {
            max_dirty_bytes: 1024,
            ..Default::default()
        };
        let mut cache = WritebackCache::new(config);
        cache.write(1, 0, vec![0u8; 1024]);
        assert!(cache.is_over_limit());
    }

    #[test]
    fn test_is_not_over_limit_when_below() {
        let config = WritebackConfig {
            max_dirty_bytes: 1024 * 1024,
            ..Default::default()
        };
        let mut cache = WritebackCache::new(config);
        cache.write(1, 0, vec![0u8; 512]);
        assert!(!cache.is_over_limit());
    }

    #[test]
    fn test_flush_candidates_stale_pages() {
        let config = WritebackConfig {
            max_dirty_age_secs: 0, // all pages are immediately stale
            ..Default::default()
        };
        let mut cache = WritebackCache::new(config);
        cache.write(1, 0, vec![0u8; 100]);
        cache.write(2, 0, vec![0u8; 200]);
        std::thread::sleep(Duration::from_millis(10));
        let candidates = cache.flush_candidates();
        assert!(!candidates.is_empty());
    }

    #[test]
    fn test_flush_candidates_over_limit() {
        let config = WritebackConfig {
            max_dirty_bytes: 100,
            max_dirty_age_secs: 3600, // won't expire
            max_writeback_pages: 256,
            ..Default::default()
        };
        let mut cache = WritebackCache::new(config);
        cache.write(1, 0, vec![0u8; 200]); // over limit
        let candidates = cache.flush_candidates();
        assert!(!candidates.is_empty());
    }

    #[test]
    fn test_flush_candidates_respects_max_pages() {
        let config = WritebackConfig {
            max_dirty_bytes: 0, // always over limit
            max_dirty_age_secs: 3600,
            max_writeback_pages: 3,
            max_dirty_per_inode: 1024 * 1024 * 1024,
        };
        let mut cache = WritebackCache::new(config);
        for i in 0..10 {
            cache.write(1, i * 4096, vec![0u8; 4096]);
        }
        let candidates = cache.flush_candidates();
        assert!(candidates.len() <= 3);
    }

    #[test]
    fn test_stats_pages_flushed() {
        let mut cache = WritebackCache::new(WritebackConfig::default());
        cache.write(1, 0, vec![0u8; 512]);
        cache.mark_flushed(1, 0);
        assert_eq!(cache.stats().pages_flushed, 1);
        assert_eq!(cache.stats().bytes_flushed, 512);
    }

    #[test]
    fn test_inode_over_limit() {
        let config = WritebackConfig {
            max_dirty_per_inode: 1024,
            ..Default::default()
        };
        let mut cache = WritebackCache::new(config);
        cache.write(1, 0, vec![0u8; 1024]);
        assert!(cache.is_inode_over_limit(1));
        assert!(!cache.is_inode_over_limit(2));
    }

    #[test]
    fn test_multiple_inodes_independent() {
        let mut cache = WritebackCache::new(WritebackConfig::default());
        cache.write(1, 0, vec![0u8; 100]);
        cache.write(2, 0, vec![0u8; 200]);
        cache.write(3, 0, vec![0u8; 300]);
        assert_eq!(cache.dirty_inodes(), 3);
        assert_eq!(cache.inode_dirty_bytes(1), 100);
        assert_eq!(cache.inode_dirty_bytes(2), 200);
        assert_eq!(cache.inode_dirty_bytes(3), 300);
        assert_eq!(cache.dirty_bytes(), 600);
    }

    #[test]
    fn test_get_page_returns_data() {
        let mut cache = WritebackCache::new(WritebackConfig::default());
        cache.write(5, 512, vec![42u8; 256]);
        let page = cache.get_page(5, 512).unwrap();
        assert_eq!(page.inode, 5);
        assert_eq!(page.offset, 512);
        assert_eq!(page.size(), 256);
        assert_eq!(page.data[0], 42);
    }

    #[test]
    fn test_get_page_missing_returns_none() {
        let cache = WritebackCache::new(WritebackConfig::default());
        assert!(cache.get_page(1, 0).is_none());
    }

    #[test]
    fn test_dirty_page_age_and_stale() {
        let page = DirtyPage::new(1, 0, vec![0u8; 64]);
        assert!(!page.is_stale(3600)); // fresh page, not stale
        assert!(page.is_stale(0)); // with 0 max_age, immediately stale after sleep
    }

    #[test]
    fn test_dirty_page_size() {
        let page = DirtyPage::new(1, 0, vec![0u8; 4096]);
        assert_eq!(page.size(), 4096);
    }

    #[test]
    fn test_default_config_values() {
        let config = WritebackConfig::default();
        assert_eq!(config.max_dirty_bytes, 64 * 1024 * 1024);
        assert_eq!(config.max_dirty_age_secs, 30);
        assert_eq!(config.max_writeback_pages, 256);
    }

    #[test]
    fn test_write_returns_true_when_over_limit() {
        let config = WritebackConfig {
            max_dirty_bytes: 100,
            ..Default::default()
        };
        let mut cache = WritebackCache::new(config);
        let over_limit = cache.write(1, 0, vec![0u8; 200]);
        assert!(over_limit);
    }

    #[test]
    fn test_write_returns_false_when_under_limit() {
        let config = WritebackConfig {
            max_dirty_bytes: 1024 * 1024,
            ..Default::default()
        };
        let mut cache = WritebackCache::new(config);
        let over_limit = cache.write(1, 0, vec![0u8; 100]);
        assert!(!over_limit);
    }
}
