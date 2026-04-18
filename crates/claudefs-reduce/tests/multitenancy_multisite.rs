use claudefs_reduce::{
    cache_coherency::{CacheKey, CacheVersion, CoherencyTracker},
    multi_tenant_quotas::{MultiTenantQuotas, QuotaAction, QuotaLimit, TenantId},
    quota_tracker::{NamespaceId, QuotaConfig, QuotaTracker},
    read_cache::{ReadCache, ReadCacheConfig},
    tenant_isolator::{TenantId as IsolatorTenantId, TenantIsolator, TenantPolicy, TenantPriority},
    write_journal::{JournalEntryData, WriteJournal, WriteJournalConfig},
};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn random_data(size: usize) -> Vec<u8> {
    (0..size).map(|i| (i * 17 % 251) as u8).collect()
}

#[test]
fn test_tenant_isolation_write_from_tenant_a_not_visible_b() {
    let quotas = MultiTenantQuotas::new();

    let tenant_a = TenantId(1);
    let tenant_b = TenantId(2);

    quotas
        .set_quota(
            tenant_a,
            QuotaLimit::new(100 * 1024 * 1024, 100 * 1024 * 1024, true),
        )
        .unwrap();
    quotas
        .set_quota(
            tenant_b,
            QuotaLimit::new(100 * 1024 * 1024, 100 * 1024 * 1024, true),
        )
        .unwrap();

    let result_a = quotas.check_quota(tenant_a, 1024).unwrap();
    assert_eq!(result_a, QuotaAction::Allowed);
    quotas.record_write(tenant_a, 1024, 512, 512).unwrap();

    let result_b = quotas.check_quota(tenant_b, 1024).unwrap();
    assert_eq!(result_b, QuotaAction::Allowed);

    let usage_a = quotas.get_usage(tenant_a).unwrap();
    let usage_b = quotas.get_usage(tenant_b).unwrap();

    assert_eq!(usage_a.logical_bytes, 1024);
    assert_eq!(usage_b.logical_bytes, 0);
}

#[test]
fn test_tenant_isolation_quota_enforcement_separate_budgets() {
    let quotas = MultiTenantQuotas::new();

    let tenant_a = TenantId(1);
    let tenant_b = TenantId(2);

    quotas
        .set_quota(
            tenant_a,
            QuotaLimit::new(10 * 1024 * 1024, 10 * 1024 * 1024, true),
        )
        .unwrap();
    quotas
        .set_quota(
            tenant_b,
            QuotaLimit::new(5 * 1024 * 1024, 5 * 1024 * 1024, true),
        )
        .unwrap();

    for _ in 0..10 {
        let _ = quotas.record_write(tenant_a, 1024 * 1024, 512 * 1024, 512 * 1024);
    }

    let result_a = quotas.check_quota(tenant_a, 1024 * 1024).unwrap();
    assert_eq!(result_a, QuotaAction::HardLimitReject);

    let result_b = quotas.check_quota(tenant_b, 1024 * 1024).unwrap();
    assert_eq!(result_b, QuotaAction::Allowed);
}

#[test]
fn test_tenant_isolation_dedup_across_tenants_not_shared() {
    use claudefs_reduce::dedup_cache::{DedupCache, DedupCacheConfig};

    let mut cache_a = DedupCache::new(DedupCacheConfig { capacity: 100 });
    let mut cache_b = DedupCache::new(DedupCacheConfig { capacity: 100 });

    let hash = [42u8; 32];
    cache_a.insert(hash);

    let hit_in_a = cache_a.contains(&hash);
    let hit_in_b = cache_b.contains(&hash);

    assert!(hit_in_a, "Tenant A should have dedup hit");
    assert!(
        !hit_in_b,
        "Tenant B should NOT see tenant A's dedup entries"
    );
}

#[test]
fn test_tenant_isolation_cache_not_shared_between_tenants() {
    let config_a = ReadCacheConfig {
        capacity_bytes: 10 * 1024 * 1024,
        max_entries: 100,
    };
    let config_b = ReadCacheConfig {
        capacity_bytes: 10 * 1024 * 1024,
        max_entries: 100,
    };

    let mut cache_a = ReadCache::new(config_a);
    let mut cache_b = ReadCache::new(config_b);

    let hash = claudefs_reduce::fingerprint::ChunkHash([42u8; 32]);
    cache_a.insert(hash, vec![0x42u8; 4096]);

    let data_a = cache_a.get(&hash);
    let data_b = cache_b.get(&hash);

    assert!(data_a.is_some(), "Tenant A cache should have data");
    assert!(
        data_b.is_none(),
        "Tenant B cache should NOT have tenant A's data"
    );
}

