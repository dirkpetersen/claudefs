//! Server-side attribute cache for NFSv3

use crate::protocol::{Fattr3, FileHandle3};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// A cached attribute entry with TTL
#[derive(Debug, Clone)]
pub struct CachedAttr {
    /// The cached file attributes
    pub attr: Fattr3,
    /// When the entry was cached
    pub cached_at: Instant,
    /// Time-to-live duration
    pub ttl: Duration,
}

impl CachedAttr {
    /// Create a new cached attribute entry with the given TTL
    pub fn new(attr: Fattr3, ttl: Duration) -> Self {
        Self {
            attr,
            cached_at: Instant::now(),
            ttl,
        }
    }

    /// Check if the cache entry has expired
    pub fn is_expired(&self) -> bool {
        self.cached_at.elapsed() >= self.ttl
    }

    /// Get the age of the cache entry in milliseconds
    pub fn age_ms(&self) -> u64 {
        self.cached_at.elapsed().as_millis() as u64
    }
}

/// Server-side attribute cache for NFSv3
/// Caches getattr results to reduce metadata load on the backend
pub struct AttrCache {
    entries: Mutex<HashMap<Vec<u8>, CachedAttr>>,
    max_entries: usize,
    default_ttl: Duration,
    hits: std::sync::atomic::AtomicU64,
    misses: std::sync::atomic::AtomicU64,
}

impl AttrCache {
    /// Create a new attribute cache
    /// max_entries: maximum number of entries (evict oldest when exceeded)
    /// default_ttl_secs: default TTL in seconds
    pub fn new(max_entries: usize, default_ttl_secs: u64) -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            max_entries,
            default_ttl: Duration::from_secs(default_ttl_secs),
            hits: std::sync::atomic::AtomicU64::new(0),
            misses: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// Insert an attribute for a file handle
    pub fn insert(&self, fh: &FileHandle3, attr: Fattr3) {
        self.insert_with_ttl(fh, attr, self.default_ttl);
    }

    /// Insert with a custom TTL
    pub fn insert_with_ttl(&self, fh: &FileHandle3, attr: Fattr3, ttl: Duration) {
        let mut entries = self.entries.lock().unwrap();

        if entries.len() >= self.max_entries && !entries.is_empty() {
            let oldest_key = entries
                .iter()
                .min_by_key(|(_, v)| v.cached_at)
                .map(|(k, _)| k.clone());
            if let Some(key) = oldest_key {
                entries.remove(&key);
            }
        }

        entries.insert(fh.data.clone(), CachedAttr::new(attr, ttl));
    }

    /// Look up a cached attribute (returns None if not found or expired)
    pub fn get(&self, fh: &FileHandle3) -> Option<Fattr3> {
        let entries = self.entries.lock().unwrap();
        match entries.get(&fh.data) {
            Some(cached) if !cached.is_expired() => {
                self.hits.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                Some(cached.attr.clone())
            }
            _ => {
                self.misses
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                None
            }
        }
    }

    /// Invalidate a file handle's cache entry
    pub fn invalidate(&self, fh: &FileHandle3) {
        self.entries.lock().unwrap().remove(&fh.data);
    }

    /// Invalidate all entries
    pub fn invalidate_all(&self) {
        self.entries.lock().unwrap().clear();
    }

    /// Return cache statistics: (hits, misses, current_size)
    pub fn stats(&self) -> (u64, u64, usize) {
        let entries = self.entries.lock().unwrap();
        (
            self.hits.load(std::sync::atomic::Ordering::Relaxed),
            self.misses.load(std::sync::atomic::Ordering::Relaxed),
            entries.len(),
        )
    }

    /// Hit rate (0.0 to 1.0)
    pub fn hit_rate(&self) -> f64 {
        let hits = self.hits.load(std::sync::atomic::Ordering::Relaxed);
        let misses = self.misses.load(std::sync::atomic::Ordering::Relaxed);
        let total = hits + misses;
        if total == 0 {
            0.0
        } else {
            hits as f64 / total as f64
        }
    }

    /// Evict expired entries
    pub fn evict_expired(&self) -> usize {
        let mut entries = self.entries.lock().unwrap();
        let before = entries.len();
        entries.retain(|_, v| !v.is_expired());
        before - entries.len()
    }

    /// Current number of entries (including expired)
    pub fn len(&self) -> usize {
        self.entries.lock().unwrap().len()
    }

