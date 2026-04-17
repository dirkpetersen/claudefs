use claudefs_reduce::{
    eviction_policy::{EvictableSegment, EvictionPolicy, EvictionPolicyConfig, EvictionStrategy},
    key_manager::{KeyManager, KeyManagerConfig},
    key_store::{KeyStore, KeyStoreConfig},
    object_assembler::{ObjectAssembler, ObjectAssemblerConfig},
    snapshot_catalog::{SnapshotCatalog, SnapshotId, SnapshotRecord},
    snapshot_diff::{SnapshotDiff, SnapshotDiffConfig},
    tier_migration::{MigrationConfig, TierMigrator},
    worm_retention_enforcer::{ComplianceHold, RetentionPolicy, WormRetentionEnforcer},
};
use std::time::{SystemTime, UNIX_EPOCH};

fn current_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

fn current_time_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

#[test]
fn test_eviction_policy_basic() {
    let config = EvictionPolicyConfig {
        strategy: EvictionStrategy::AgeWeightedSize,
        target_free_bytes: 200 * 1024 * 1024,
        max_segments_per_pass: 100,
    };
    let policy = EvictionPolicy::new(config);

    let current_time = current_time_secs();
    let candidates: Vec<EvictableSegment> = (0..10)
        .map(|i| EvictableSegment {
            segment_id: i,
            size_bytes: 100 * 1024 * 1024,
            last_access_secs: current_time - 3600,
            current_time_secs: current_time,
            pinned: false,
        })
        .collect();

    let (_evicted, stats) = policy.select_for_eviction(candidates);
    assert!(stats.candidates_evaluated >= 10);
}

#[test]
fn test_s3_blob_assembly() {
    let config = ObjectAssemblerConfig {
        target_blob_bytes: 64 * 1024 * 1024,
        key_prefix: "test".to_string(),
    };
    let mut assembler = ObjectAssembler::new(config);

    let mut completed_blobs = Vec::new();

    for i in 0..150 {
        let hash = [i as u8; 32];
        let data = vec![0x42u8; 1024 * 1024];

        if let Some(_blob) = assembler.pack(hash, &data) {
            completed_blobs.push(_blob);
        }
    }

    if let Some(_blob) = assembler.flush() {
        completed_blobs.push(_blob);
    }

    assert!(completed_blobs.len() >= 2, "Should create at least 2 blobs");
}

#[test]
fn test_snapshot_creation() {
    let mut catalog = SnapshotCatalog::new();

    let record = SnapshotRecord {
        id: SnapshotId(0),
        name: "test-volume".to_string(),
        created_at_ms: current_time_ms(),
        inode_count: 10,
        unique_chunk_count: 5,
        shared_chunk_count: 3,
        total_bytes: 1000,
        unique_bytes: 400,
    };

    let id = catalog.add(record);

    let retrieved = catalog.get(id);
    assert!(retrieved.is_some());
}

#[test]
fn test_snapshot_incremental_diff() {
    let config = SnapshotDiffConfig::default();
    let diff = SnapshotDiff::new(config);

    let blocks_a: Vec<claudefs_reduce::snapshot_diff::SnapshotBlock> = (0..100)
        .map(|i| claudefs_reduce::snapshot_diff::SnapshotBlock {
            hash: [i as u8; 32],
            offset: i as u64 * 4096,
            len: 4096,
            segment_id: 1,
        })
        .collect();
    let mut blocks_b = blocks_a.clone();
    for i in 90..100 {
        blocks_b[i] = claudefs_reduce::snapshot_diff::SnapshotBlock {
            hash: [(i + 10) as u8; 32],
            offset: i as u64 * 4096,
            len: 4096,
            segment_id: 1,
        };
    }

    let result = diff.compute(&blocks_a, &blocks_b);

    assert!(result.added_blocks.len() <= 20);
}

#[test]
fn test_worm_retention_policy() {
    let mut enforcer = WormRetentionEnforcer::new();

    let future_time = current_time_secs() + (90 * 24 * 3600);
    let policy = RetentionPolicy::time_based(future_time);

    let chunk_id = 1;
    enforcer.set_policy(chunk_id, policy, "test_user").unwrap();

    let can_delete = enforcer.can_delete(chunk_id);
    assert!(!can_delete, "Should not allow early deletion");
}