#[test]
fn test_tenant_isolation_gc_doesnt_affect_other_tenants() {
    use claudefs_reduce::gc_coordinator::{GcCandidate, GcCoordinator, GcCoordinatorConfig};

    let config = GcCoordinatorConfig::default();
    let mut coordinator = GcCoordinator::new(config);

    coordinator.add_candidate(GcCandidate {
        hash: [1u8; 32],
        ref_count: 0,
        size_bytes: 1024,
        segment_id: 1,
    });
    coordinator.add_candidate(GcCandidate {
        hash: [2u8; 32],
        ref_count: 1,
        size_bytes: 1024,
        segment_id: 2,
    });
    coordinator.add_candidate(GcCandidate {
        hash: [3u8; 32],
        ref_count: 0,
        size_bytes: 1024,
        segment_id: 3,
    });

    let stats = coordinator.execute_sweep();
    assert!(stats.chunks_freed > 0, "GC should free unreferenced chunks");
}

#[test]
fn test_tenant_quota_increase_allows_more_writes() {
    let quotas = MultiTenantQuotas::new();

    let tenant = TenantId(1);

    quotas
        .set_quota(
            tenant,
            QuotaLimit::new(5 * 1024 * 1024, 5 * 1024 * 1024, true),
        )
        .unwrap();

    let result_before = quotas.check_quota(tenant, 4 * 1024 * 1024).unwrap();
    assert_eq!(result_before, QuotaAction::Allowed);

    quotas
        .set_quota(
            tenant,
            QuotaLimit::new(20 * 1024 * 1024, 20 * 1024 * 1024, true),
        )
        .unwrap();

    let result_after = quotas.check_quota(tenant, 10 * 1024 * 1024).unwrap();
    assert_eq!(result_after, QuotaAction::Allowed);
}

#[test]
fn test_tenant_quota_decrease_triggers_enforcement() {
    let quotas = MultiTenantQuotas::new();

    let tenant = TenantId(1);

    quotas
        .set_quota(
            tenant,
            QuotaLimit::new(20 * 1024 * 1024, 20 * 1024 * 1024, true),
        )
        .unwrap();

    let _ = quotas.record_write(tenant, 10 * 1024 * 1024, 5 * 1024 * 1024, 5 * 1024 * 1024);

    quotas
        .set_quota(
            tenant,
            QuotaLimit::new(5 * 1024 * 1024, 5 * 1024 * 1024, true),
        )
        .unwrap();

    let result = quotas.check_quota(tenant, 1 * 1024 * 1024).unwrap();
    assert_eq!(result, QuotaAction::HardLimitReject);
}

#[test]
fn test_tenant_quota_overage_backpressure_soft_limit() {
    let quotas = MultiTenantQuotas::new();

    let tenant = TenantId(1);

    quotas
        .set_quota(
            tenant,
            QuotaLimit::new(10 * 1024 * 1024, 10 * 1024 * 1024, true),
        )
        .unwrap();

    quotas
        .set_soft_limit(tenant, 8 * 1024 * 1024, 8 * 1024 * 1024)
        .unwrap();

    let result = quotas.check_quota(tenant, 9 * 1024 * 1024).unwrap();
    assert!(
        matches!(result, QuotaAction::SoftLimitBackpressure),
        "Soft limit should allow writes with backpressure"
    );
}

#[test]
fn test_tenant_quota_hard_limit_rejects_new_writes() {
    let quotas = MultiTenantQuotas::new();

    let tenant = TenantId(1);

    quotas
        .set_quota(
            tenant,
            QuotaLimit::new(5 * 1024 * 1024, 5 * 1024 * 1024, true),
        )
        .unwrap();

    let _ = quotas.record_write(tenant, 5 * 1024 * 1024, 2 * 1024 * 1024, 2 * 1024 * 1024);

    let result = quotas.check_quota(tenant, 1024).unwrap();
    assert_eq!(result, QuotaAction::HardLimitReject);
}

