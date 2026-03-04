//! Replication health/throttle & reduce fingerprint/dedup security tests.
//!
//! Part of A10 Phase 15: Replication health & data integrity security audit

#[cfg(test)]
mod tests {
    use claudefs_reduce::dedupe::{CasIndex, Chunker, ChunkerConfig};
    use claudefs_reduce::fingerprint::{blake3_hash, super_features, ChunkHash, SuperFeatures};
    use claudefs_repl::health::{
        ClusterHealth, HealthThresholds, LinkHealth, LinkHealthReport, ReplicationHealthMonitor,
    };
    use claudefs_repl::throttle::TokenBucket as ReplTokenBucket;
    use claudefs_repl::throttle::{SiteThrottle, ThrottleConfig, ThrottleManager};

    macro_rules! finding {
        ($id:expr, $msg:expr) => {
            eprintln!("FINDING-REPL-HEALTH-{}: {}", $id, $msg)
        };
    }

    fn make_health_monitor() -> ReplicationHealthMonitor {
        ReplicationHealthMonitor::new(HealthThresholds::default())
    }

    fn make_health_monitor_custom(
        degraded_lag: u64,
        critical_lag: u64,
        disconnected_errors: u32,
    ) -> ReplicationHealthMonitor {
        let thresholds = HealthThresholds {
            degraded_lag_entries: degraded_lag,
            critical_lag_entries: critical_lag,
            disconnected_errors,
        };
        ReplicationHealthMonitor::new(thresholds)
    }

    fn make_throttle_config() -> ThrottleConfig {
        ThrottleConfig::default()
    }

    fn make_token_bucket(capacity: u64, rate: f64) -> ReplTokenBucket {
        ReplTokenBucket::new(capacity, rate)
    }

    // Category 1: Replication Health Monitoring (5 tests)

    #[test]
    fn test_health_initial_not_configured() {
        let monitor = make_health_monitor();
        let health = monitor.cluster_health();
        assert_eq!(health, ClusterHealth::NotConfigured);
    }

    #[test]
    fn test_health_register_and_check() {
        let mut monitor = make_health_monitor();
        monitor.register_site(1, "primary".to_string());
        monitor.record_success(1, 0, 1000000);

        let report = monitor.site_health(1).expect("site should exist");
        assert_eq!(report.health, LinkHealth::Healthy);
    }

    #[test]
    fn test_health_degraded_on_lag() {
        let mut monitor = make_health_monitor_custom(100, 1000, 5);
        monitor.register_site(1, "primary".to_string());
        monitor.record_success(1, 500, 1000000);

        let report = monitor.site_health(1).expect("site should exist");
        assert!(matches!(
            report.health,
            LinkHealth::Degraded {
                lag_entries: 500,
                ..
            }
        ));
    }

    #[test]
    fn test_health_disconnected_on_errors() {
        let mut monitor = make_health_monitor_custom(100, 1000, 3);
        monitor.register_site(1, "primary".to_string());

        monitor.record_error(1);
        monitor.record_error(1);
        monitor.record_error(1);

        let report = monitor.site_health(1).expect("site should exist");
        assert_eq!(report.health, LinkHealth::Disconnected);
    }

    #[test]
    fn test_health_cluster_aggregation() {
        let mut monitor = make_health_monitor_custom(100, 1000, 3);

        monitor.register_site(1, "primary".to_string());
        monitor.register_site(2, "secondary".to_string());
        monitor.register_site(3, "tertiary".to_string());

        monitor.record_success(1, 0, 1000000);
        monitor.record_success(2, 500, 1000000);
        monitor.record_error(3);
        monitor.record_error(3);
        monitor.record_error(3);

        let cluster_health = monitor.cluster_health();
        assert!(matches!(
            cluster_health,
            ClusterHealth::Degraded | ClusterHealth::Critical
        ));
    }

    // Category 2: Write Throttling (5 tests)

    #[test]
    fn test_throttle_token_bucket_consume() {
        let mut bucket = make_token_bucket(100, 10.0);
        let now = 0u64;

        assert_eq!(bucket.available(now), 100);

        let consumed = bucket.try_consume(50, now);
        assert!(consumed, "should consume 50 tokens");

        let try_60 = bucket.try_consume(60, now);
        assert!(!try_60, "should not consume 60 tokens after consuming 50");
    }

    #[test]
    fn test_throttle_site_send() {
        let mut throttle = SiteThrottle::new(make_throttle_config());
        let now = 0u64;

        let result = throttle.try_send(1000, 1, now);
        assert!(result, "should allow sending 1000 bytes, 1 entry");
    }

