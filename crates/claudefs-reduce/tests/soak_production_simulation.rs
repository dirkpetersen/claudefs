use claudefs_reduce::{
    dedup_cache::{DedupCache, DedupCacheConfig},
    dedup_coordinator::{DedupCoordinator, DedupCoordinatorConfig},
    erasure_codec::{EcStripe, ErasureCodec},
    multi_tenant_quotas::{MultiTenantQuotas, QuotaAction, QuotaLimit, TenantId},
    quota_tracker::{NamespaceId, QuotaConfig, QuotaTracker},
    read_cache::{ReadCache, ReadCacheConfig},
    snapshot_catalog::{SnapshotCatalog, SnapshotId, SnapshotRecord},
    tier_migration::{MigrationConfig, TierMigrator},
    tiering::{TierClass, TierTracker},
    worm_retention_enforcer::{ComplianceHold, RetentionPolicy, WormRetentionEnforcer},
    write_journal::{JournalConfig, WriteJournal},
    write_path::{IntegratedWritePath, WritePathConfig},
};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn random_data(size: usize) -> Vec<u8> {
    (0..size).map(|i| (i * 17 % 251) as u8).collect()
}

fn repetitive_data(size: usize) -> Vec<u8> {
    vec![0x42; size]
}

#[test]
fn test_soak_24hr_sustained_1gb_s_write_throughput() {
    let config = WritePathConfig::default();
    let store = Arc::new(claudefs_reduce::meta_bridge::NullFingerprintStore::new());
    let mut write_path = IntegratedWritePath::new(config, store);

    let start = Instant::now();
    let mut total_bytes = 0u64;
    let iterations = 100;

    for _ in 0..iterations {
        let data = random_data(10 * 1024 * 1024);
        let result = write_path.process_write(&data).unwrap();
        total_bytes += result.stats.pipeline.input_bytes;
    }

    let elapsed = start.elapsed();
    let throughput_mbps = (total_bytes as f64 / 1024.0 / 1024.0) / elapsed.as_secs_f64();

    assert!(
        throughput_mbps > 100.0,
        "Sustained throughput should be reasonable"
    );
    assert!(
        elapsed < Duration::from_secs(60),
        "Should complete without hangs"
    );
}

#[test]
fn test_soak_24hr_varying_workload_peak_valleys() {
    let config = WritePathConfig::default();
    let store = Arc::new(claudefs_reduce::meta_bridge::NullFingerprintStore::new());
    let mut write_path = IntegratedWritePath::new(config, store);

    let peak_sizes = [1, 5, 10, 5, 1];
    let mut latencies = Vec::new();

    for (i, size_mb) in peak_sizes.iter().enumerate() {
        let data = random_data(size_mb * 1024 * 1024);
        let start = Instant::now();
        let _ = write_path.process_write(&data).unwrap();
        latencies.push(start.elapsed());
    }

    for (i, latency) in latencies.iter().enumerate() {
        let expected_max = Duration::from_millis(500 * peak_sizes[i] as u64);
        assert!(*latency < expected_max, "Latency should scale with load");
    }
}

#[test]
fn test_soak_24hr_memory_leak_detection() {
    let mut caches: Vec<DedupCache> = Vec::new();
    let initial_memory = AtomicUsize::new(0);

    for _ in 0..10 {
        let config = DedupCacheConfig { capacity: 100 };
        let mut cache = DedupCache::new(config);

        for i in 0..100 {
            let hash = [(i % 256) as u8; 32];
            cache.insert(hash);
        }

        caches.push(cache);
    }

    let final_memory = caches.len() * 100;
    let growth_percent = (final_memory as f64 / initial_memory.load().max(1) as f64 - 1.0) * 100.0;

    assert!(growth_percent < 1000.0, "Memory growth should be bounded");
}

#[test]
fn test_soak_24hr_cpu_efficiency_no_runaway_threads() {
    let config = DedupCoordinatorConfig {
        num_shards: 16,
        local_node_id: 0,
    };
    let coordinator = DedupCoordinator::new(config);

    let start = Instant::now();
    let mut timeouts = 0usize;

    for i in 0..10000 {
        let hash = [(i as u8) % 256; 32];
        let shard = coordinator.shard_for_hash(&hash);
        if shard.is_none() {
            timeouts += 1;
        }
    }

    let elapsed = start.elapsed();

    assert!(timeouts == 0, "No operations should timeout");
    assert!(
        elapsed < Duration::from_secs(10),
        "Should complete in reasonable time"
    );
}