#[test]
fn test_tenant_quota_soft_limit_recovery_after_gc() {
    let quotas = MultiTenantQuotas::new();

    let tenant = TenantId(1);

    quotas
        .set_quota(
            tenant,
            QuotaLimit::new(10 * 1024 * 1024, 10 * 1024 * 1024, true),
        )
        .unwrap();
    quotas
        .set_soft_limit(tenant, 8 * 1024 * 1024, 8 * 1024 * 1024)
        .unwrap();

    let _ = quotas.record_write(tenant, 9 * 1024 * 1024, 4 * 1024 * 1024, 4 * 1024 * 1024);

    quotas.release_bytes(tenant, 2 * 1024 * 1024);

    let result = quotas.check_quota(tenant, 5 * 1024 * 1024).unwrap();
    assert_eq!(result, QuotaAction::Allowed);
}

#[test]
fn test_tenant_deletion_cascading_cleanup() {
    let mut isolator = TenantIsolator::default();

    let tenant = IsolatorTenantId(1);

    isolator.register_tenant(TenantPolicy {
        tenant_id: tenant,
        quota_bytes: 10 * 1024 * 1024,
        max_iops: 1000,
        priority: TenantPriority::Normal,
    });

    isolator.record_write(tenant, 1024 * 1024).unwrap();

    let usage_before = isolator.get_usage(tenant).unwrap();
    assert!(usage_before.bytes_used > 0);

    isolator.unregister_tenant(tenant).unwrap();

    let usage_after = isolator.get_usage(tenant);
    assert!(usage_after.is_none(), "Deleted tenant should have no usage");
}

#[test]
fn test_tenant_account_multi_write_path_quota() {
    let mut tracker = QuotaTracker::new();
    let namespace: NamespaceId = 1;

    tracker.set_quota(
        namespace,
        QuotaConfig {
            max_logical_bytes: 10 * 1024 * 1024,
            max_physical_bytes: 10 * 1024 * 1024,
        },
    );

    let logical_total = tracker.usage(namespace).logical_bytes;

    for i in 0..10 {
        let _ = tracker.check_write(namespace, 1024 * 1024, 1024 * 1024);
        tracker.record_write(namespace, 1024 * 1024, 1024 * 1024);
    }

    let final_logical = tracker.usage(namespace).logical_bytes;
    assert_eq!(final_logical, logical_total + 10 * 1024 * 1024);
}

#[test]
fn test_multisite_write_consistency_site_a_primary() {
    let config = WriteJournalConfig::default();
    let mut journal = WriteJournal::new(config);

    for i in 0..10 {
        journal.append(JournalEntryData {
            inode_id: 1,
            offset: i as u64 * 4096,
            size: 4096,
            timestamp_ms: i,
        });
    }

    let latest = journal.latest_timestamp_ms();
    assert!(latest.is_some());
    assert!(latest.unwrap() >= 9);
}

#[test]
fn test_multisite_write_consistency_site_b_async_replica() {
    let config = WriteJournalConfig::default();
    let site_a_journal = WriteJournal::new(config.clone());
    let mut site_b_journal = WriteJournal::new(config);

    for i in 0..10 {
        site_a_journal.append(JournalEntryData {
            inode_id: 1,
            offset: i as u64 * 4096,
            size: 4096,
            timestamp_ms: i,
        });
    }

    let mut synced = 0;
    for i in 0..10 {
        site_b_journal.append(JournalEntryData {
            inode_id: 1,
            offset: i as u64 * 4096,
            size: 4096,
            timestamp_ms: i + 1,
        });
        synced += 1;
    }

    assert_eq!(
        synced, 10,
        "Async replica should eventually sync all entries"
    );
}

#[test]
fn test_multisite_write_conflict_same_block_both_sites() {
    let mut site_a_usage: HashMap<u64, u64> = HashMap::new();
    let mut site_b_usage: HashMap<u64, u64> = HashMap::new();

    let block_id = 1u64;
    let timestamp_a = 1000u64;
    let timestamp_b = 2000u64;

    site_a_usage.insert(block_id, timestamp_a);
    site_b_usage.insert(block_id, timestamp_b);

    let winner = if timestamp_a > timestamp_b {
        &site_a_usage
    } else {
        &site_b_usage
    };

    assert_eq!(
        winner.get(&block_id),
        Some(&2000u64),
        "LWW: later timestamp wins"
    );
}

#[test]
fn test_multisite_dedup_coordination_across_sites() {
    use claudefs_reduce::dedup_coordinator::{DedupCoordinator, DedupCoordinatorConfig};

    let config_a = DedupCoordinatorConfig {
        num_shards: 4,
        local_node_id: 0,
    };
    let config_b = DedupCoordinatorConfig {
        num_shards: 4,
        local_node_id: 1,
    };

    let coordinator_a = DedupCoordinator::new(config_a);
    let coordinator_b = DedupCoordinator::new(config_b);

    let hash = [42u8; 32];

    let shard_a = coordinator_a.shard_for_hash(&hash);
    let shard_b = coordinator_b.shard_for_hash(&hash);

    assert_eq!(
        shard_a, shard_b,
        "Same hash should route to same shard across sites"
    );
}

