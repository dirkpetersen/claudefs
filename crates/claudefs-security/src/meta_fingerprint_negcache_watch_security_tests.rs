//! Metadata fingerprint index, negative cache, and watch/notify security tests.
//!
//! Part of A10 Phase 20: Meta fingerprint/neg-cache/watch security audit

use claudefs_meta::fingerprint::FingerprintIndex;
use claudefs_meta::neg_cache::{NegCacheConfig, NegativeCache};
use claudefs_meta::types::{InodeId, NodeId};
use claudefs_meta::watch::{WatchEvent, WatchManager};
use std::time::Duration;

fn make_hash(i: u8) -> [u8; 32] {
    let mut hash = [0u8; 32];
    hash[0] = i;
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fingerprint_insert_new() {
        let index = FingerprintIndex::new();
        let hash = make_hash(1);

        let result = index.insert(hash, 1000, 4096);
        assert!(result.expect("insert failed"));
        assert!(index.contains(&hash));
        assert_eq!(index.entry_count(), 1);
    }

    #[test]
    fn test_fingerprint_insert_duplicate_increments_ref() {
        let index = FingerprintIndex::new();
        let hash = make_hash(2);

        index.insert(hash, 1000, 4096).expect("insert failed");
        let result = index.insert(hash, 2000, 4096).expect("insert failed");

        assert!(!result);

        let entry = index.lookup(&hash).expect("lookup failed");
        assert_eq!(entry.ref_count, 2);
        assert_eq!(entry.block_location, 1000);
    }

    #[test]
    fn test_fingerprint_decrement_removes_at_zero() {
        let index = FingerprintIndex::new();
        let hash = make_hash(3);

        index.insert(hash, 1000, 4096).expect("insert failed");

        let new_count = index.decrement_ref(&hash).expect("decrement failed");
        assert_eq!(new_count, 0);
        assert!(!index.contains(&hash));
    }

    #[test]
    fn test_fingerprint_total_deduplicated_bytes() {
        let index = FingerprintIndex::new();

        index
            .insert(make_hash(1), 1000, 4096)
            .expect("insert failed");
        index
            .insert(make_hash(1), 1000, 4096)
            .expect("insert failed");

        index
            .insert(make_hash(2), 2000, 8192)
            .expect("insert failed");
        index
            .insert(make_hash(2), 2000, 8192)
            .expect("insert failed");
        index
            .insert(make_hash(2), 2000, 8192)
            .expect("insert failed");

        index
            .insert(make_hash(3), 3000, 1024)
            .expect("insert failed");

        let deduped = index.total_deduplicated_bytes();
        assert_eq!(deduped, 4096 + 16384);
    }

    #[test]
    fn test_fingerprint_increment_decrement_nonexistent() {
        let index = FingerprintIndex::new();
        let hash = make_hash(99);

        let result = index.increment_ref(&hash);
        assert!(result.is_err());

        let result = index.decrement_ref(&hash);
        assert!(result.is_err());
    }

    #[test]
    fn test_fingerprint_garbage_collect() {
        let index = FingerprintIndex::new();

        let hash1 = make_hash(1);
        let hash2 = make_hash(2);

        index.insert(hash1, 1000, 4096).expect("insert failed");
        index.insert(hash2, 2000, 4096).expect("insert failed");
        index.insert(hash2, 2000, 4096).expect("insert failed");

        index.decrement_ref(&hash2).expect("decrement failed");

        let removed = index.garbage_collect();
        assert_eq!(removed, 0);
        assert_eq!(index.entry_count(), 2);
    }

    #[test]
    fn test_fingerprint_multiple_hashes() {
        let index = FingerprintIndex::new();

        for i in 0..100u8 {
            let hash = make_hash(i);
            index
                .insert(hash, (i as u64) * 1000, 4096)
                .expect("insert failed");
        }

        assert_eq!(index.entry_count(), 100);

        for i in 0..100u8 {
            let hash = make_hash(i);
            let entry = index.lookup(&hash);
            assert!(entry.is_some(), "hash {} not found", i);
        }
    }

    #[test]
    fn test_fingerprint_lookup_nonexistent() {
        let index = FingerprintIndex::new();
        let hash = make_hash(100);

        let result = index.lookup(&hash);
        assert!(result.is_none());
    }

    #[test]
    fn test_fingerprint_entry_fields() {
        let index = FingerprintIndex::new();
        let hash = make_hash(4);

        index.insert(hash, 5000, 8192).expect("insert failed");

        let entry = index.lookup(&hash).expect("entry not found");
        assert_eq!(entry.hash, hash);
        assert_eq!(entry.ref_count, 1);
        assert_eq!(entry.block_location, 5000);
        assert_eq!(entry.size, 8192);
    }

    #[test]
    fn test_fingerprint_contains() {
        let index = FingerprintIndex::new();
        let hash = make_hash(5);

        assert!(!index.contains(&hash));

        index.insert(hash, 1000, 4096).expect("insert failed");

        assert!(index.contains(&hash));

        index.decrement_ref(&hash).expect("decrement failed");

        assert!(!index.contains(&hash));
    }

    #[test]
    fn test_neg_cache_insert_and_check() {
        let mut cache = NegativeCache::new(NegCacheConfig::default());
        cache.insert(InodeId::new(1), "missing.txt".to_string());

        assert!(cache.is_negative(&InodeId::new(1), "missing.txt"));
        assert!(!cache.is_negative(&InodeId::new(1), "other.txt"));
    }

    #[test]
    fn test_neg_cache_invalidation() {
        let mut cache = NegativeCache::new(NegCacheConfig::default());
        cache.insert(InodeId::new(1), "file.txt".to_string());

        cache.invalidate(&InodeId::new(1), "file.txt");

        assert!(!cache.is_negative(&InodeId::new(1), "file.txt"));
        assert_eq!(cache.stats().invalidations, 1);
    }

    #[test]
    fn test_neg_cache_dir_invalidation() {
        let mut cache = NegativeCache::new(NegCacheConfig::default());
        cache.insert(InodeId::new(1), "a.txt".to_string());
        cache.insert(InodeId::new(1), "b.txt".to_string());
        cache.insert(InodeId::new(1), "c.txt".to_string());
        cache.insert(InodeId::new(2), "d.txt".to_string());

        cache.invalidate_dir(&InodeId::new(1));

        assert!(!cache.is_negative(&InodeId::new(1), "a.txt"));
        assert!(!cache.is_negative(&InodeId::new(1), "b.txt"));
        assert!(!cache.is_negative(&InodeId::new(1), "c.txt"));
        assert!(cache.is_negative(&InodeId::new(2), "d.txt"));
    }

    #[test]
    fn test_neg_cache_disabled() {
        let mut cache = NegativeCache::new(NegCacheConfig {
            enabled: false,
            ..Default::default()
        });
        cache.insert(InodeId::new(1), "missing.txt".to_string());

        assert!(!cache.is_negative(&InodeId::new(1), "missing.txt"));
        assert_eq!(cache.entry_count(), 0);
    }

    #[test]
    fn test_neg_cache_max_entries_eviction() {
        let mut cache = NegativeCache::new(NegCacheConfig {
            max_entries: 3,
            ..Default::default()
        });
        cache.insert(InodeId::new(1), "a.txt".to_string());
        cache.insert(InodeId::new(1), "b.txt".to_string());
        cache.insert(InodeId::new(1), "c.txt".to_string());
        cache.insert(InodeId::new(1), "d.txt".to_string());

        assert!(cache.entry_count() <= 3);
    }

    #[test]
    fn test_neg_cache_stats_tracking() {
        let mut cache = NegativeCache::new(NegCacheConfig::default());
        cache.insert(InodeId::new(1), "missing.txt".to_string());

        cache.is_negative(&InodeId::new(1), "missing.txt");
        cache.is_negative(&InodeId::new(1), "other.txt");

        assert_eq!(cache.stats().hits, 1);
        assert_eq!(cache.stats().misses, 1);
        assert_eq!(cache.stats().inserts, 1);
    }

    #[test]
    fn test_neg_cache_hit_ratio() {
        let mut cache = NegativeCache::new(NegCacheConfig::default());
        cache.insert(InodeId::new(1), "a.txt".to_string());

        cache.is_negative(&InodeId::new(1), "a.txt");
        cache.is_negative(&InodeId::new(1), "b.txt");

        let ratio = cache.hit_ratio();
        assert!((ratio - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_neg_cache_ttl_expiration() {
        let mut cache = NegativeCache::new(NegCacheConfig {
            ttl: Duration::from_millis(10),
            ..Default::default()
        });
        cache.insert(InodeId::new(1), "temp.txt".to_string());

        std::thread::sleep(Duration::from_millis(15));

        assert!(!cache.is_negative(&InodeId::new(1), "temp.txt"));
        assert_eq!(cache.stats().expirations, 1);
    }

    #[test]
    fn test_neg_cache_cleanup_expired() {
        let mut cache = NegativeCache::new(NegCacheConfig {
            ttl: Duration::from_millis(10),
            ..Default::default()
        });
        cache.insert(InodeId::new(1), "a.txt".to_string());
        cache.insert(InodeId::new(1), "b.txt".to_string());

        std::thread::sleep(Duration::from_millis(15));

        let removed = cache.cleanup_expired();
        assert_eq!(removed, 2);
        assert_eq!(cache.entry_count(), 0);
    }

    #[test]
    fn test_neg_cache_clear() {
        let mut cache = NegativeCache::new(NegCacheConfig::default());
        cache.insert(InodeId::new(1), "a.txt".to_string());
        cache.insert(InodeId::new(2), "b.txt".to_string());

        cache.clear();

        assert_eq!(cache.entry_count(), 0);
    }

    #[test]
    fn test_watch_add_and_remove() {
        let manager = WatchManager::new(100);

        let watch_id = manager.add_watch(NodeId::new(1), InodeId::new(100), false);
        assert_eq!(manager.watch_count(), 1);

        assert!(manager.remove_watch(watch_id));
        assert_eq!(manager.watch_count(), 0);

        assert!(!manager.remove_watch(watch_id));
    }

    #[test]
    fn test_watch_notify_create_event() {
        let manager = WatchManager::new(100);

        manager.add_watch(NodeId::new(1), InodeId::new(100), false);

        manager.notify(WatchEvent::Create {
            parent: InodeId::new(100),
            name: "test.txt".to_string(),
            ino: InodeId::new(200),
        });

        let events = manager.drain_events(NodeId::new(1));
        assert_eq!(events.len(), 1);

        match &events[0] {
            WatchEvent::Create { parent, name, ino } => {
                assert_eq!(*parent, InodeId::new(100));
                assert_eq!(name, "test.txt");
                assert_eq!(*ino, InodeId::new(200));
            }
            _ => panic!("expected Create event"),
        }
    }

    #[test]
    fn test_watch_max_events_per_client() {
        let manager = WatchManager::new(2);

        manager.add_watch(NodeId::new(1), InodeId::new(100), false);

        manager.notify(WatchEvent::Create {
            parent: InodeId::new(100),
            name: "a.txt".to_string(),
            ino: InodeId::new(1),
        });
        manager.notify(WatchEvent::Create {
            parent: InodeId::new(100),
            name: "b.txt".to_string(),
            ino: InodeId::new(2),
        });
        manager.notify(WatchEvent::Create {
            parent: InodeId::new(100),
            name: "c.txt".to_string(),
            ino: InodeId::new(3),
        });

        let events = manager.drain_events(NodeId::new(1));
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn test_watch_remove_client_watches() {
        let manager = WatchManager::new(100);

        manager.add_watch(NodeId::new(1), InodeId::new(100), false);
        manager.add_watch(NodeId::new(1), InodeId::new(200), false);
        manager.add_watch(NodeId::new(2), InodeId::new(300), false);

        let removed = manager.remove_client_watches(NodeId::new(1));
        assert_eq!(removed, 2);
        assert_eq!(manager.watch_count(), 1);
    }

    #[test]
    fn test_watch_event_isolation() {
        let manager = WatchManager::new(100);

        manager.add_watch(NodeId::new(1), InodeId::new(100), false);
        manager.add_watch(NodeId::new(2), InodeId::new(200), false);

        manager.notify(WatchEvent::Create {
            parent: InodeId::new(100),
            name: "test.txt".to_string(),
            ino: InodeId::new(300),
        });

        let events_client1 = manager.drain_events(NodeId::new(1));
        assert_eq!(events_client1.len(), 1);

        let events_client2 = manager.drain_events(NodeId::new(2));
        assert_eq!(events_client2.len(), 0);
    }
}