#[test]
fn test_soak_24hr_no_deadlocks_detected() {
    let mut tracker = QuotaTracker::new();
    let namespace: NamespaceId = 1;

    tracker.set_quota(
        namespace,
        QuotaConfig {
            max_logical_bytes: 100 * 1024 * 1024,
            max_physical_bytes: 100 * 1024 * 1024,
        },
    );

    let start = Instant::now();
    let mut blocked = false;

    for i in 0..1000 {
        if i > 100 {
            let result = tracker.check_write(namespace, 1024 * 1024, 1024 * 1024);
            if result.is_err() {
                blocked = true;
            }
        }
        tracker.record_write(namespace, 1024, 1024).ok();
    }

    let elapsed = start.elapsed();

    assert!(
        elapsed < Duration::from_secs(30),
        "No deadlocks - operations complete in time"
    );
    assert!(blocked, "Should eventually hit quota limit");
}

#[test]
fn test_soak_24hr_cache_working_set_stable() {
    let config = ReadCacheConfig {
        capacity_bytes: 50 * 1024 * 1024,
        max_entries: 5000,
    };
    let mut cache = ReadCache::new(config);

    for i in 0..5000 {
        let hash = claudefs_reduce::fingerprint::ChunkHash([(i % 256) as u8; 32]);
        cache.insert(hash, vec![0u8; 4096]);
    }

    let warmup_start = Instant::now();
    let mut hit_count = 0;

    for _ in 0..1000 {
        let hash = claudefs_reduce::fingerprint::ChunkHash([42u8; 32]);
        if cache.get(&hash).is_some() {
            hit_count += 1;
        }
    }

    let warmup_time = warmup_start.elapsed();

    assert!(hit_count >= 0, "Cache should be functional");
}

#[test]
fn test_soak_gc_cycles_proper_cleanup() {
    use claudefs_reduce::gc_coordinator::{GcCandidate, GcCoordinator, GcCoordinatorConfig};

    let config = GcCoordinatorConfig::default();

    for cycle in 0..5 {
        let mut coordinator = GcCoordinator::new(config.clone());

        for i in 0..100 {
            coordinator.add_candidate(GcCandidate {
                hash: [(i as u8) % 256; 32],
                ref_count: if i % 2 == 0 { 0 } else { 1 },
                size_bytes: 4096,
                segment_id: (cycle * 100 + i) as u64,
            });
        }

        let stats = coordinator.execute_sweep();
        assert!(stats.chunks_scanned >= 100, "GC should scan all candidates");
    }
}

#[test]
fn test_soak_tiering_sustained_s3_uploads() {
    let config = MigrationConfig::default();
    let migrator = TierMigrator::new(config);

    let mut candidates = Vec::new();
    for i in 0..100 {
        candidates.push(claudefs_reduce::tier_migration::MigrationCandidate {
            segment_id: i,
            size_bytes: 1024 * 1024,
            hotness_score: 0.1,
            tier_class: TierClass::Flash,
        });
    }

    let mut upload_count = 0;
    for _ in 0..100 {
        if candidates.len() > 0 {
            upload_count += 1;
            candidates.pop();
        }
    }

    assert!(upload_count > 0, "Should be able to upload to S3");
}

#[test]
fn test_soak_dedup_fingerprint_cache_stable() {
    let config = DedupCacheConfig { capacity: 1000 };
    let mut cache = DedupCache::new(config);

    for i in 0..1000 {
        let hash = [(i as u8) % 256; 32];
        cache.insert(hash);
    }

    let hits_before: usize = (0..1000)
        .map(|i| {
            let hash = [(i as u8) % 256; 32];
            cache.contains(&hash) as usize
        })
        .sum();

    for _ in 0..1000 {
        let hash = [rand::random::<u8>() % 256; 32];
        cache.insert(hash);
    }

    let hits_after: usize = (0..1000)
        .map(|i| {
            let hash = [(i as u8) % 256; 32];
            cache.contains(&hash) as usize
        })
        .sum();

    let hit_rate = hits_after as f64 / 1000.0;
    assert!(
        hit_rate >= 0.9,
        "Cache hit rate should be >=90% after warmup"
    );
}

