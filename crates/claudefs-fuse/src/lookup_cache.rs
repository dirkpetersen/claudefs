//! Fast (parent_inode, name) -> (child_inode, FileType) lookup cache.
//!
//! This module provides a cache layer for individual file/directory lookups,
//! complementing `dir_cache.rs` (full directory listing cache). Caching individual
//! lookup results reduces metadata RPCs for common operations like cd, find, stat.

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// File type for lookup entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryType {
    RegularFile,
    Directory,
    Symlink,
    Other,
}

impl EntryType {
    /// Returns true if this is a regular file.
    pub fn is_regular(&self) -> bool {
        matches!(self, EntryType::RegularFile)
    }

    /// Returns true if this is a directory.
    pub fn is_directory(&self) -> bool {
        matches!(self, EntryType::Directory)
    }
}

/// A cached lookup entry.
#[derive(Debug, Clone)]
pub struct LookupEntry {
    pub child_ino: u64,
    pub entry_type: EntryType,
    pub generation: u64,
    pub cached_at: Instant,
}

impl LookupEntry {
    /// Returns true if this entry has expired based on the given TTL.
    pub fn is_expired(&self, ttl_secs: u64) -> bool {
        self.cached_at.elapsed() > Duration::from_secs(ttl_secs)
    }
}

/// Lookup cache configuration.
#[derive(Debug, Clone)]
pub struct LookupCacheConfig {
    pub capacity: usize,
    pub ttl_secs: u64,
    pub negative_ttl_secs: u64,
}

impl Default for LookupCacheConfig {
    fn default() -> Self {
        Self {
            capacity: 65536,
            ttl_secs: 5,
            negative_ttl_secs: 3,
        }
    }
}

/// Cache statistics.
#[derive(Debug, Default, Clone)]
pub struct LookupCacheStats {
    pub hits: u64,
    pub misses: u64,
    pub negative_hits: u64,
    pub evictions: u64,
    pub invalidations: u64,
    pub entries: usize,
}

/// The lookup cache for (parent_inode, name) -> LookupEntry.
pub struct LookupCache {
    entries: HashMap<(u64, String), LookupEntry>,
    negative_entries: HashMap<(u64, String), Instant>,
    config: LookupCacheConfig,
    stats: LookupCacheStats,
}

impl LookupCache {
    /// Creates a new lookup cache with the given configuration.
    pub fn new(config: LookupCacheConfig) -> Self {
        Self {
            entries: HashMap::new(),
            negative_entries: HashMap::new(),
            config,
            stats: LookupCacheStats::default(),
        }
    }

    /// Inserts a positive lookup result into the cache.
    pub fn insert(&mut self, parent_ino: u64, name: &str, entry: LookupEntry) {
        if self.entries.len() >= self.config.capacity {
            self.evict_one();
        }
        let key = (parent_ino, name.to_string());
        self.entries.insert(key.clone(), entry);
        self.negative_entries.remove(&key);
    }

    /// Inserts a negative entry (name does not exist) into the cache.
    pub fn insert_negative(&mut self, parent_ino: u64, name: &str) {
        if self.negative_entries.len() >= self.config.capacity {
            self.evict_one_negative();
        }
        let key = (parent_ino, name.to_string());
        self.negative_entries.insert(key.clone(), Instant::now());
        self.entries.remove(&key);
    }

    /// Looks up (parent_ino, name). Returns Some(entry) on hit, None on miss.
    /// Expired entries are evicted and treated as misses.
    pub fn get(&mut self, parent_ino: u64, name: &str) -> Option<LookupEntry> {
        let key = (parent_ino, name.to_string());

        if let Some(entry) = self.entries.get(&key).cloned() {
            if entry.is_expired(self.config.ttl_secs) {
                self.entries.remove(&key);
                self.stats.evictions += 1;
                self.stats.misses += 1;
                return None;
            }
            self.stats.hits += 1;
            return Some(entry);
        }

        if let Some(cached_at) = self.negative_entries.get(&key) {
            if cached_at.elapsed() > Duration::from_secs(self.config.negative_ttl_secs) {
                self.negative_entries.remove(&key);
                self.stats.evictions += 1;
                self.stats.misses += 1;
                return None;
            }
            self.stats.negative_hits += 1;
            return None;
        }

        self.stats.misses += 1;
        None
    }

    /// Returns true if there is a valid negative entry for this (parent, name).
    pub fn is_negative(&mut self, parent_ino: u64, name: &str) -> bool {
        let key = (parent_ino, name.to_string());

        if let Some(cached_at) = self.negative_entries.get(&key) {
            if cached_at.elapsed() > Duration::from_secs(self.config.negative_ttl_secs) {
                self.negative_entries.remove(&key);
                self.stats.evictions += 1;
                return false;
            }
            self.stats.negative_hits += 1;
            return true;
        }
        false
    }