    /// Returns true if the cache contains no entries
    pub fn is_empty(&self) -> bool {
        self.entries.lock().unwrap().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Ftype3;

    fn make_fh(data: &[u8]) -> FileHandle3 {
        FileHandle3::new(data.to_vec()).unwrap()
    }

    fn make_attr(inode: u64) -> Fattr3 {
        Fattr3 {
            ftype: Ftype3::Reg,
            mode: 0o644,
            nlink: 1,
            uid: 0,
            gid: 0,
            size: 4096,
            used: 4096,
            rdev: (0, 0),
            fsid: 1,
            fileid: inode,
            atime: crate::protocol::Nfstime3::zero(),
            mtime: crate::protocol::Nfstime3::zero(),
            ctime: crate::protocol::Nfstime3::zero(),
        }
    }

    #[test]
    fn test_cached_attr_new() {
        let attr = make_attr(1);
        let cached = CachedAttr::new(attr, Duration::from_secs(60));
        assert_eq!(cached.attr.fileid, 1);
        assert!(!cached.is_expired());
    }

    #[test]
    fn test_cached_attr_is_expired() {
        let attr = make_attr(1);
        let cached = CachedAttr::new(attr, Duration::from_millis(1));
        std::thread::sleep(Duration::from_millis(10));
        assert!(cached.is_expired());
    }

    #[test]
    fn test_cached_attr_age_ms() {
        let attr = make_attr(1);
        let cached = CachedAttr::new(attr, Duration::from_secs(60));
        std::thread::sleep(Duration::from_millis(50));
        let age = cached.age_ms();
        assert!(age >= 40 && age < 200);
    }

    #[test]
    fn test_attr_cache_insert_get() {
        let cache = AttrCache::new(100, 60);
        let fh = make_fh(b"file1");
        let attr = make_attr(1);

        cache.insert(&fh, attr.clone());

        let cached = cache.get(&fh);
        assert!(cached.is_some());
        assert_eq!(cached.unwrap().fileid, 1);
    }

    #[test]
    fn test_attr_cache_miss() {
        let cache = AttrCache::new(100, 60);
        let fh = make_fh(b"nonexistent");

        let result = cache.get(&fh);
        assert!(result.is_none());
    }

    #[test]
    fn test_attr_cache_ttl_expiry() {
        let cache = AttrCache::new(100, 0);
        let fh = make_fh(b"file1");
        let attr = make_attr(1);

        cache.insert(&fh, attr);

        std::thread::sleep(Duration::from_millis(10));
        let result = cache.get(&fh);
        assert!(result.is_none());
    }

    #[test]
    fn test_attr_cache_invalidate() {
        let cache = AttrCache::new(100, 60);
        let fh = make_fh(b"file1");
        let attr = make_attr(1);

        cache.insert(&fh, attr);
        cache.invalidate(&fh);

        let result = cache.get(&fh);
        assert!(result.is_none());
    }

    #[test]
    fn test_attr_cache_invalidate_all() {
        let cache = AttrCache::new(100, 60);
        let fh1 = make_fh(b"file1");
        let fh2 = make_fh(b"file2");

        cache.insert(&fh1, make_attr(1));
        cache.insert(&fh2, make_attr(2));
        cache.invalidate_all();

        assert!(cache.get(&fh1).is_none());
        assert!(cache.get(&fh2).is_none());
    }

    #[test]
    fn test_attr_cache_stats() {
        let cache = AttrCache::new(100, 60);
        let fh = make_fh(b"file1");
        let attr = make_attr(1);

        cache.insert(&fh, attr);
        cache.get(&fh).unwrap();
        cache.get(&make_fh(b"missing"));

        let (hits, misses, size) = cache.stats();
        assert_eq!(hits, 1);
        assert_eq!(misses, 1);
        assert_eq!(size, 1);
    }

    #[test]
    fn test_attr_cache_hit_rate() {
        let cache = AttrCache::new(100, 60);
        let fh = make_fh(b"file1");
        let attr = make_attr(1);

        cache.insert(&fh, attr);
        for _ in 0..3 {
            cache.get(&fh).unwrap();
        }
        cache.get(&make_fh(b"missing"));

        let rate = cache.hit_rate();
        assert!((rate - 0.75).abs() < 0.01);
    }

    #[test]
    fn test_attr_cache_evict_expired() {
        let cache = AttrCache::new(100, 0);
        let fh1 = make_fh(b"file1");
        let fh2 = make_fh(b"file2");

        cache.insert(&fh1, make_attr(1));
        cache.insert(&fh2, make_attr(2));

        std::thread::sleep(Duration::from_millis(20));

        let evicted = cache.evict_expired();
        assert_eq!(evicted, 2);
    }

    #[test]
    fn test_attr_cache_capacity_limit() {
        let cache = AttrCache::new(2, 60);
        let fh1 = make_fh(b"file1");
        let fh2 = make_fh(b"file2");
        let fh3 = make_fh(b"file3");

        cache.insert(&fh1, make_attr(1));
        cache.insert(&fh2, make_attr(2));
        cache.insert(&fh3, make_attr(3));

        assert!(cache.len() <= 2);
    }

    #[test]
    fn test_attr_cache_len() {
        let cache = AttrCache::new(100, 60);
        assert_eq!(cache.len(), 0);

        cache.insert(&make_fh(b"file1"), make_attr(1));
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn test_attr_cache_is_empty() {
        let cache = AttrCache::new(100, 60);
        assert!(cache.is_empty());

        cache.insert(&make_fh(b"file1"), make_attr(1));
        assert!(!cache.is_empty());
    }
}