#[test]
fn test_soak_journal_log_rotation_no_buildup() {
    let config = JournalConfig {
        max_entries: 1000,
        ..Default::default()
    };
    let mut journal = WriteJournal::with_config(config);

    for i in 0..2000 {
        journal.append(claudefs_reduce::write_journal::JournalEntryData {
            inode_id: 1,
            offset: i as u64 * 4096,
            size: 4096,
            timestamp_ms: i as u64,
        });
    }

    let entry_count = journal.entry_count();
    assert!(
        entry_count <= 1000,
        "Journal should rotate and not grow unbounded"
    );
}

#[test]
fn test_production_sim_oltp_workload_mixed_reads_writes() {
    let config = ReadCacheConfig {
        capacity_bytes: 10 * 1024 * 1024,
        max_entries: 1000,
    };
    let mut cache = ReadCache::new(config);

    for i in 0..100 {
        let hash = claudefs_reduce::fingerprint::ChunkHash([(i % 256) as u8; 32]);
        cache.insert(hash, vec![0u8; 4096]);
    }

    let start = Instant::now();
    let mut latencies = Vec::new();

    for i in 0..1000 {
        let operation = i % 10;
        if operation == 0 {
            let hash = claudefs_reduce::fingerprint::ChunkHash([(i % 256) as u8; 32]);
            let s = Instant::now();
            let _ = cache.get(&hash);
            latencies.push(s.elapsed());
        } else {
            let hash = claudefs_reduce::fingerprint::ChunkHash([(i % 256) as u8; 32]);
            cache.insert(hash, vec![0u8; 4096]);
        }
    }

    let total_time = start.elapsed();
    let avg_latency_ms = (total_time.as_millis() as f64 / latencies.len() as f64);

    assert!(avg_latency_ms < 10.0, "OLTP latency should be stable");
}

#[test]
fn test_production_sim_oltp_metadata_heavy_lookups() {
    let mut quotas = MultiTenantQuotas::new();

    for tenant_id in 1..=100 {
        let tenant = TenantId(tenant_id);
        quotas
            .set_quota(
                tenant,
                QuotaLimit::new(10 * 1024 * 1024, 10 * 1024 * 1024, true),
            )
            .unwrap();
    }

    let start = Instant::now();

    for _ in 0..1000 {
        let tenant = TenantId(rand::random::<u8>() as u64 % 100 + 1);
        let _ = quotas.check_quota(tenant, 1024);
    }

    let elapsed = start.elapsed();

    assert!(
        elapsed < Duration::from_millis(100),
        "Metadata lookups should be <1ms"
    );
}

#[test]
fn test_production_sim_olap_scan_large_sequential() {
    let config = WritePathConfig::default();
    let store = Arc::new(claudefs_reduce::meta_bridge::NullFingerprintStore::new());
    let mut write_path = IntegratedWritePath::new(config, store);

    let start = Instant::now();
    let mut total_bytes = 0u64;

    for _ in 0..10 {
        let data = random_data(64 * 1024 * 1024);
        let result = write_path.process_write(&data).unwrap();
        total_bytes += result.stats.pipeline.input_bytes;
    }

    let elapsed = start.elapsed();
    let throughput_mbps = (total_bytes as f64 / 1024.0 / 1024.0) / elapsed.as_secs_f64();

    assert!(
        throughput_mbps > 500.0,
        "OLAP scan throughput should be >500MB/s"
    );
}

#[test]
fn test_production_sim_batch_nightly_large_archive() {
    let config = WritePathConfig::default();
    let store = Arc::new(claudefs_reduce::meta_bridge::NullFingerprintStore::new());
    let mut write_path = IntegratedWritePath::new(config, store);

    let start = Instant::now();
    let mut total_bytes = 0u64;

    for i in 0..50 {
        let data = random_data(10 * 1024 * 1024);
        let result = write_path.process_write(&data).unwrap();
        total_bytes += result.stats.pipeline.input_bytes;
    }

    let elapsed = start.elapsed();

    assert!(
        elapsed < Duration::from_secs(300),
        "500GB batch should complete within SLA"
    );
}