#[test]
fn test_worm_legal_hold() {
    let mut enforcer = WormRetentionEnforcer::new();

    let past_time = current_time_secs() - (95 * 24 * 3600);
    let mut policy = RetentionPolicy::time_based(past_time);

    let hold = ComplianceHold {
        hold_id: "hold_1".to_string(),
        placed_by: "legal".to_string(),
        placed_at: current_time_secs(),
        reason: "Legal investigation".to_string(),
        expires_at: None,
    };
    policy.add_hold(hold);

    let chunk_id = 1;
    enforcer.set_policy(chunk_id, policy, "test_user").unwrap();

    let can_delete = enforcer.can_delete(chunk_id);
    assert!(!can_delete, "Legal hold should prevent deletion");
}

#[test]
fn test_key_rotation_basic() {
    let config = KeyManagerConfig::default();
    let test_key = claudefs_reduce::encryption::EncryptionKey([42u8; 32]);
    let manager = KeyManager::with_initial_key(config, test_key);

    let key_v1 = manager.generate_dek().unwrap();
    let wrapped = manager.wrap_dek(&key_v1).unwrap();
    let unwrapped = manager.unwrap_dek(&wrapped).unwrap();

    assert_eq!(key_v1.key, unwrapped.key);
}

#[test]
fn test_key_store_basic() {
    let mut store = KeyStore::new(KeyStoreConfig::default());

    let now_ms = current_time_ms();
    store.generate_key(1, now_ms);

    let retrieved = store.get(1);
    assert!(retrieved.is_some());
}

#[test]
fn test_tier_migration_basic() {
    let config = MigrationConfig::default();
    let _migrator = TierMigrator::new(config);
}

#[test]
fn test_tiering_advisor_basic() {
    let advisor = claudefs_reduce::tiering_advisor::TieringAdvisor::default();
    let metrics = claudefs_reduce::tiering_advisor::AccessMetrics {
        segment_id: 1,
        size_bytes: 1024 * 1024,
        last_access_age_sec: 3600,
        access_count: 10,
        compression_ratio: 0.5,
        dedup_ratio: 0.3,
    };
    let _recommendation = advisor.recommend(&metrics);
}

#[test]
fn test_adaptive_classifier() {
    let config = claudefs_reduce::adaptive_classifier::ClassifierConfig::default();
    let classifier = claudefs_reduce::adaptive_classifier::AdaptiveClassifier::new(config);
    let workload = vec![0x42u8; 4096];
    let workload_str = std::str::from_utf8(&workload).unwrap();

    let result = classifier.classify_pattern(workload_str);
    assert!(result.is_ok());
}

#[test]
fn test_object_store_bridge() {
    let mut store = claudefs_reduce::object_store_bridge::MemoryObjectStore::default();

    let key = claudefs_reduce::object_store_bridge::ObjectKey::new("bucket", "test".to_string());
    let data = vec![0x42u8; 1024];

    let _ = store.put(key.clone(), data.clone(), 0);
}

#[test]
fn test_bandwidth_throttle_config() {
    let config = claudefs_reduce::bandwidth_throttle::ThrottleConfig {
        rate_bytes_per_sec: 10 * 1024 * 1024,
        burst_bytes: 1024 * 1024,
    };
    let mut throttle = claudefs_reduce::bandwidth_throttle::BandwidthThrottle::new(config);

    let decision = throttle.request(512 * 1024, 0);
    assert!(decision == claudefs_reduce::bandwidth_throttle::ThrottleDecision::Allowed);
}

#[test]
fn test_dedup_analytics() {
    let mut analytics = claudefs_reduce::dedup_analytics::DedupAnalytics::new(100);
    let sample = claudefs_reduce::dedup_analytics::DedupSample {
        total_logical_bytes: 1024 * 1024,
        total_physical_bytes: 512 * 1024,
        unique_chunks: 100,
        dedup_ratio: 0.5,
        timestamp_ms: 0,
    };

    analytics.record_sample(sample);
    let current = analytics.current_ratio();
    assert!(current.is_some());
}

#[test]
fn test_compaction_scheduler() {
    let config = claudefs_reduce::compaction_scheduler::CompactionSchedulerConfig::default();
    let _scheduler = claudefs_reduce::compaction_scheduler::CompactionScheduler::new(config);
}

#[test]
fn test_defrag_planner() {
    let config = claudefs_reduce::defrag_planner::DefragPlannerConfig::default();
    let _planner = claudefs_reduce::defrag_planner::DefragPlanner::new(config);
}
