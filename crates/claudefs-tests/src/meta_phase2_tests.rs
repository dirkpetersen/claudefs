//! Tests for A2 Metadata Phase 2 modules: LockManager, NegativeCache, PathResolver

use claudefs_meta::locking::{LockEntry, LockManager, LockType};
use claudefs_meta::neg_cache::{NegCacheConfig, NegCacheStats, NegativeCache};
use claudefs_meta::pathres::{PathCacheEntry, PathResolver};
use claudefs_meta::types::{DirEntry, FileType, InodeId, MetaError, NodeId, ShardId};
use std::time::Duration;
use std::time::Instant;

fn ino(n: u64) -> InodeId {
    InodeId::new(n)
}

fn node(n: u64) -> NodeId {
    NodeId::new(n)
}

fn shard(n: u16) -> ShardId {
    ShardId::new(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== Section 1: LockManager Tests ====================

    #[test]
    fn test_lock_manager_new() {
        let mgr = LockManager::new();
        assert!(mgr.is_locked(ino(1)).is_ok());
    }

    #[test]
    fn test_is_locked_empty() {
        let mgr = LockManager::new();
        let locked = mgr.is_locked(ino(42)).unwrap();
        assert!(!locked);
    }

    #[test]
    fn test_acquire_read_lock() {
        let mgr = LockManager::new();
        let lock_id = mgr.acquire(ino(1), LockType::Read, node(1)).unwrap();
        assert!(lock_id > 0);
    }

    #[test]
    fn test_acquire_write_lock() {
        let mgr = LockManager::new();
        let lock_id = mgr.acquire(ino(1), LockType::Write, node(1)).unwrap();
        assert!(lock_id > 0);
    }

    #[test]
    fn test_is_locked_after_acquire() {
        let mgr = LockManager::new();
        mgr.acquire(ino(42), LockType::Read, node(1)).unwrap();
        let locked = mgr.is_locked(ino(42)).unwrap();
        assert!(locked);
    }

    #[test]
    fn test_locks_on_returns_entries() {
        let mgr = LockManager::new();
        mgr.acquire(ino(42), LockType::Read, node(1)).unwrap();
        let locks = mgr.locks_on(ino(42)).unwrap();
        assert_eq!(locks.len(), 1);
    }

    #[test]
    fn test_lock_entry_fields() {
        let mgr = LockManager::new();
        let lock_id = mgr.acquire(ino(42), LockType::Read, node(1)).unwrap();
        let locks = mgr.locks_on(ino(42)).unwrap();
        assert_eq!(locks.len(), 1);
        let entry = &locks[0];
        assert_eq!(entry.ino, ino(42));
        assert_eq!(entry.lock_type, LockType::Read);
        assert_eq!(entry.holder, node(1));
        assert_eq!(entry.lock_id, lock_id);
    }

    #[test]
    fn test_release_lock() {
        let mgr = LockManager::new();
        let lock_id = mgr.acquire(ino(42), LockType::Write, node(1)).unwrap();
        mgr.release(lock_id).unwrap();
        let locked = mgr.is_locked(ino(42)).unwrap();
        assert!(!locked);
    }

    #[test]
    fn test_release_nonexistent_lock() {
        let mgr = LockManager::new();
        let result = mgr.release(99999);
        assert!(result.is_ok());
    }

    #[test]
    fn test_multiple_readers_allowed() {
        let mgr = LockManager::new();
        mgr.acquire(ino(1), LockType::Read, node(1)).unwrap();
        mgr.acquire(ino(1), LockType::Read, node(2)).unwrap();
        let locks = mgr.locks_on(ino(1)).unwrap();
        assert_eq!(locks.len(), 2);
    }

    #[test]
    fn test_writer_blocks_reader() {
        let mgr = LockManager::new();
        mgr.acquire(ino(42), LockType::Write, node(1)).unwrap();
        let result = mgr.acquire(ino(42), LockType::Read, node(2));
        assert!(result.is_err());
    }

    #[test]
    fn test_reader_blocks_writer() {
        let mgr = LockManager::new();
        mgr.acquire(ino(42), LockType::Read, node(1)).unwrap();
        let result = mgr.acquire(ino(42), LockType::Write, node(2));
        assert!(result.is_err());
    }

    #[test]
    fn test_two_writers_fail() {
        let mgr = LockManager::new();
        mgr.acquire(ino(42), LockType::Write, node(1)).unwrap();
        let result = mgr.acquire(ino(42), LockType::Write, node(2));
        assert!(result.is_err());
    }

    #[test]
    fn test_lock_id_increments() {
        let mgr = LockManager::new();
        let id1 = mgr.acquire(ino(1), LockType::Read, node(1)).unwrap();
        let id2 = mgr.acquire(ino(2), LockType::Read, node(1)).unwrap();
        let id3 = mgr.acquire(ino(3), LockType::Write, node(1)).unwrap();
        assert_ne!(id1, id2);
        assert_ne!(id2, id3);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_release_all_for_node() {
        let mgr = LockManager::new();
        let node1 = node(1);
        mgr.acquire(ino(1), LockType::Read, node1).unwrap();
        mgr.acquire(ino(2), LockType::Read, node1).unwrap();
        mgr.acquire(ino(3), LockType::Write, node1).unwrap();
        mgr.acquire(ino(1), LockType::Read, node(2)).unwrap();
        let released = mgr.release_all_for_node(node1).unwrap();
        assert_eq!(released, 3);
    }

    #[test]
    fn test_release_all_for_node_count() {
        let mgr = LockManager::new();
        let node1 = node(1);
        mgr.acquire(ino(1), LockType::Read, node1).unwrap();
        let released = mgr.release_all_for_node(node1).unwrap();
        assert_eq!(released, 1);
        let locked = mgr.is_locked(ino(1)).unwrap();
        assert!(!locked);
    }

    #[test]
    fn test_lock_different_inodes_independent() {
        let mgr = LockManager::new();
        mgr.acquire(ino(1), LockType::Write, node(1)).unwrap();
        mgr.acquire(ino(2), LockType::Write, node(1)).unwrap();
        let locked1 = mgr.is_locked(ino(1)).unwrap();
        let locked2 = mgr.is_locked(ino(2)).unwrap();
        assert!(locked1);
        assert!(locked2);
    }

    #[test]
    fn test_locks_on_empty() {
        let mgr = LockManager::new();
        let locks = mgr.locks_on(ino(99)).unwrap();
        assert!(locks.is_empty());
    }

    #[test]
    fn test_read_then_release_then_write_ok() {
        let mgr = LockManager::new();
        let lock_id = mgr.acquire(ino(42), LockType::Read, node(1)).unwrap();
        mgr.release(lock_id).unwrap();
        let result = mgr.acquire(ino(42), LockType::Write, node(2));
        assert!(result.is_ok());
    }

    // ==================== Section 2: NegativeCache Tests ====================

    #[test]
    fn test_neg_cache_config_defaults() {
        let config = NegCacheConfig::default();
        assert_eq!(config.ttl, Duration::from_secs(3));
        assert_eq!(config.max_entries, 8192);
        assert!(config.enabled);
    }

    #[test]
    fn test_neg_cache_new() {
        let cache = NegativeCache::new(NegCacheConfig::default());
        assert_eq!(cache.entry_count(), 0);
    }

    #[test]
    fn test_is_negative_empty() {
        let mut cache = NegativeCache::new(NegCacheConfig::default());
        let result = cache.is_negative(&ino(1), "nonexistent");
        assert!(!result);
    }

    #[test]
    fn test_insert_makes_negative() {
        let mut cache = NegativeCache::new(NegCacheConfig::default());
        cache.insert(ino(1), "missing.txt".to_string());
        let result = cache.is_negative(&ino(1), "missing.txt");
        assert!(result);
    }

    #[test]
    fn test_is_negative_different_parent() {
        let mut cache = NegativeCache::new(NegCacheConfig::default());
        cache.insert(ino(1), "missing.txt".to_string());
        let result = cache.is_negative(&ino(2), "missing.txt");
        assert!(!result);
    }

    #[test]
    fn test_is_negative_different_name() {
        let mut cache = NegativeCache::new(NegCacheConfig::default());
        cache.insert(ino(1), "missing.txt".to_string());
        let result = cache.is_negative(&ino(1), "other.txt");
        assert!(!result);
    }

    #[test]
    fn test_stats_initial_zeros() {
        let cache = NegativeCache::new(NegCacheConfig::default());
        let stats = cache.stats();
        assert_eq!(stats.hits, 0);
        assert_eq!(stats.misses, 0);
        assert_eq!(stats.inserts, 0);
        assert_eq!(stats.invalidations, 0);
        assert_eq!(stats.expirations, 0);
    }

    #[test]
    fn test_insert_increments_inserts() {
        let mut cache = NegativeCache::new(NegCacheConfig::default());
        cache.insert(ino(1), "a.txt".to_string());
        assert_eq!(cache.stats().inserts, 1);
    }

    #[test]
    fn test_is_negative_hit_increments_hits() {
        let mut cache = NegativeCache::new(NegCacheConfig::default());
        cache.insert(ino(1), "a.txt".to_string());
        let _ = cache.is_negative(&ino(1), "a.txt");
        assert_eq!(cache.stats().hits, 1);
    }

    #[test]
    fn test_is_negative_miss_increments_misses() {
        let mut cache = NegativeCache::new(NegCacheConfig::default());
        let _ = cache.is_negative(&ino(1), "missing.txt");
        assert_eq!(cache.stats().misses, 1);
    }

    #[test]
    fn test_invalidate_removes_entry() {
        let mut cache = NegativeCache::new(NegCacheConfig::default());
        cache.insert(ino(1), "a.txt".to_string());
        cache.invalidate(&ino(1), "a.txt");
        let result = cache.is_negative(&ino(1), "a.txt");
        assert!(!result);
    }

    #[test]
    fn test_invalidate_increments_invalidations() {
        let mut cache = NegativeCache::new(NegCacheConfig::default());
        cache.insert(ino(1), "a.txt".to_string());
        cache.invalidate(&ino(1), "a.txt");
        assert_eq!(cache.stats().invalidations, 1);
    }

    #[test]
    fn test_invalidate_dir_removes_all_children() {
        let mut cache = NegativeCache::new(NegCacheConfig::default());
        cache.insert(ino(1), "a.txt".to_string());
        cache.insert(ino(1), "b.txt".to_string());
        cache.insert(ino(2), "c.txt".to_string());
        cache.invalidate_dir(&ino(1));
        assert!(!cache.is_negative(&ino(1), "a.txt"));
        assert!(!cache.is_negative(&ino(1), "b.txt"));
        assert!(cache.is_negative(&ino(2), "c.txt"));
    }

    #[test]
    fn test_entry_count_after_insert() {
        let mut cache = NegativeCache::new(NegCacheConfig::default());
        cache.insert(ino(1), "a.txt".to_string());
        assert_eq!(cache.entry_count(), 1);
    }

    #[test]
    fn test_entry_count_after_invalidate() {
        let mut cache = NegativeCache::new(NegCacheConfig::default());
        cache.insert(ino(1), "a.txt".to_string());
        cache.invalidate(&ino(1), "a.txt");
        assert_eq!(cache.entry_count(), 0);
    }

    #[test]
    fn test_hit_ratio_no_lookups() {
        let cache = NegativeCache::new(NegCacheConfig::default());
        assert_eq!(cache.hit_ratio(), 0.0);
    }

    #[test]
    fn test_hit_ratio_all_hits() {
        let mut cache = NegativeCache::new(NegCacheConfig::default());
        cache.insert(ino(1), "a.txt".to_string());
        let _ = cache.is_negative(&ino(1), "a.txt");
        let _ = cache.is_negative(&ino(1), "a.txt");
        assert_eq!(cache.hit_ratio(), 1.0);
    }

    #[test]
    fn test_hit_ratio_half_hits() {
        let mut cache = NegativeCache::new(NegCacheConfig::default());
        cache.insert(ino(1), "a.txt".to_string());
        let _ = cache.is_negative(&ino(1), "a.txt");
        let _ = cache.is_negative(&ino(1), "b.txt");
        let ratio = cache.hit_ratio();
        assert!((ratio - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_max_entries_not_exceeded() {
        let mut cache = NegativeCache::new(NegCacheConfig {
            max_entries: 2,
            ..Default::default()
        });
        for i in 0..5 {
            cache.insert(ino(1), format!("file{}.txt", i));
        }
        assert!(cache.entry_count() <= 2);
    }

    // ==================== Section 3: PathResolver Tests ====================

    #[test]
    fn test_path_resolver_new() {
        let resolver = PathResolver::new(256, 1000, 30, 10000);
        assert!(resolver.cache_size() == 0);
    }

    #[test]
    fn test_parse_path_simple() {
        let components = PathResolver::parse_path("a/b/c");
        assert_eq!(components, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_parse_path_root() {
        let components = PathResolver::parse_path("/");
        assert!(components.is_empty());
        let components2 = PathResolver::parse_path("");
        assert!(components2.is_empty());
    }

    #[test]
    fn test_parse_path_absolute() {
        let components = PathResolver::parse_path("/a/b");
        assert_eq!(components, vec!["a", "b"]);
    }

    #[test]
    fn test_parse_path_double_slash() {
        let components = PathResolver::parse_path("a//b");
        assert_eq!(components, vec!["a", "b"]);
    }

    #[test]
    fn test_cache_size_empty() {
        let resolver = PathResolver::new(256, 1000, 30, 10000);
        assert_eq!(resolver.cache_size(), 0);
    }

    #[test]
    fn test_cache_resolution_and_size() {
        let resolver = PathResolver::new(256, 1000, 30, 10000);
        resolver.cache_resolution(
            InodeId::ROOT_INODE,
            "home",
            PathCacheEntry {
                ino: ino(10),
                file_type: FileType::Directory,
                shard: shard(10),
            },
        );
        assert_eq!(resolver.cache_size(), 1);
    }

    #[test]
    fn test_speculative_resolve_empty_cache() {
        let resolver = PathResolver::new(256, 1000, 30, 10000);
        let (resolved, remaining) = resolver.speculative_resolve("/home/user/file.txt");
        assert!(resolved.is_empty());
        assert_eq!(remaining, vec!["home", "user", "file.txt"]);
    }

    #[test]
    fn test_speculative_resolve_partial_hit() {
        let resolver = PathResolver::new(256, 1000, 30, 10000);
        let home_ino = ino(10);
        resolver.cache_resolution(
            InodeId::ROOT_INODE,
            "home",
            PathCacheEntry {
                ino: home_ino,
                file_type: FileType::Directory,
                shard: home_ino.shard(256),
            },
        );
        let (resolved, remaining) = resolver.speculative_resolve("/home/user/file.txt");
        assert_eq!(resolved.len(), 1);
        assert_eq!(remaining, vec!["user", "file.txt"]);
    }

    #[test]
    fn test_invalidate_entry_removes() {
        let resolver = PathResolver::new(256, 1000, 30, 10000);
        resolver.cache_resolution(
            InodeId::ROOT_INODE,
            "a",
            PathCacheEntry {
                ino: ino(10),
                file_type: FileType::Directory,
                shard: shard(0),
            },
        );
        resolver.invalidate_entry(InodeId::ROOT_INODE, "a");
        assert_eq!(resolver.cache_size(), 0);
    }

    #[test]
    fn test_invalidate_parent_removes_all() {
        let resolver = PathResolver::new(256, 1000, 30, 10000);
        resolver.cache_resolution(
            InodeId::ROOT_INODE,
            "a",
            PathCacheEntry {
                ino: ino(10),
                file_type: FileType::Directory,
                shard: shard(0),
            },
        );
        resolver.cache_resolution(
            InodeId::ROOT_INODE,
            "b",
            PathCacheEntry {
                ino: ino(20),
                file_type: FileType::Directory,
                shard: shard(0),
            },
        );
        resolver.invalidate_parent(InodeId::ROOT_INODE);
        assert_eq!(resolver.cache_size(), 0);
    }

    #[test]
    fn test_clear_cache() {
        let resolver = PathResolver::new(256, 1000, 30, 10000);
        resolver.cache_resolution(
            InodeId::ROOT_INODE,
            "a",
            PathCacheEntry {
                ino: ino(10),
                file_type: FileType::Directory,
                shard: shard(0),
            },
        );
        resolver.clear_cache();
        assert_eq!(resolver.cache_size(), 0);
    }

    #[test]
    fn test_cache_negative() {
        let resolver = PathResolver::new(256, 1000, 30, 10000);
        let before = resolver.check_negative(InodeId::ROOT_INODE, "nonexistent");
        assert!(!before);
        resolver.cache_negative(InodeId::ROOT_INODE, "nonexistent");
        let after = resolver.check_negative(InodeId::ROOT_INODE, "nonexistent");
        assert!(after);
    }

    #[test]
    fn test_check_negative_false_initially() {
        let resolver = PathResolver::new(256, 1000, 30, 10000);
        let result = resolver.check_negative(InodeId::ROOT_INODE, "missing");
        assert!(!result);
    }

    #[test]
    fn test_invalidate_negative_removes() {
        let resolver = PathResolver::new(256, 1000, 30, 10000);
        resolver.cache_negative(InodeId::ROOT_INODE, "nonexistent");
        resolver.invalidate_negative(InodeId::ROOT_INODE, "nonexistent");
        let result = resolver.check_negative(InodeId::ROOT_INODE, "nonexistent");
        assert!(!result);
    }

    #[test]
    fn test_invalidate_negative_parent() {
        let resolver = PathResolver::new(256, 1000, 30, 10000);
        let parent = ino(100);
        resolver.cache_negative(parent, "file1");
        resolver.cache_negative(parent, "file2");
        resolver.invalidate_negative_parent(parent);
        assert_eq!(resolver.negative_cache_size(), 0);
    }

    #[test]
    fn test_negative_cache_size() {
        let resolver = PathResolver::new(256, 1000, 30, 10000);
        assert_eq!(resolver.negative_cache_size(), 0);
        resolver.cache_negative(InodeId::ROOT_INODE, "file1");
        resolver.cache_negative(InodeId::ROOT_INODE, "file2");
        assert_eq!(resolver.negative_cache_size(), 2);
    }

    #[test]
    fn test_resolve_path_with_lookup_fn_root() {
        let resolver = PathResolver::new(256, 1000, 30, 10000);
        let result = resolver.resolve_path("/", |_, _| {
            Ok(DirEntry {
                name: "root".to_string(),
                ino: InodeId::ROOT_INODE,
                file_type: FileType::Directory,
            })
        });
        assert!(result.is_ok());
    }

    #[test]
    fn test_resolve_path_with_lookup_fn_success() {
        let resolver = PathResolver::new(256, 1000, 30, 10000);
        let home_ino = ino(10);
        let user_ino = ino(20);
        let file_ino = ino(30);

        let result =
            resolver.resolve_path("/home/user/file.txt", |parent, name| match (parent, name) {
                (p, "home") if p == InodeId::ROOT_INODE => Ok(DirEntry {
                    name: "home".to_string(),
                    ino: home_ino,
                    file_type: FileType::Directory,
                }),
                (p, "user") if p == home_ino => Ok(DirEntry {
                    name: "user".to_string(),
                    ino: user_ino,
                    file_type: FileType::Directory,
                }),
                (p, "file.txt") if p == user_ino => Ok(DirEntry {
                    name: "file.txt".to_string(),
                    ino: file_ino,
                    file_type: FileType::RegularFile,
                }),
                (parent, name) => Err(MetaError::EntryNotFound {
                    parent,
                    name: name.to_string(),
                }),
            });
        assert_eq!(result.unwrap(), file_ino);
    }
}

#[cfg(test)]
mod proptest_tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn prop_lock_release_leaves_clean(ino_id in 1u64..1000, node_id in 1u64..10) {
            let mgr = LockManager::new();
            let lock_id = mgr.acquire(ino(ino_id), LockType::Write, node(node_id)).unwrap();
            mgr.release(lock_id).unwrap();
            let locked = mgr.is_locked(ino(ino_id)).unwrap();
            prop_assert!(!locked);
        }

        #[test]
        fn prop_neg_cache_insert_lookup(parent_id in 1u64..1000, name in "[a-z]{1,10}") {
            let mut cache = NegativeCache::new(NegCacheConfig::default());
            cache.insert(ino(parent_id), name.clone());
            let found = cache.is_negative(&ino(parent_id), &name);
            prop_assert!(found);
        }

        #[test]
        fn prop_parse_path_nonempty_segments(path in "[a-zA-Z0-9/]+") {
            let components = PathResolver::parse_path(&path);
            prop_assert!(!components.is_empty(), "Non-empty path should produce at least 1 segment");
        }
    }
}