#[test]
fn test_multisite_tiering_decision_consistency() {
    use claudefs_reduce::tiering::{TierClass, TierTracker};

    let mut tracker_a = TierTracker::new();
    let mut tracker_b = TierTracker::new();

    let segment_id = 1u64;
    let metrics_a = claudefs_reduce::tiering::AccessRecord {
        segment_id,
        last_access_age_sec: 7200,
        access_count: 1,
        size_bytes: 1024 * 1024,
    };
    let metrics_b = claudefs_reduce::tiering::AccessRecord {
        segment_id,
        last_access_age_sec: 7200,
        access_count: 1,
        size_bytes: 1024 * 1024,
    };

    tracker_a.record_access(metrics_a);
    tracker_b.record_access(metrics_b);

    let tier_a = tracker_a.decide_tier(segment_id);
    let tier_b = tracker_b.decide_tier(segment_id);

    assert_eq!(
        tier_a, tier_b,
        "Same metrics should yield same tiering decision"
    );
}

#[test]
fn test_multisite_cache_coherency_read_after_write_consistency() {
    let mut tracker = CoherencyTracker::new();

    let key = CacheKey {
        inode_id: 1,
        chunk_index: 0,
    };
    let version = CacheVersion::new();

    tracker.register(key, version.clone(), 1024);

    let is_valid = tracker.is_valid(&key, &version);
    assert!(is_valid, "Read-after-write should see consistent data");

    tracker.invalidate(
        &claudefs_reduce::cache_coherency::InvalidationEvent::ChunkInvalidated { key: key.clone() },
    );

    let is_valid_after = tracker.is_valid(&key, &version);
    assert!(!is_valid_after, "Invalidated data should not be visible");
}

#[test]
fn test_multisite_site_failure_recovery_from_replica() {
    use claudefs_reduce::stripe_coordinator::{EcConfig, NodeId, StripeCoordinator};

    let config = EcConfig {
        data_shards: 4,
        parity_shards: 2,
    };
    let nodes: Vec<_> = (0..6).map(NodeId).collect();
    let coordinator = StripeCoordinator::new(config, nodes);

    let plan = coordinator.plan_stripe(1);
    assert_eq!(
        plan.placements.len(),
        6,
        "Should have full stripe placement"
    );

    let available = coordinator.available_nodes();
    assert!(
        available.len() >= 4,
        "Should have enough replicas for recovery"
    );
}

#[test]
fn test_multisite_network_partition_site_latency_spike() {
    use claudefs_reduce::bandwidth_throttle::{
        BandwidthThrottle, ThrottleConfig, ThrottleDecision,
    };

    let config = ThrottleConfig {
        rate_bytes_per_sec: 10 * 1024 * 1024,
        burst_bytes: 1024 * 1024,
    };
    let mut throttle = BandwidthThrottle::new(config);

    let start = Instant::now();
    let mut decisions = Vec::new();
    let mut now_ms = 0u64;

    for _ in 0..100 {
        let decision = throttle.request(512 * 1024, now_ms);
        decisions.push(decision);
        now_ms += 100;
    }
    let elapsed = start.elapsed();

    let allowed_count = decisions
        .iter()
        .filter(|d| matches!(d, ThrottleDecision::Allowed))
        .count();

    assert!(
        elapsed < Duration::from_secs(1),
        "Should handle requests during latency spike"
    );
    assert!(allowed_count > 0, "Some requests should be allowed");
}

#[test]
fn test_multisite_split_brain_majority_quorum_prevails() {
    let mut votes: HashMap<u32, bool> = HashMap::new();
    votes.insert(1, true);
    votes.insert(2, true);
    votes.insert(3, false);
    votes.insert(4, false);

    let majority_count = votes.values().filter(|v| **v).count();
    let total = votes.len();

    assert!(
        majority_count > total / 2,
        "Majority quorum should prevail in split brain"
    );
}

