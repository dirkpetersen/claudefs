#![allow(dead_code)]

use crate::inode::{InodeId, InodeKind};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tracing::debug;

#[derive(Debug, Clone)]
pub struct DirEntry {
    pub name: String,
    pub ino: InodeId,
    pub kind: InodeKind,
}

#[derive(Debug, Clone)]
pub struct ReaddirSnapshot {
    pub entries: Vec<DirEntry>,
    pub inserted_at: Instant,
    pub ttl: Duration,
}

impl ReaddirSnapshot {
    pub fn is_expired(&self) -> bool {
        Instant::now().duration_since(self.inserted_at) > self.ttl
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[derive(Debug, Clone)]
pub struct DirCacheConfig {
    pub max_dirs: usize,
    pub ttl: Duration,
    pub negative_ttl: Duration,
}

impl Default for DirCacheConfig {
    fn default() -> Self {
        Self {
            max_dirs: 1024,
            ttl: Duration::from_secs(30),
            negative_ttl: Duration::from_secs(5),
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct DirCacheStats {
    pub hits: u64,
    pub misses: u64,
    pub negative_hits: u64,
    pub invalidations: u64,
    pub snapshots_cached: usize,
}

pub struct DirCache {
    snapshots: HashMap<InodeId, ReaddirSnapshot>,
    negative: HashMap<(InodeId, String), Instant>,
    config: DirCacheConfig,
    stats: DirCacheStats,
}

impl DirCache {
    pub fn new(config: DirCacheConfig) -> Self {
        Self {
            snapshots: HashMap::new(),
            negative: HashMap::new(),
            config,
            stats: DirCacheStats::default(),
        }
    }

    pub fn insert_snapshot(&mut self, dir_ino: InodeId, entries: Vec<DirEntry>) {
        if self.snapshots.len() >= self.config.max_dirs {
            self.evict_expired();
            if self.snapshots.len() >= self.config.max_dirs {
                if let Some(oldest) = self.snapshots.keys().cloned().collect::<Vec<_>>().pop() {
                    self.snapshots.remove(&oldest);
                }
            }
        }
        self.snapshots.insert(
            dir_ino,
            ReaddirSnapshot {
                entries,
                inserted_at: Instant::now(),
                ttl: self.config.ttl,
            },
        );
        self.stats.snapshots_cached = self.snapshots.len();
        debug!(
            "dir_cache: inserted snapshot for dir {} (cached: {})",
            dir_ino, self.stats.snapshots_cached
        );
    }

    pub fn get_snapshot(&mut self, dir_ino: InodeId) -> Option<ReaddirSnapshot> {
        if let Some(snap) = self.snapshots.get(&dir_ino).cloned() {
            if snap.is_expired() {
                self.snapshots.remove(&dir_ino);
                self.stats.snapshots_cached = self.snapshots.len();
                debug!("dir_cache: snapshot for dir {} expired", dir_ino);
                return None;
            }
            self.stats.hits += 1;
            Some(snap)
        } else {
            self.stats.misses += 1;
            None
        }
    }

    pub fn lookup(&mut self, parent: InodeId, name: &str) -> Option<DirEntry> {
        if self.is_negative(parent, name) {
            self.stats.negative_hits += 1;
            debug!("dir_cache: negative lookup hit for {}/{}", parent, name);
            return None;
        }
        if let Some(snap) = self.get_snapshot(parent) {
            snap.entries.iter().find(|e| e.name == name).cloned()
        } else {
            self.stats.misses += 1;
            None
        }
    }

    pub fn insert_negative(&mut self, parent: InodeId, name: &str) {
        self.negative
            .insert((parent, name.to_string()), Instant::now());
    }

    pub fn is_negative(&mut self, parent: InodeId, name: &str) -> bool {
        if let Some(at) = self.negative.get(&(parent, name.to_string())) {
            if Instant::now().duration_since(*at) > self.config.negative_ttl {
                self.negative.remove(&(parent, name.to_string()));
                false
            } else {
                true
            }
        } else {
            false
        }
    }

    pub fn invalidate_dir(&mut self, dir_ino: InodeId) {
        self.snapshots.remove(&dir_ino);
        self.stats.invalidations += 1;
        self.stats.snapshots_cached = self.snapshots.len();
        debug!("dir_cache: invalidated dir {}", dir_ino);
    }

    pub fn invalidate_entry(&mut self, parent: InodeId, name: &str) {
        self.negative.insert(
            (parent, name.to_string()),
            Instant::now() - self.config.negative_ttl - Duration::from_secs(1),
        );
    }

    pub fn evict_expired(&mut self) -> usize {
        let before = self.snapshots.len();
        self.snapshots.retain(|_, snap| !snap.is_expired());
        self.negative
            .retain(|_, at| Instant::now().duration_since(*at) <= self.config.negative_ttl);
        let evicted = before - self.snapshots.len();
        self.stats.snapshots_cached = self.snapshots.len();
        if evicted > 0 {
            debug!("dir_cache: evicted {} expired snapshots", evicted);
        }
        evicted
    }

    pub fn stats(&self) -> &DirCacheStats {
        &self.stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inode::InodeKind;

    fn make_entry(name: &str, ino: InodeId, kind: InodeKind) -> DirEntry {
        DirEntry {
            name: name.to_string(),
            ino,
            kind,
        }
    }

    #[test]
    fn test_insert_snapshot_and_get() {
        let mut cache = DirCache::new(DirCacheConfig::default());
        let entries = vec![
            make_entry("file1.txt", 2, InodeKind::File),
            make_entry("file2.txt", 3, InodeKind::File),
        ];
        cache.insert_snapshot(1, entries);

        let snapshot = cache.get_snapshot(1);
        assert!(snapshot.is_some());
        assert_eq!(snapshot.unwrap().len(), 2);
    }

    #[test]
    fn test_lookup_hit() {
        let mut cache = DirCache::new(DirCacheConfig::default());
        let entries = vec![make_entry("file1.txt", 2, InodeKind::File)];
        cache.insert_snapshot(1, entries);

        let result = cache.lookup(1, "file1.txt");
        assert!(result.is_some());
        assert_eq!(result.unwrap().ino, 2);
    }

    #[test]
    fn test_lookup_miss() {
        let mut cache = DirCache::new(DirCacheConfig::default());
        let entries = vec![make_entry("file1.txt", 2, InodeKind::File)];
        cache.insert_snapshot(1, entries);

        let result = cache.lookup(1, "nonexistent");
        assert!(result.is_none());
    }

    #[test]
    fn test_negative_entry() {
        let mut cache = DirCache::new(DirCacheConfig::default());
        cache.insert_negative(1, "deleted.txt");

        assert!(cache.is_negative(1, "deleted.txt"));
    }

    #[test]
    fn test_negative_expires() {
        let mut cache = DirCache::new(DirCacheConfig {
            max_dirs: 100,
            ttl: Duration::from_secs(30),
            negative_ttl: Duration::from_millis(10),
        });
        cache.insert_negative(1, "deleted.txt");

        std::thread::sleep(Duration::from_millis(20));

        assert!(!cache.is_negative(1, "deleted.txt"));
    }

    #[test]
    fn test_invalidate_dir() {
        let mut cache = DirCache::new(DirCacheConfig::default());
        cache.insert_snapshot(1, vec![make_entry("file.txt", 2, InodeKind::File)]);

        cache.invalidate_dir(1);

        assert!(cache.get_snapshot(1).is_none());
    }

    #[test]
    fn test_invalidate_entry() {
        let mut cache = DirCache::new(DirCacheConfig::default());
        cache.insert_negative(1, "deleted.txt");

        cache.invalidate_entry(1, "deleted.txt");

        assert!(!cache.is_negative(1, "deleted.txt"));
    }

    #[test]
    fn test_evict_expired() {
        let mut cache = DirCache::new(DirCacheConfig {
            max_dirs: 100,
            ttl: Duration::from_millis(10),
            negative_ttl: Duration::from_secs(5),
        });
        cache.insert_snapshot(1, vec![]);

        std::thread::sleep(Duration::from_millis(20));

        let evicted = cache.evict_expired();
        assert!(evicted > 0);
        assert!(cache.get_snapshot(1).is_none());
    }

    #[test]
    fn test_ttl_expiry() {
        let mut cache = DirCache::new(DirCacheConfig {
            max_dirs: 100,
            ttl: Duration::from_millis(10),
            negative_ttl: Duration::from_secs(5),
        });
        cache.insert_snapshot(1, vec![make_entry("file.txt", 2, InodeKind::File)]);

        std::thread::sleep(Duration::from_millis(20));

        assert!(cache.get_snapshot(1).is_none());
    }

    #[test]
    fn test_stats_hits_and_misses() {
        let mut cache = DirCache::new(DirCacheConfig::default());

        cache.get_snapshot(1);
        assert_eq!(cache.stats().misses, 1);

        cache.insert_snapshot(2, vec![]);
        cache.get_snapshot(2);
        assert_eq!(cache.stats().hits, 1);
    }

    #[test]
    fn test_stats_negative_hits() {
        let mut cache = DirCache::new(DirCacheConfig::default());
        cache.insert_negative(1, "test");

        cache.lookup(1, "test");

        assert_eq!(cache.stats().negative_hits, 1);
    }

    #[test]
    fn test_default_config() {
        let config = DirCacheConfig::default();
        assert_eq!(config.max_dirs, 1024);
        assert_eq!(config.ttl, Duration::from_secs(30));
        assert_eq!(config.negative_ttl, Duration::from_secs(5));
    }

    #[test]
    fn test_snapshot_len() {
        let snapshot = ReaddirSnapshot {
            entries: vec![
                make_entry("a", 1, InodeKind::File),
                make_entry("b", 2, InodeKind::File),
            ],
            inserted_at: Instant::now(),
            ttl: Duration::from_secs(30),
        };
        assert_eq!(snapshot.len(), 2);
    }

    #[test]
    fn test_snapshot_is_empty() {
        let snapshot = ReaddirSnapshot {
            entries: vec![],
            inserted_at: Instant::now(),
            ttl: Duration::from_secs(30),
        };
        assert!(snapshot.is_empty());
    }

    #[test]
    fn test_multiple_dirs() {
        let mut cache = DirCache::new(DirCacheConfig::default());
        cache.insert_snapshot(1, vec![make_entry("a", 10, InodeKind::File)]);
        cache.insert_snapshot(2, vec![make_entry("b", 20, InodeKind::File)]);

        assert_eq!(cache.get_snapshot(1).unwrap().len(), 1);
        assert_eq!(cache.get_snapshot(2).unwrap().len(), 1);
    }

    #[test]
    fn test_max_dirs_limit() {
        let mut cache = DirCache::new(DirCacheConfig {
            max_dirs: 2,
            ttl: Duration::from_secs(30),
            negative_ttl: Duration::from_secs(5),
        });
        cache.insert_snapshot(1, vec![]);
        cache.insert_snapshot(2, vec![]);
        cache.insert_snapshot(3, vec![]);

        assert!(cache.stats().snapshots_cached <= 2);
    }

    #[test]
    fn test_dir_entry_clone() {
        let entry = make_entry("test", 1, InodeKind::File);
        let cloned = entry.clone();
        assert_eq!(cloned.name, "test");
        assert_eq!(cloned.ino, 1);
    }

    #[test]
    fn test_lookup_different_parent() {
        let mut cache = DirCache::new(DirCacheConfig::default());
        cache.insert_snapshot(1, vec![make_entry("file.txt", 2, InodeKind::File)]);

        let result = cache.lookup(2, "file.txt");
        assert!(result.is_none());
    }

    #[test]
    fn test_negative_for_different_parent() {
        let mut cache = DirCache::new(DirCacheConfig::default());
        cache.insert_negative(1, "test");

        assert!(!cache.is_negative(2, "test"));
    }

    #[test]
    fn test_invalidate_nonexistent_dir() {
        let mut cache = DirCache::new(DirCacheConfig::default());
        cache.invalidate_dir(999);

        assert_eq!(cache.stats().invalidations, 1);
    }

    #[test]
    fn test_snapshot_is_not_expired_immediately() {
        let snapshot = ReaddirSnapshot {
            entries: vec![],
            inserted_at: Instant::now(),
            ttl: Duration::from_secs(30),
        };
        assert!(!snapshot.is_expired());
    }

    #[test]
    fn test_stats_snapshot_count() {
        let mut cache = DirCache::new(DirCacheConfig::default());
        cache.insert_snapshot(1, vec![]);
        cache.insert_snapshot(2, vec![]);

        assert_eq!(cache.stats().snapshots_cached, 2);
    }

    #[test]
    fn test_clear_negative_on_invalidate_entry() {
        let mut cache = DirCache::new(DirCacheConfig::default());
        cache.insert_negative(1, "test");

        assert!(cache.is_negative(1, "test"));

        cache.invalidate_entry(1, "test");

        assert!(!cache.is_negative(1, "test"));
    }

    #[test]
    fn test_kinds_in_entries() {
        let mut cache = DirCache::new(DirCacheConfig::default());
        cache.insert_snapshot(
            1,
            vec![
                make_entry("dir1", 2, InodeKind::Directory),
                make_entry("file1", 3, InodeKind::File),
                make_entry("sym1", 4, InodeKind::Symlink),
            ],
        );

        let result = cache.lookup(1, "dir1");
        assert!(result.is_some());
        assert_eq!(result.unwrap().kind, InodeKind::Directory);

        let result = cache.lookup(1, "file1");
        assert!(result.is_some());
        assert_eq!(result.unwrap().kind, InodeKind::File);
    }

    #[test]
    fn test_lookup_returns_correct_entry() {
        let mut cache = DirCache::new(DirCacheConfig::default());
        cache.insert_snapshot(
            1,
            vec![
                make_entry("a", 10, InodeKind::File),
                make_entry("b", 20, InodeKind::File),
                make_entry("c", 30, InodeKind::File),
            ],
        );

        let result = cache.lookup(1, "b").unwrap();
        assert_eq!(result.ino, 20);
    }

    #[test]
    fn test_consecutive_lookups() {
        let mut cache = DirCache::new(DirCacheConfig::default());
        cache.insert_snapshot(1, vec![make_entry("a", 10, InodeKind::File)]);

        let _ = cache.lookup(1, "a");
        let _ = cache.lookup(1, "a");

        assert_eq!(cache.stats().hits, 2);
    }
}
