use crate::operations::DirEntry;
use lru::LruCache;
use std::num::NonZeroUsize;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct DirCacheConfig {
    pub capacity: usize,
    pub ttl_secs: u64,
}

impl Default for DirCacheConfig {
    fn default() -> Self {
        DirCacheConfig {
            capacity: 1_000,
            ttl_secs: 60,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DirCacheEntry {
    pub entries: Vec<DirEntry>,
    pub inserted_at: Instant,
    pub mtime: u64,
}

impl DirCacheEntry {
    fn is_expired(&self, ttl_secs: u64) -> bool {
        self.inserted_at.elapsed() > Duration::from_secs(ttl_secs)
    }
}

pub struct DirCache {
    entries: LruCache<u64, DirCacheEntry>,
    config: DirCacheConfig,
    stats: DirCacheStats,
}

#[derive(Debug, Default, Clone)]
pub struct DirCacheStats {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub invalidations: u64,
    pub size: usize,
}

impl DirCache {
    pub fn new(config: DirCacheConfig) -> Self {
        let capacity =
            NonZeroUsize::new(config.capacity).unwrap_or(NonZeroUsize::new(1_000).unwrap());
        DirCache {
            entries: LruCache::new(capacity),
            config,
            stats: DirCacheStats::default(),
        }
    }

    pub fn get(&mut self, ino: u64) -> Option<Vec<DirEntry>> {
        if let Some(entry) = self.entries.get(&ino) {
            if entry.is_expired(self.config.ttl_secs) {
                self.entries.pop(&ino);
                self.stats.misses += 1;
                None
            } else {
                self.stats.hits += 1;
                Some(entry.entries.clone())
            }
        } else {
            self.stats.misses += 1;
            None
        }
    }

    pub fn insert(&mut self, ino: u64, entries: Vec<DirEntry>, mtime: u64) {
        let prev_len = self.entries.len();
        self.entries.push(
            ino,
            DirCacheEntry {
                entries,
                inserted_at: Instant::now(),
                mtime,
            },
        );
        if self.entries.len() <= prev_len {
            self.stats.evictions += 1;
        }
    }

    pub fn invalidate(&mut self, ino: u64) {
        if self.entries.pop(&ino).is_some() {
            self.stats.invalidations += 1;
        }
    }

    pub fn invalidate_tree(&mut self, ino: u64) {
        self.entries.pop(&ino);
    }

    pub fn stats(&self) -> DirCacheStats {
        DirCacheStats {
            hits: self.stats.hits,
            misses: self.stats.misses,
            evictions: self.stats.evictions,
            invalidations: self.stats.invalidations,
            size: self.entries.len(),
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get_mtime(&mut self, ino: u64) -> Option<u64> {
        self.entries.get(&ino).map(|e| e.mtime)
    }

    pub fn is_stale(&mut self, ino: u64, current_mtime: u64) -> bool {
        if let Some(cached_mtime) = self.get_mtime(ino) {
            cached_mtime != current_mtime
        } else {
            true
        }
    }
}

impl Default for DirCache {
    fn default() -> Self {
        Self::new(DirCacheConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fuser::FileType;

    fn test_entry(ino: u64, name: &str) -> DirEntry {
        DirEntry {
            ino,
            offset: ino as i64,
            kind: FileType::RegularFile,
            name: name.to_string(),
        }
    }

    #[test]
    fn test_insert_and_get_within_ttl() {
        let mut cache = DirCache::new(DirCacheConfig {
            capacity: 100,
            ttl_secs: 60,
        });
        let entries = vec![test_entry(2, "file1.txt"), test_entry(3, "file2.txt")];
        cache.insert(1, entries.clone(), 1000);

        let result = cache.get(1);
        assert!(result.is_some());
        assert_eq!(result.unwrap().len(), 2);
    }

    #[test]
    fn test_get_after_ttl_expiry() {
        let mut cache = DirCache::new(DirCacheConfig {
            capacity: 100,
            ttl_secs: 0,
        });
        let entries = vec![test_entry(2, "file1.txt")];
        cache.insert(1, entries, 1000);

        std::thread::sleep(Duration::from_millis(10));

        let result = cache.get(1);
        assert!(result.is_none());
    }

    #[test]
    fn test_invalidate_removes_entry() {
        let mut cache = DirCache::new(DirCacheConfig::default());
        let entries = vec![test_entry(2, "file1.txt")];
        cache.insert(1, entries, 1000);

        cache.invalidate(1);

        assert!(cache.get(1).is_none());
    }

    #[test]
    fn test_clear_empties_cache() {
        let mut cache = DirCache::new(DirCacheConfig::default());
        cache.insert(1, vec![test_entry(2, "file1.txt")], 1000);
        cache.insert(2, vec![test_entry(3, "file2.txt")], 1000);

        cache.clear();

        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn test_stats_track_hits_and_misses() {
        let mut cache = DirCache::new(DirCacheConfig::default());

        cache.get(1);
        assert_eq!(cache.stats().misses, 1);

        cache.insert(2, vec![test_entry(3, "file.txt")], 1000);
        cache.get(2);
        assert_eq!(cache.stats().hits, 1);
    }

    #[test]
    fn test_capacity_eviction() {
        let config = DirCacheConfig {
            capacity: 2,
            ttl_secs: 60,
        };
        let mut cache = DirCache::new(config);

        cache.insert(1, vec![test_entry(10, "a")], 1000);
        cache.insert(2, vec![test_entry(20, "b")], 1000);
        cache.insert(3, vec![test_entry(30, "c")], 1000);

        assert!(cache.len() <= 2);
    }

    #[test]
    fn test_mtime_tracking() {
        let mut cache = DirCache::new(DirCacheConfig::default());
        cache.insert(1, vec![test_entry(2, "file.txt")], 1000);

        assert_eq!(cache.get_mtime(1), Some(1000));
        assert!(cache.is_stale(1, 1000));
        assert!(!cache.is_stale(1, 1001));
    }

    #[test]
    fn test_default_config_sensible_values() {
        let config = DirCacheConfig::default();
        assert_eq!(config.capacity, 1_000);
        assert_eq!(config.ttl_secs, 60);
    }
}