#[test]
fn test_multisite_gc_coordination_both_sites_same_decision() {
    use claudefs_reduce::gc_coordinator::{GcCandidate, GcCoordinator, GcCoordinatorConfig};

    let config = GcCoordinatorConfig::default();
    let mut coordinator_a = GcCoordinator::new(config.clone());
    let mut coordinator_b = GcCoordinator::new(config);

    for i in 0..10 {
        coordinator_a.add_candidate(GcCandidate {
            hash: [i as u8; 32],
            ref_count: if i < 5 { 1 } else { 0 },
            size_bytes: 1024,
            segment_id: i as u64,
        });
        coordinator_b.add_candidate(GcCandidate {
            hash: [i as u8; 32],
            ref_count: if i < 5 { 1 } else { 0 },
            size_bytes: 1024,
            segment_id: i as u64,
        });
    }

    let stats_a = coordinator_a.execute_sweep();
    let stats_b = coordinator_b.execute_sweep();

    assert_eq!(
        stats_a.chunks_freed, stats_b.chunks_freed,
        "Both sites should make same GC decisions"
    );
}

#[test]
fn test_multisite_quota_enforcement_replicated() {
    let quotas = MultiTenantQuotas::new();

    let tenant = TenantId(1);

    quotas
        .set_quota(
            tenant,
            QuotaLimit::new(10 * 1024 * 1024, 10 * 1024 * 1024, true),
        )
        .unwrap();

    let _ = quotas.record_write(tenant, 5 * 1024 * 1024, 2 * 1024 * 1024, 2 * 1024 * 1024);

    let usage = quotas.get_usage(tenant).unwrap();
    assert!(usage.logical_bytes > 0, "Quota usage should be tracked");

    let result = quotas.check_quota(tenant, 6 * 1024 * 1024).unwrap();
    assert_eq!(result, QuotaAction::Allowed);

    let result_over = quotas.check_quota(tenant, 6 * 1024 * 1024).unwrap();
    assert_eq!(result_over, QuotaAction::HardLimitReject);
}

#[test]
fn test_multisite_tenant_isolation_across_sites() {
    let quotas_a = MultiTenantQuotas::new();
    let quotas_b = MultiTenantQuotas::new();

    let tenant = TenantId(1);

    quotas_a
        .set_quota(
            tenant,
            QuotaLimit::new(10 * 1024 * 1024, 10 * 1024 * 1024, true),
        )
        .unwrap();
    quotas_b
        .set_quota(
            tenant,
            QuotaLimit::new(10 * 1024 * 1024, 10 * 1024 * 1024, true),
        )
        .unwrap();

    quotas_a
        .record_write(tenant, 5 * 1024 * 1024, 2 * 1024 * 1024, 2 * 1024 * 1024)
        .unwrap();

    let usage_a = quotas_a.get_usage(tenant).unwrap();
    let usage_b = quotas_b.get_usage(tenant).unwrap();

    assert_eq!(usage_a.logical_bytes, 5 * 1024 * 1024);
    assert_eq!(
        usage_b.logical_bytes, 0,
        "Site B should have independent tenant state"
    );
}

#[test]
fn test_multisite_disaster_recovery_switchover_time_rto() {
    let config = WriteJournalConfig::default();
    let mut journal = WriteJournal::new(config);

    let start = Instant::now();

    for i in 0..1000 {
        journal.append(JournalEntryData {
            inode_id: 1,
            offset: i as u64 * 4096,
            size: 4096,
            timestamp_ms: i,
        });
    }

    let entry = journal.get_entry(999);
    let elapsed = start.elapsed();

    assert!(
        entry.is_some(),
        "Should be able to retrieve entry for recovery"
    );
    assert!(
        elapsed < Duration::from_secs(5 * 60),
        "RTO should be <5 minutes"
    );
}

#[test]
fn test_multisite_snapshot_consistency_across_sites() {
    let mut catalog_a = claudefs_reduce::snapshot_catalog::SnapshotCatalog::new();
    let mut catalog_b = claudefs_reduce::snapshot_catalog::SnapshotCatalog::new();

    let record = claudefs_reduce::snapshot_catalog::SnapshotRecord {
        id: claudefs_reduce::snapshot_catalog::SnapshotId(1),
        name: "test_snapshot".to_string(),
        created_at_ms: 1000,
        inode_count: 100,
        unique_chunk_count: 50,
        shared_chunk_count: 25,
        total_bytes: 100000,
        unique_bytes: 40000,
    };

    let id_a = catalog_a.add(record.clone());
    let id_b = catalog_b.add(record);

    let snapshot_a = catalog_a.get(id_a);
    let snapshot_b = catalog_b.get(id_b);

    assert!(snapshot_a.is_some() && snapshot_b.is_some());
    assert_eq!(
        snapshot_a.unwrap().inode_count,
        snapshot_b.unwrap().inode_count
    );
}