    /// Invalidates all entries under a given parent inode.
    pub fn invalidate_parent(&mut self, parent_ino: u64) {
        let keys_to_remove: Vec<_> = self
            .entries
            .keys()
            .filter(|(ino, _)| *ino == parent_ino)
            .cloned()
            .collect();
        for key in keys_to_remove {
            self.entries.remove(&key);
            self.stats.invalidations += 1;
        }

        let neg_keys_to_remove: Vec<_> = self
            .negative_entries
            .keys()
            .filter(|(ino, _)| *ino == parent_ino)
            .cloned()
            .collect();
        for key in neg_keys_to_remove {
            self.negative_entries.remove(&key);
            self.stats.invalidations += 1;
        }
    }

    /// Invalidates a specific (parent, name) entry.
    pub fn invalidate(&mut self, parent_ino: u64, name: &str) {
        let key = (parent_ino, name.to_string());
        if self.entries.remove(&key).is_some() {
            self.stats.invalidations += 1;
        }
        if self.negative_entries.remove(&key).is_some() {
            self.stats.invalidations += 1;
        }
    }

    /// Invalidates all entries referencing a given child inode.
    pub fn invalidate_inode(&mut self, child_ino: u64) {
        let keys_to_remove: Vec<_> = self
            .entries
            .iter()
            .filter(|(_, entry)| entry.child_ino == child_ino)
            .map(|(k, _)| k.clone())
            .collect();
        for key in keys_to_remove {
            self.entries.remove(&key);
            self.stats.invalidations += 1;
        }
    }

    /// Returns a snapshot of statistics.
    pub fn stats(&self) -> LookupCacheStats {
        LookupCacheStats {
            hits: self.stats.hits,
            misses: self.stats.misses,
            negative_hits: self.stats.negative_hits,
            evictions: self.stats.evictions,
            invalidations: self.stats.invalidations,
            entries: self.entries.len(),
        }
    }

    /// Evicts all expired entries (background GC task).
    pub fn evict_expired(&mut self) -> usize {
        let mut evicted = 0;

        let expired_keys: Vec<_> = self
            .entries
            .iter()
            .filter(|(_, entry)| entry.is_expired(self.config.ttl_secs))
            .map(|(k, _)| k.clone())
            .collect();
        for key in expired_keys {
            self.entries.remove(&key);
            evicted += 1;
        }

        let expired_neg_keys: Vec<_> = self
            .negative_entries
            .iter()
            .filter(|(_, cached_at)| {
                cached_at.elapsed() > Duration::from_secs(self.config.negative_ttl_secs)
            })
            .map(|(k, _)| k.clone())
            .collect();
        for key in expired_neg_keys {
            self.negative_entries.remove(&key);
            evicted += 1;
        }

        self.stats.evictions += evicted as u64;
        evicted
    }

    fn evict_one(&mut self) {
        if let Some(key) = self.entries.keys().next().cloned() {
            self.entries.remove(&key);
            self.stats.evictions += 1;
        }
    }