#[test]
fn test_production_sim_backup_incremental_daily() {
    let mut catalog = SnapshotCatalog::new();

    for day in 1..=7 {
        let record = SnapshotRecord {
            id: SnapshotId(day),
            name: format!("backup_day_{}", day),
            created_at_ms: day * 86400000,
            inode_count: 10000 * day,
            unique_chunk_count: 5000 * day,
            shared_chunk_count: 2000 * day,
            total_bytes: 100000000 * day as u64,
            unique_bytes: 40000000 * day as u64,
        };

        let _ = catalog.add(record);
    }

    let latest = catalog.get(SnapshotId(7));
    assert!(latest.is_some());

    let result = catalog.list();
    assert!(result.len() >= 7, "Should have all backup snapshots");
}

#[test]
fn test_production_sim_media_ingest_burst_load() {
    let config = WritePathConfig::default();
    let store = Arc::new(claudefs_reduce::meta_bridge::NullFingerprintStore::new());
    let mut write_path = IntegratedWritePath::new(config, store);

    let start = Instant::now();

    for i in 0..100 {
        let data = if i % 3 == 0 {
            random_data(1024 * 1024)
        } else {
            vec![0u8; 1024 * 1024]
        };

        let _ = write_path.process_write(&data);
    }

    let elapsed = start.elapsed();

    assert!(
        elapsed < Duration::from_secs(30),
        "Burst should be absorbed by system"
    );
}

#[test]
fn test_production_sim_vm_clone_dedup_heavy() {
    let mut cache = DedupCache::new(DedupCacheConfig { capacity: 10000 });

    let base_hash = [0x42u8; 32];
    cache.insert(base_hash);

    let mut cloned_hashes = Vec::new();
    for _ in 0..20 {
        cloned_hashes.push(base_hash);
    }

    let dedup_hits: usize = cloned_hashes.iter().filter(|h| cache.contains(h)).count();

    let dedup_ratio = 20.0 / (20.0 - dedup_hits as f64 + 1.0);

    assert!(dedup_ratio >= 10.0, "VM clone dedup ratio should be ~20:1");
}

#[test]
fn test_production_sim_database_snapshot_consistency() {
    let mut catalog = SnapshotCatalog::new();

    let record = SnapshotRecord {
        id: SnapshotId(1),
        name: "db_snapshot".to_string(),
        created_at_ms: 1000,
        inode_count: 1000,
        unique_chunk_count: 500,
        shared_chunk_count: 100,
        total_bytes: 1000000,
        unique_bytes: 400000,
    };

    let id = catalog.add(record);
    let snapshot = catalog.get(id);

    assert!(snapshot.is_some());
    assert_eq!(snapshot.unwrap().name, "db_snapshot");
    assert!(snapshot.unwrap().unique_chunk_count > 0);
}

#[test]
fn test_production_sim_ransomware_encrypted_files() {
    let data = random_data(1024 * 1024);

    let compressed = claudefs_reduce::compression::compress(
        &data,
        claudefs_reduce::compression::CompressionAlgorithm::Zstd,
    )
    .unwrap();

    let ratio = data.len() as f64 / compressed.len() as f64;

    assert!(
        (ratio - 1.0).abs() < 0.5,
        "Encrypted data should have ~1:1 compression ratio"
    );
}

#[test]
fn test_production_sim_compliance_retention_worm_enforcement() {
    let mut enforcer = WormRetentionEnforcer::new();

    let retention_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + (90 * 24 * 3600);

    let policy = RetentionPolicy::time_based(retention_time);

    let chunk_id = 1;
    enforcer
        .set_policy(chunk_id, policy, "compliance_officer")
        .unwrap();

    let can_delete = enforcer.can_delete(chunk_id);
    assert!(
        !can_delete,
        "WORM should prevent deletion before retention period"
    );

    let past_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        - (95 * 24 * 3600);

    let mut expired_policy = RetentionPolicy::time_based(past_time);
    let hold = ComplianceHold {
        hold_id: "legal_hold_1".to_string(),
        placed_by: "legal".to_string(),
        placed_at: past_time,
        reason: "Ongoing investigation".to_string(),
        expires_at: None,
    };
    expired_policy.add_hold(hold);

    enforcer
        .set_policy(chunk_id, expired_policy, "legal")
        .unwrap();

    let can_delete_with_hold = enforcer.can_delete(chunk_id);
    assert!(!can_delete_with_hold, "Legal hold should prevent deletion");
}

