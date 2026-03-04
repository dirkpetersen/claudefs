//! Cache for directory listing (readdir) operations.
//!
//! Stores directory entries returned by readdir/readdirplus so that
//! subsequent directory traversals and lookups can avoid round-trips
//! to the metadata service.

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// A single directory entry.
#[derive(Debug, Clone, PartialEq)]
pub struct DirEntry {
    /// Inode number of the entry.
    pub inode: u64,
    /// Name of the entry within its parent directory.
    pub name: String,
    /// File type: 'f' for regular, 'd' for directory, 'l' for symlink, 'o' for other.
    pub file_type: char,
    /// File size in bytes (for regular files).
    pub size: u64,
    /// Unix permission mode bits.
    pub mode: u32,
}

impl DirEntry {
    /// Creates a new directory entry.
    pub fn new(inode: u64, name: impl Into<String>, file_type: char, size: u64, mode: u32) -> Self {
        DirEntry {
            inode,
            name: name.into(),
            file_type,
            size,
            mode,
        }
    }

    /// Returns true if this entry is a regular file.
    pub fn is_file(&self) -> bool {
        self.file_type == 'f'
    }

    /// Returns true if this entry is a directory.
    pub fn is_dir(&self) -> bool {
        self.file_type == 'd'
    }

    /// Returns true if this entry is a symlink.
    pub fn is_symlink(&self) -> bool {
        self.file_type == 'l'
    }
}

/// A cached page of directory entries.
#[derive(Debug, Clone)]
pub struct DirPage {
    /// The directory entries in this page.
    pub entries: Vec<DirEntry>,
    /// The readdir offset for the next page (0 means start from beginning).
    pub next_offset: i64,
    /// Whether this page covers all entries (no more pages).
    pub is_complete: bool,
    /// When this page was cached.
    pub cached_at: Instant,
}

impl DirPage {
    /// Creates a new directory page.
    pub fn new(entries: Vec<DirEntry>, next_offset: i64, is_complete: bool) -> Self {
        DirPage {
            entries,
            next_offset,
            is_complete,
            cached_at: Instant::now(),
        }
    }

    /// Returns the number of entries in this page.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns true if this page has no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns true if this page has expired past the given TTL in seconds.
    pub fn is_expired(&self, ttl_secs: u64) -> bool {
        self.cached_at.elapsed() > Duration::from_secs(ttl_secs)
    }
}

/// Configuration for the readdir cache.
#[derive(Debug, Clone)]
pub struct ReaddirCacheConfig {
    /// Maximum number of directories tracked.
    pub max_dirs: usize,
    /// TTL for cached directory pages, in seconds.
    pub ttl_secs: u64,
    /// Maximum entries stored per directory.
    pub max_entries_per_dir: usize,
}

impl Default for ReaddirCacheConfig {
    fn default() -> Self {
        ReaddirCacheConfig {
            max_dirs: 1000,
            ttl_secs: 5,
            max_entries_per_dir: 10_000,
        }
    }
}

/// Cache statistics.
#[derive(Debug, Default, Clone)]
pub struct ReaddirCacheStats {
    /// Total cache hits.
    pub hits: u64,
    /// Total cache misses.
    pub misses: u64,
    /// Total invalidations.
    pub invalidations: u64,
    /// Number of directories currently cached.
    pub cached_dirs: usize,
}

/// Cache for directory listing results.
pub struct ReaddirCache {
    pages: HashMap<u64, DirPage>,
    config: ReaddirCacheConfig,
    stats: ReaddirCacheStats,
}

impl ReaddirCache {
    /// Creates a new readdir cache with the given configuration.
    pub fn new(config: ReaddirCacheConfig) -> Self {
        ReaddirCache {
            pages: HashMap::new(),
            config,
            stats: ReaddirCacheStats::default(),
        }
    }

    /// Stores a directory page for the given parent inode.
    ///
    /// If the cache is at capacity, the oldest entries are evicted.
    /// If the page would exceed `max_entries_per_dir`, it is truncated.
    pub fn insert(&mut self, parent_ino: u64, mut page: DirPage) {
        if page.entries.len() > self.config.max_entries_per_dir {
            page.entries.truncate(self.config.max_entries_per_dir);
        }
        if self.pages.len() >= self.config.max_dirs && !self.pages.contains_key(&parent_ino) {
            // evict one expired entry, or the first one we find
            let to_remove = self
                .pages
                .iter()
                .find(|(_, p)| p.is_expired(self.config.ttl_secs))
                .map(|(k, _)| *k)
                .or_else(|| self.pages.keys().next().copied());
            if let Some(key) = to_remove {
                self.pages.remove(&key);
            }
        }
        self.pages.insert(parent_ino, page);
        self.stats.cached_dirs = self.pages.len();
    }

