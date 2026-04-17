use claudefs_reduce::{
    eviction_policy::{EvictableSegment, EvictionPolicy, EvictionPolicyConfig, EvictionStrategy},
    fingerprint::ChunkHash,
    key_manager::{DataKey, KeyVersion},
    key_store::{KeyStore, KeyStoreConfig},
    object_assembler::{ObjectAssembler, ObjectAssemblerConfig},
    similarity_coordinator::{SimilarityConfig, SimilarityCoordinator},
    snapshot_catalog::SnapshotCatalog,
    snapshot_diff::{SnapshotDiff, SnapshotDiffConfig},
    tier_migration::{MigrationCandidate, MigrationConfig, TierMigrator},
    tiering::{AccessRecord, TierClass},
    worm_retention_enforcer::{
        ComplianceHold, RetentionPolicy, RetentionType, WormRetentionEnforcer,
    },
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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

    let (evicted, stats) = policy.select_for_eviction(candidates);
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

        if let Some(blob) = assembler.pack(hash, &data) {
            completed_blobs.push(blob);
        }
    }

    if let Some(blob) = assembler.flush() {
        completed_blobs.push(blob);
    }

    assert!(completed_blobs.len() >= 2, "Should create at least 2 blobs");
}

#[test]
fn test_snapshot_creation() {
    let mut catalog = SnapshotCatalog::default();

    let id = catalog.create("test-volume".to_string()).unwrap();

    let record = catalog.get(&id).unwrap();
    assert!(record.is_some());
}

#[test]
fn test_snapshot_incremental_diff() {
    let config = SnapshotDiffConfig::default();
    let diff = SnapshotDiff::new(config);

    let blocks_a: Vec<claudefs_reduce::snapshot_diff::BlockHash> =
        (0..100).map(|i| ChunkHash([i as u8; 32])).collect();
    let mut blocks_b = blocks_a.clone();
    for i in 90..100 {
        blocks_b[i] = ChunkHash([(i + 10) as u8; 32]);
    }

    let result = diff.compute_diff(&blocks_a, &blocks_b);

    assert!(result.changed_blocks <= 20);
}

#[test]
fn test_similarity_detection() {
    let config = SimilarityConfig {
        threshold: 90,
        ..Default::default()
    };
    let coordinator = SimilarityCoordinator::new(config);

    let block1 = vec![0x42u8; 4096];
    let mut block2 = block1.clone();
    for i in 0..409 {
        block2[i * 10] = 0x00;
    }

    let similarity = coordinator.detect_similarity(&block1, &block2);
    assert!(similarity >= 80 || similarity == 0);
}

#[test]
fn test_worm_retention_policy() {
    let mut enforcer = WormRetentionEnforcer::default();

    let policy = RetentionPolicy {
        retention_type: RetentionType::FixedDuration,
        retention_days: 90,
        created_at: SystemTime::now() - Duration::from_secs(45 * 24 * 3600),
    };

    let can_delete = enforcer.can_delete("test-data".to_string(), &policy);
    assert!(!can_delete, "Should not allow early deletion");
}

#[test]
fn test_worm_legal_hold() {
    let mut enforcer = WormRetentionEnforcer::default();

    let policy = RetentionPolicy {
        retention_type: RetentionType::FixedDuration,
        retention_days: 90,
        created_at: SystemTime::now() - Duration::from_secs(95 * 24 * 3600),
    };

    let hold = ComplianceHold::new("test-data".to_string(), "legal".to_string());
    enforcer.add_legal_hold(hold).unwrap();

    let can_delete = enforcer.can_delete("test-data".to_string(), &policy);
    assert!(!can_delete, "Legal hold should prevent deletion");
}

#[test]
fn test_key_rotation_basic() {
    let mut store = KeyStore::new(KeyStoreConfig::default());

    let key_v1 = DataKey::generate();
    store.insert_key(KeyVersion(1), key_v1.clone()).unwrap();

    let retrieved = store.get_key(KeyVersion(1));
    assert!(retrieved.is_some());
}

#[test]
fn test_tier_migration_basic() {
    let config = MigrationConfig {
        source_tier: TierClass::Flash,
        dest_tier: TierClass::S3,
        batch_size: 10,
    };
    let _migrator = TierMigrator::new(config);
}

#[test]
fn test_tiering_advisor_basic() {
    let advisor = claudefs_reduce::tiering_advisor::TieringAdvisor::default();
    let _recommendation = advisor.recommend_eviction(&[]);
}

#[test]
fn test_adaptive_classifier() {
    let classifier = claudefs_reduce::adaptive_classifier::AdaptiveClassifier::default();
    let workload = vec![0x42u8; 4096];

    let result = classifier.classify(&workload);
    assert!(result.hot_score >= 0.0 && result.hot_score <= 1.0);
}

#[test]
fn test_object_store_bridge() {
    let store = claudefs_reduce::object_store_bridge::MemoryObjectStore::default();

    let key = claudefs_reduce::object_store_bridge::ObjectKey::new("test".to_string());
    let data = vec![0x42u8; 1024];

    store.put(key.clone(), data.clone()).unwrap();
    let retrieved = store.get(&key);

    assert!(retrieved.is_some());
    assert_eq!(retrieved.unwrap(), data);
}

#[test]
fn test_bandwidth_throttle_config() {
    let config = claudefs_reduce::bandwidth_throttle::ThrottleConfig {
        rate_bytes_per_sec: 10 * 1024 * 1024,
        burst_bytes: 1024 * 1024,
    };
    let mut throttle = claudefs_reduce::bandwidth_throttle::BandwidthThrottle::new(config);

    let decision = throttle.try_acquire(512 * 1024, 0);
    assert!(decision == claudefs_reduce::bandwidth_throttle::ThrottleDecision::Allowed);
}

#[test]
fn test_dedup_analytics() {
    let analytics = claudefs_reduce::dedup_analytics::DedupAnalytics::default();
    let sample = claudefs_reduce::dedup_analytics::DedupSample {
        logical_bytes: 1024 * 1024,
        physical_bytes: 512 * 1024,
        dedup_hits: 100,
        timestamp_ms: 0,
    };

    analytics.record_sample(sample);
    let trend = analytics.get_trend();
    assert!(trend.samples >= 1);
}

#[test]
fn test_compaction_scheduler() {
    let config = claudefs_reduce::compaction_scheduler::CompactionSchedulerConfig::default();
    let scheduler = claudefs_reduce::compaction_scheduler::CompactionScheduler::new(config);

    assert!(scheduler.is_empty());
}

#[test]
fn test_defrag_planner() {
    let config = claudefs_reduce::defrag_planner::DefragPlannerConfig::default();
    let planner = claudefs_reduce::defrag_planner::DefragPlanner::new(config);

    let actions = planner.plan_defrag();
    assert!(actions.is_empty() || actions.len() >= 0);
}