    #[test]
    fn test_throttle_manager_per_site() {
        let mut manager = ThrottleManager::new(make_throttle_config());
        manager.register_site(1, make_throttle_config());

        let now = 0u64;
        let result = manager.try_send(1, 1000, 10, now);
        assert!(result, "should allow send on registered site 1");

        let result_unregistered = manager.try_send(2, 1000, 10, now);
        assert!(
            result_unregistered,
            "unregistered site should default to allowed"
        );
    }

    #[test]
    fn test_throttle_config_update() {
        let mut manager = ThrottleManager::new(make_throttle_config());
        manager.register_site(1, make_throttle_config());

        let mut new_config = make_throttle_config();
        new_config.max_bytes_per_sec = 50 * 1024 * 1024;
        manager.update_site_config(1, new_config);

        let available = manager.available_bytes(1, 0);
        assert!(
            available <= 50 * 1024 * 1024,
            "available bytes should reflect new config"
        );
    }

    #[test]
    fn test_throttle_remove_site() {
        let mut manager = ThrottleManager::new(make_throttle_config());
        manager.register_site(1, make_throttle_config());
        manager.remove_site(1);

        let now = 0u64;
        let result = manager.try_send(1, 1000, 10, now);
        assert!(result, "removed site should default to allowed (unlimited)");
    }

    // Category 3: Data Fingerprinting (5 tests)

    #[test]
    fn test_blake3_deterministic() {
        let hash1 = blake3_hash(b"test data for hashing");
        let hash2 = blake3_hash(b"test data for hashing");
        assert_eq!(hash1, hash2, "same data should produce same hash");

        let hash3 = blake3_hash(b"different data");
        assert_ne!(hash1, hash3, "different data should produce different hash");
    }

    #[test]
    fn test_blake3_hash_hex() {
        let hash = blake3_hash(b"test data");
        let hex = hash.to_hex();

        assert_eq!(hex.len(), 64, "hex string should be 64 chars (32 bytes)");

        let bytes = hash.as_bytes();
        assert_eq!(bytes.len(), 32, "as_bytes should return 32-byte array");
    }

    #[test]
    fn test_super_features_similarity() {
        let data = b"this is some test data that we will use for similarity testing";
        let sf1 = super_features(data);
        let sf2 = super_features(data);

        assert_eq!(
            sf1.similarity(&sf2),
            4,
            "identical data should have similarity 4"
        );

        let different_data = b"completely different data here that has nothing in common";
        let sf3 = super_features(different_data);
        let sim = sf1.similarity(&sf3);
        assert!(sim < 4, "different data should have similarity < 4");
    }

    #[test]
    fn test_super_features_is_similar() {
        let mut data1 = vec![1u8; 100];
        let mut data2 = vec![1u8; 100];
        data2[10] = 2;

        let sf1 = super_features(&data1);
        let sf2 = super_features(&data2);

        assert!(
            sf1.is_similar(&sf2),
            "similar blocks should be detected as similar"
        );

        let different =
            b"totally different content that has nothing in common with the first block";
        let sf3 = super_features(different);
        assert!(
            !sf1.is_similar(&sf3),
            "very different blocks should not be similar"
        );
    }

    #[test]
    fn test_blake3_empty_data() {
        let empty_hash = blake3_hash(b"");
        let null_hash = blake3_hash(b"\0");

        assert_ne!(
            empty_hash, null_hash,
            "empty data should hash differently from null byte"
        );

        let hex = empty_hash.to_hex();
        assert!(!hex.is_empty(), "empty data should produce valid hex");
    }

    // Category 4: Deduplication CAS Index (5 tests)

    #[test]
    fn test_cas_index_insert_lookup() {
        let mut cas = CasIndex::new();
        let hash = blake3_hash(b"test chunk");

        assert!(!cas.lookup(&hash), "hash should not exist initially");

        cas.insert(hash);

        assert!(cas.lookup(&hash), "hash should exist after insert");

        let nonexistent = blake3_hash(b"nonexistent");
        assert!(
            !cas.lookup(&nonexistent),
            "nonexistent hash should return false"
        );
    }

    #[test]
    fn test_cas_index_refcount() {
        let mut cas = CasIndex::new();
        let hash = blake3_hash(b"shared chunk");

        cas.insert(hash);
        cas.insert(hash);
        cas.insert(hash);

        assert_eq!(
            cas.refcount(&hash),
            3,
            "refcount should be 3 after 3 inserts"
        );

        cas.release(&hash);

        assert_eq!(
            cas.refcount(&hash),
            2,
            "refcount should be 2 after one release"
        );
    }