    /// Retrieves a cached directory page for the given parent inode.
    ///
    /// Returns `None` if no entry exists or the entry has expired.
    pub fn get(&mut self, parent_ino: u64) -> Option<&DirPage> {
        let ttl = self.config.ttl_secs;
        if let Some(page) = self.pages.get(&parent_ino) {
            if page.is_expired(ttl) {
                self.pages.remove(&parent_ino);
                self.stats.misses += 1;
                self.stats.cached_dirs = self.pages.len();
                None
            } else {
                self.stats.hits += 1;
                self.pages.get(&parent_ino)
            }
        } else {
            self.stats.misses += 1;
            None
        }
    }

    /// Invalidates the cached directory page for the given parent inode.
    pub fn invalidate(&mut self, parent_ino: u64) {
        if self.pages.remove(&parent_ino).is_some() {
            self.stats.invalidations += 1;
        }
        self.stats.cached_dirs = self.pages.len();
    }

    /// Clears all cached directory pages.
    pub fn clear(&mut self) {
        let count = self.pages.len() as u64;
        self.pages.clear();
        self.stats.invalidations += count;
        self.stats.cached_dirs = 0;
    }

    /// Returns a snapshot of cache statistics.
    pub fn stats(&self) -> ReaddirCacheStats {
        self.stats.clone()
    }

    /// Returns the number of cached directories.
    pub fn len(&self) -> usize {
        self.pages.len()
    }

    /// Returns true if no directories are cached.
    pub fn is_empty(&self) -> bool {
        self.pages.is_empty()
    }

    /// Looks up an entry by name within a cached directory.
    ///
    /// Returns the `DirEntry` if the directory is cached and contains the name.
    pub fn lookup_entry(&mut self, parent_ino: u64, name: &str) -> Option<DirEntry> {
        let ttl = self.config.ttl_secs;
        if let Some(page) = self.pages.get(&parent_ino) {
            if page.is_expired(ttl) {
                self.pages.remove(&parent_ino);
                self.stats.misses += 1;
                self.stats.cached_dirs = self.pages.len();
                None
            } else {
                let result = page.entries.iter().find(|e| e.name == name).cloned();
                if result.is_some() {
                    self.stats.hits += 1;
                } else {
                    self.stats.misses += 1;
                }
                result
            }
        } else {
            self.stats.misses += 1;
            None
        }
    }
}