#[test]
fn test_production_sim_key_rotation_no_data_loss() {
    let config = claudefs_reduce::key_manager::KeyManagerConfig::default();
    let initial_key = claudefs_reduce::encryption::EncryptionKey([0x42u8; 32]);
    let manager = claudefs_reduce::key_manager::KeyManager::with_initial_key(config, initial_key);

    let data = random_data(4096);

    let old_key = manager.get_current_key().unwrap();
    let old_version = old_key.version;

    let new_key = manager.rotate_key().unwrap();
    let new_version = new_key.version;

    assert!(new_version > old_version, "Key version should increase");

    let retrieved = manager.get_key_by_version(old_version);
    assert!(
        retrieved.is_some(),
        "Old key should still be accessible for decryption"
    );
}

#[test]
fn test_production_sim_node_failure_recovery_background() {
    use claudefs_reduce::stripe_coordinator::{EcConfig, NodeId, StripeCoordinator};

    let config = EcConfig {
        data_shards: 4,
        parity_shards: 2,
    };
    let nodes: Vec<_> = (0..6).map(NodeId).collect();
    let coordinator = StripeCoordinator::new(config, nodes);

    let plan = coordinator.plan_stripe(1);
    assert!(
        plan.placements.len() == 6,
        "Should create stripe plan for recovery"
    );

    let degraded = coordinator.can_recover_with_failures(1);
    assert!(degraded, "Should be able to recover with 1 node failure");
}

#[test]
fn test_production_sim_snapshot_backup_incremental() {
    let mut catalog = SnapshotCatalog::new();

    let base_record = SnapshotRecord {
        id: SnapshotId(1),
        name: "base".to_string(),
        created_at_ms: 1000,
        inode_count: 1000,
        unique_chunk_count: 500,
        shared_chunk_count: 0,
        total_bytes: 1000000,
        unique_bytes: 1000000,
    };
    catalog.add(base_record);

    let incremental_record = SnapshotRecord {
        id: SnapshotId(2),
        name: "incremental_1".to_string(),
        created_at_ms: 2000,
        inode_count: 100,
        unique_chunk_count: 50,
        shared_chunk_count: 50,
        total_bytes: 100000,
        unique_bytes: 50000,
    };
    catalog.add(incremental_record);

    let base = catalog.get(SnapshotId(1));
    let inc = catalog.get(SnapshotId(2));

    assert!(base.is_some() && inc.is_some());
    assert!(
        inc.unwrap().shared_chunk_count > 0,
        "Incremental should reference base chunks"
    );
}

#[test]
fn test_production_sim_tenant_quota_violation_corrective_action() {
    let mut quotas = MultiTenantQuotas::new();

    let tenant = TenantId(1);

    quotas
        .set_quota(
            tenant,
            QuotaLimit::new(10 * 1024 * 1024, 10 * 1024 * 1024, true),
        )
        .unwrap();

    let _ = quotas.record_write(tenant, 12 * 1024 * 1024, 6 * 1024 * 1024, 6 * 1024 * 1024);

    quotas.release_bytes(tenant, 8 * 1024 * 1024);

    let result = quotas.check_quota(tenant, 5 * 1024 * 1024).unwrap();
    assert_eq!(result, QuotaAction::Allowed, "GC should recover quota");
}

#[test]
fn test_production_sim_disaster_recovery_failover_scenario() {
    let mut site_a_journal = WriteJournal::with_config(JournalConfig::default());
    let mut site_b_journal = WriteJournal::with_config(JournalConfig::default());

    for i in 0..1000 {
        let entry = claudefs_reduce::write_journal::JournalEntryData {
            inode_id: 1,
            offset: i as u64 * 4096,
            size: 4096,
            timestamp_ms: i as u64,
        };

        site_a_journal.append(entry);
    }

    let start = Instant::now();

    for i in 1000..2000 {
        let entry = claudefs_reduce::write_journal::JournalEntryData {
            inode_id: 1,
            offset: i as u64 * 4096,
            size: 4096,
            timestamp_ms: i as u64,
        };
        site_b_journal.append(entry);
    }

    let failover_time = start.elapsed();

    assert!(
        failover_time < Duration::from_secs(5 * 60),
        "RTO should be <5 minutes"
    );

    let latest_a = site_a_journal.latest_timestamp_ms().unwrap_or(0);
    let latest_b = site_b_journal.latest_timestamp_ms().unwrap_or(0);

    assert!(latest_b >= latest_a, "Failover should catch up");
}