    #[test]
    fn test_cas_index_drain_unreferenced() {
        let mut cas = CasIndex::new();

        let hash_a = blake3_hash(b"chunk A");
        let hash_b = blake3_hash(b"chunk B");

        cas.insert(hash_a);
        cas.insert(hash_b);
        cas.insert(hash_b);

        cas.release(&hash_a);
        cas.release(&hash_b);

        assert_eq!(cas.refcount(&hash_a), 0, "hash A should have refcount 0");
        assert_eq!(cas.refcount(&hash_b), 1, "hash B should have refcount 1");

        let drained = cas.drain_unreferenced();

        assert_eq!(drained.len(), 1, "should drain exactly 1 hash");
        assert!(drained.contains(&hash_a), "drained hash should be hash A");

        assert!(!cas.lookup(&hash_a), "hash A should no longer be present");
        assert!(cas.lookup(&hash_b), "hash B should still be present");
    }

    #[test]
    fn test_chunker_deterministic() {
        let chunker = Chunker::new();
        let data = b"some test data that we will chunk multiple times to verify determinism";

        let chunks1 = chunker.chunk(data);
        let chunks2 = chunker.chunk(data);

        assert_eq!(
            chunks1.len(),
            chunks2.len(),
            "should produce same number of chunks"
        );

        for (c1, c2) in chunks1.iter().zip(chunks2.iter()) {
            assert_eq!(c1.hash, c2.hash, "chunk hashes should match");
        }
    }

    #[test]
    fn test_chunker_config_sizes() {
        let config = ChunkerConfig::default();

        assert!(config.min_size > 0, "min_size should be > 0");
        assert!(config.avg_size > 0, "avg_size should be > 0");
        assert!(config.max_size > 0, "max_size should be > 0");

        assert!(
            config.min_size < config.avg_size,
            "min_size should be < avg_size"
        );
        assert!(
            config.avg_size < config.max_size,
            "avg_size should be < max_size"
        );
    }

    // Category 5: Health Reset & Edge Cases (5 tests)

    #[test]
    fn test_health_reset_site() {
        let mut monitor = make_health_monitor_custom(100, 1000, 3);
        monitor.register_site(1, "primary".to_string());

        monitor.record_error(1);
        monitor.record_error(1);
        monitor.record_error(1);

        let before_reset = monitor.site_health(1).expect("site should exist");
        assert_eq!(before_reset.health, LinkHealth::Disconnected);

        monitor.reset_site(1);

        let after_reset = monitor.site_health(1).expect("site should exist");
        assert_eq!(after_reset.consecutive_errors, 0, "errors should be reset");
    }

    #[test]
    fn test_health_thresholds_default() {
        let thresholds = HealthThresholds::default();

        assert!(
            thresholds.degraded_lag_entries > 0,
            "degraded_lag should be > 0"
        );
        assert!(
            thresholds.critical_lag_entries > thresholds.degraded_lag_entries,
            "critical_lag should be > degraded_lag"
        );
        assert!(
            thresholds.disconnected_errors > 0,
            "disconnected_errors should be > 0"
        );
    }

    #[test]
    fn test_health_remove_site() {
        let mut monitor = make_health_monitor();
        monitor.register_site(1, "primary".to_string());
        monitor.register_site(2, "secondary".to_string());

        monitor.remove_site(1);

        let reports = monitor.all_site_health();
        assert_eq!(reports.len(), 1, "should have 1 site after removal");
        assert_eq!(reports[0].site_id, 2);
    }

    #[test]
    fn test_cas_index_empty() {
        let mut cas = CasIndex::new();

        assert!(cas.is_empty(), "new CasIndex should be empty");
        assert_eq!(cas.len(), 0, "new CasIndex should have len 0");

        let hash = blake3_hash(b"test");
        cas.insert(hash);

        assert!(!cas.is_empty(), "CasIndex with entry should not be empty");
        assert_eq!(cas.len(), 1, "CasIndex with entry should have len 1");
    }

    #[test]
    fn test_throttle_config_defaults() {
        let config = ThrottleConfig::default();

        assert!(
            config.max_bytes_per_sec > 0,
            "max_bytes_per_sec should be > 0"
        );
        assert!(
            config.max_entries_per_sec > 0,
            "max_entries_per_sec should be > 0"
        );
        assert!(config.burst_factor >= 1.0, "burst_factor should be >= 1.0");
    }
}