impl Default for ReaddirCache {
    fn default() -> Self {
        Self::new(ReaddirCacheConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(name: &str, inode: u64) -> DirEntry {
        DirEntry::new(inode, name, 'f', 100, 0o644)
    }

    fn make_dir_entry(name: &str, inode: u64) -> DirEntry {
        DirEntry::new(inode, name, 'd', 0, 0o755)
    }

    fn make_page(entries: Vec<DirEntry>) -> DirPage {
        DirPage::new(entries, 0, true)
    }

    #[test]
    fn test_new_cache_is_empty() {
        let cache = ReaddirCache::new(ReaddirCacheConfig::default());
        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn test_insert_and_get() {
        let mut cache = ReaddirCache::new(ReaddirCacheConfig::default());
        let entries = vec![make_entry("file.txt", 2), make_dir_entry("subdir", 3)];
        cache.insert(1, make_page(entries));
        assert!(cache.get(1).is_some());
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn test_get_returns_none_for_missing() {
        let mut cache = ReaddirCache::new(ReaddirCacheConfig::default());
        assert!(cache.get(99).is_none());
    }

    #[test]
    fn test_invalidate_removes_entry() {
        let mut cache = ReaddirCache::new(ReaddirCacheConfig::default());
        cache.insert(1, make_page(vec![make_entry("a", 2)]));
        cache.invalidate(1);
        assert!(cache.is_empty());
        assert_eq!(cache.stats().invalidations, 1);
    }

    #[test]
    fn test_clear_removes_all() {
        let mut cache = ReaddirCache::new(ReaddirCacheConfig::default());
        cache.insert(1, make_page(vec![make_entry("a", 2)]));
        cache.insert(2, make_page(vec![make_entry("b", 3)]));
        cache.clear();
        assert!(cache.is_empty());
    }

    #[test]
    fn test_lookup_entry_found() {
        let mut cache = ReaddirCache::new(ReaddirCacheConfig::default());
        let entries = vec![make_entry("hello.txt", 5)];
        cache.insert(1, make_page(entries));
        let result = cache.lookup_entry(1, "hello.txt");
        assert!(result.is_some());
        assert_eq!(result.unwrap().inode, 5);
    }

    #[test]
    fn test_lookup_entry_not_found() {
        let mut cache = ReaddirCache::new(ReaddirCacheConfig::default());
        cache.insert(1, make_page(vec![make_entry("file.txt", 2)]));
        assert!(cache.lookup_entry(1, "missing.txt").is_none());
    }

    #[test]
    fn test_lookup_entry_missing_dir() {
        let mut cache = ReaddirCache::new(ReaddirCacheConfig::default());
        assert!(cache.lookup_entry(99, "anything").is_none());
    }

    #[test]
    fn test_stats_track_hits_and_misses() {
        let mut cache = ReaddirCache::new(ReaddirCacheConfig::default());
        cache.get(1); // miss
        cache.insert(1, make_page(vec![make_entry("x", 2)]));
        cache.get(1); // hit
        let stats = cache.stats();
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.misses, 1);
    }

    #[test]
    fn test_dir_entry_type_helpers() {
        let file = DirEntry::new(1, "f", 'f', 0, 0o644);
        let dir = DirEntry::new(2, "d", 'd', 0, 0o755);
        let link = DirEntry::new(3, "l", 'l', 0, 0o777);
        assert!(file.is_file());
        assert!(!file.is_dir());
        assert!(dir.is_dir());
        assert!(!dir.is_symlink());
        assert!(link.is_symlink());
    }

    #[test]
    fn test_dir_page_len() {
        let entries = vec![make_entry("a", 1), make_entry("b", 2), make_entry("c", 3)];
        let page = make_page(entries);
        assert_eq!(page.len(), 3);
        assert!(!page.is_empty());
    }

    #[test]
    fn test_empty_page_is_empty() {
        let page = make_page(vec![]);
        assert!(page.is_empty());
        assert_eq!(page.len(), 0);
    }

    #[test]
    fn test_max_entries_per_dir_truncation() {
        let config = ReaddirCacheConfig {
            max_dirs: 100,
            ttl_secs: 60,
            max_entries_per_dir: 3,
        };
        let mut cache = ReaddirCache::new(config);
        let entries = (0..10)
            .map(|i| make_entry(&format!("f{}", i), i + 1))
            .collect();
        cache.insert(1, make_page(entries));
        let page = cache.get(1).unwrap();
        assert!(page.len() <= 3);
    }

    #[test]
    fn test_cached_dirs_stat_updates() {
        let mut cache = ReaddirCache::new(ReaddirCacheConfig::default());
        cache.insert(1, make_page(vec![make_entry("a", 2)]));
        assert_eq!(cache.stats().cached_dirs, 1);
        cache.insert(2, make_page(vec![make_entry("b", 3)]));
        assert_eq!(cache.stats().cached_dirs, 2);
        cache.invalidate(1);
        assert_eq!(cache.stats().cached_dirs, 1);
    }

    #[test]
    fn test_page_next_offset() {
        let page = DirPage::new(vec![make_entry("a", 1)], 42, false);
        assert_eq!(page.next_offset, 42);
        assert!(!page.is_complete);
    }

    #[test]
    fn test_page_is_complete() {
        let page = DirPage::new(vec![], 0, true);
        assert!(page.is_complete);
    }

    #[test]
    fn test_ttl_zero_expires_immediately() {
        let config = ReaddirCacheConfig {
            max_dirs: 100,
            ttl_secs: 0,
            max_entries_per_dir: 1000,
        };
        let mut cache = ReaddirCache::new(config);
        cache.insert(1, make_page(vec![make_entry("x", 2)]));
        std::thread::sleep(std::time::Duration::from_millis(10));
        assert!(cache.get(1).is_none());
    }

    #[test]
    fn test_invalidate_nonexistent_no_op() {
        let mut cache = ReaddirCache::new(ReaddirCacheConfig::default());
        cache.invalidate(999); // should not panic
        assert_eq!(cache.stats().invalidations, 0);
    }

    #[test]
    fn test_multiple_inserts_same_key_replaces() {
        let mut cache = ReaddirCache::new(ReaddirCacheConfig::default());
        cache.insert(1, make_page(vec![make_entry("a", 2)]));
        cache.insert(1, make_page(vec![make_entry("b", 3), make_entry("c", 4)]));
        let page = cache.get(1).unwrap();
        assert_eq!(page.len(), 2);
    }

    #[test]
    fn test_default_config_values() {
        let config = ReaddirCacheConfig::default();
        assert_eq!(config.max_dirs, 1000);
        assert_eq!(config.ttl_secs, 5);
        assert_eq!(config.max_entries_per_dir, 10_000);
    }
}