    fn evict_one_negative(&mut self) {
        if let Some(key) = self.negative_entries.keys().next().cloned() {
            self.negative_entries.remove(&key);
            self.stats.evictions += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(child_ino: u64) -> LookupEntry {
        LookupEntry {
            child_ino,
            entry_type: EntryType::RegularFile,
            generation: 1,
            cached_at: Instant::now(),
        }
    }

    #[test]
    fn new_cache_is_empty() {
        let cache = LookupCache::new(LookupCacheConfig::default());
        let stats = cache.stats();
        assert_eq!(stats.entries, 0);
    }

    #[test]
    fn insert_and_get_returns_entry() {
        let mut cache = LookupCache::new(LookupCacheConfig::default());
        cache.insert(1, "foo.txt", make_entry(42));
        let result = cache.get(1, "foo.txt");
        assert!(result.is_some());
        assert_eq!(result.unwrap().child_ino, 42);
    }

    #[test]
    fn get_missing_returns_none() {
        let mut cache = LookupCache::new(LookupCacheConfig::default());
        let result = cache.get(1, "nonexistent");
        assert!(result.is_none());
    }

    #[test]
    fn ttl_expiry() {
        let config = LookupCacheConfig {
            capacity: 10,
            ttl_secs: 0,
            negative_ttl_secs: 3,
        };
        let mut cache = LookupCache::new(config);
        cache.insert(1, "foo.txt", make_entry(42));
        std::thread::sleep(Duration::from_millis(10));
        let result = cache.get(1, "foo.txt");
        assert!(result.is_none());
    }

    #[test]
    fn negative_insert_is_negative_true() {
        let mut cache = LookupCache::new(LookupCacheConfig::default());
        cache.insert_negative(1, "missing.txt");
        assert!(cache.is_negative(1, "missing.txt"));
    }

    #[test]
    fn negative_expiry() {
        let config = LookupCacheConfig {
            capacity: 10,
            ttl_secs: 5,
            negative_ttl_secs: 0,
        };
        let mut cache = LookupCache::new(config);
        cache.insert_negative(1, "missing.txt");
        std::thread::sleep(Duration::from_millis(10));
        assert!(!cache.is_negative(1, "missing.txt"));
    }

    #[test]
    fn is_negative_false_when_no_entry() {
        let mut cache = LookupCache::new(LookupCacheConfig::default());
        assert!(!cache.is_negative(1, "foo"));
    }

    #[test]
    fn invalidate_parent_removes_all() {
        let mut cache = LookupCache::new(LookupCacheConfig::default());
        cache.insert(1, "file1", make_entry(10));
        cache.insert(1, "file2", make_entry(11));
        cache.insert(2, "file3", make_entry(12));
        cache.invalidate_parent(1);
        assert!(cache.get(1, "file1").is_none());
        assert!(cache.get(1, "file2").is_none());
        assert!(cache.get(2, "file3").is_some());
    }

    #[test]
    fn invalidate_removes_specific() {
        let mut cache = LookupCache::new(LookupCacheConfig::default());
        cache.insert(1, "foo", make_entry(10));
        cache.insert(1, "bar", make_entry(11));
        cache.invalidate(1, "foo");
        assert!(cache.get(1, "foo").is_none());
        assert!(cache.get(1, "bar").is_some());
    }

    #[test]
    fn invalidate_inode_removes_all_references() {
        let mut cache = LookupCache::new(LookupCacheConfig::default());
        cache.insert(1, "a", make_entry(100));
        cache.insert(2, "b", make_entry(100));
        cache.insert(3, "c", make_entry(200));
        cache.invalidate_inode(100);
        assert!(cache.get(1, "a").is_none());
        assert!(cache.get(2, "b").is_none());
        assert!(cache.get(3, "c").is_some());
    }

    #[test]
    fn stats_hits_increment_on_hit() {
        let mut cache = LookupCache::new(LookupCacheConfig::default());
        cache.insert(1, "foo", make_entry(10));
        cache.get(1, "foo");
        let stats = cache.stats();
        assert_eq!(stats.hits, 1);
    }

    #[test]
    fn stats_misses_increment_on_miss() {
        let mut cache = LookupCache::new(LookupCacheConfig::default());
        cache.get(1, "foo");
        let stats = cache.stats();
        assert_eq!(stats.misses, 1);
    }

    #[test]
    fn stats_negative_hits_increment() {
        let mut cache = LookupCache::new(LookupCacheConfig::default());
        cache.insert_negative(1, "foo");
        cache.is_negative(1, "foo");
        let stats = cache.stats();
        assert_eq!(stats.negative_hits, 1);
    }

    #[test]
    fn stats_invalidations_increment() {
        let mut cache = LookupCache::new(LookupCacheConfig::default());
        cache.insert(1, "foo", make_entry(10));
        cache.invalidate(1, "foo");
        let stats = cache.stats();
        assert_eq!(stats.invalidations, 1);
    }

    #[test]
    fn capacity_limit_evicts_old_entry() {
        let config = LookupCacheConfig {
            capacity: 2,
            ttl_secs: 5,
            negative_ttl_secs: 3,
        };
        let mut cache = LookupCache::new(config);
        cache.insert(1, "a", make_entry(1));
        cache.insert(1, "b", make_entry(2));
        cache.insert(1, "c", make_entry(3));
        let stats = cache.stats();
        assert!(stats.entries <= 2);
    }

    #[test]
    fn insert_negative_then_positive_positive_wins() {
        let mut cache = LookupCache::new(LookupCacheConfig::default());
        cache.insert_negative(1, "foo");
        cache.insert(1, "foo", make_entry(42));
        let result = cache.get(1, "foo");
        assert!(result.is_some());
        assert_eq!(result.unwrap().child_ino, 42);
    }

    #[test]
    fn insert_then_invalidate_get_returns_none() {
        let mut cache = LookupCache::new(LookupCacheConfig::default());
        cache.insert(1, "foo", make_entry(10));
        cache.invalidate(1, "foo");
        assert!(cache.get(1, "foo").is_none());
    }

    #[test]
    fn entry_type_is_regular() {
        assert!(EntryType::RegularFile.is_regular());
        assert!(!EntryType::Directory.is_regular());
        assert!(!EntryType::Symlink.is_regular());
    }

    #[test]
    fn entry_type_is_directory() {
        assert!(EntryType::Directory.is_directory());
        assert!(!EntryType::RegularFile.is_directory());
    }

    #[test]
    fn evict_expired_returns_count() {
        let config = LookupCacheConfig {
            capacity: 10,
            ttl_secs: 0,
            negative_ttl_secs: 0,
        };
        let mut cache = LookupCache::new(config);
        cache.insert(1, "a", make_entry(1));
        cache.insert(2, "b", make_entry(2));
        cache.insert_negative(3, "c");
        std::thread::sleep(Duration::from_millis(10));
        let evicted = cache.evict_expired();
        assert_eq!(evicted, 3);
    }

    #[test]
    fn multiple_parents_same_child_inode() {
        let mut cache = LookupCache::new(LookupCacheConfig::default());
        cache.insert(1, "foo", make_entry(100));
        cache.insert(2, "foo", make_entry(100));
        cache.insert(3, "foo", make_entry(100));
        cache.invalidate_inode(100);
        assert!(cache.get(1, "foo").is_none());
        assert!(cache.get(2, "foo").is_none());
        assert!(cache.get(3, "foo").is_none());
    }
}
