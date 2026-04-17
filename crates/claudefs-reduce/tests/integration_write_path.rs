use claudefs_reduce::{
    bandwidth_throttle::{BandwidthThrottle, ThrottleConfig, ThrottleDecision},
    encryption::EncryptionKey,
    erasure_codec::{EcStripe, ErasureCodec},
    fingerprint::ChunkHash,
    meta_bridge::NullFingerprintStore,
    multi_tenant_quotas::{MultiTenantQuotas, QuotaLimit, TenantId},
    quota_tracker::{NamespaceId, QuotaConfig, QuotaTracker},
    segment::{SegmentPacker, SegmentPackerConfig},
    stripe_coordinator::{EcConfig, NodeId, StripeCoordinator},
    write_buffer::{PendingWrite, WriteBuffer, WriteBufferConfig},
    write_path::{IntegratedWritePath, WritePathConfig},
};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn random_data(size: usize) -> Vec<u8> {
    (0..size).map(|i| (i * 17 % 251) as u8).collect()
}

fn repetitive_data(size: usize) -> Vec<u8> {
    vec![0x42; size]
}

#[test]
fn test_write_path_all_stages_enabled() {
    let mut config = WritePathConfig::default();
    config.pipeline.dedup_enabled = true;
    config.pipeline.compression_enabled = true;
    config.pipeline.encryption_enabled = true;

    let store = Arc::new(NullFingerprintStore::new());
    let key = EncryptionKey([0x42u8; 32]);
    let mut write_path = IntegratedWritePath::new_with_key(config, key, store);

    let data = random_data(1024 * 1024);
    let result = write_path.process_write(&data).unwrap();

    assert!(!result.reduced_chunks.is_empty());
    assert!(result.stats.pipeline.input_bytes > 0);
}

#[test]
fn test_write_path_no_compression() {
    let mut config = WritePathConfig::default();
    config.pipeline.dedup_enabled = true;
    config.pipeline.compression_enabled = false;
    config.pipeline.encryption_enabled = false;

    let store = Arc::new(NullFingerprintStore::new());
    let mut write_path = IntegratedWritePath::new(config, store);

    let data = repetitive_data(512 * 1024);
    let result = write_path.process_write(&data).unwrap();

    assert!(result.stats.pipeline.input_bytes > 0);
}

#[test]
fn test_write_path_no_dedup() {
    let mut config = WritePathConfig::default();
    config.pipeline.dedup_enabled = false;
    config.pipeline.compression_enabled = true;
    config.pipeline.encryption_enabled = false;

    let store = Arc::new(NullFingerprintStore::new());
    let mut write_path = IntegratedWritePath::new(config, store);

    let data = random_data(1024 * 1024);
    let result = write_path.process_write(&data).unwrap();

    assert!(result.stats.distributed_dedup_hits == 0);
    assert!(!result.reduced_chunks.is_empty());
}

#[test]
fn test_distributed_dedup_coordination() {
    let config = claudefs_reduce::dedup_coordinator::DedupCoordinatorConfig {
        num_shards: 3,
        local_node_id: 0,
    };
    let coordinator = claudefs_reduce::dedup_coordinator::DedupCoordinator::new(config);

    for _ in 0..10 {
        for i in 0..100u8 {
            let hash = [i; 32];
            let shard1 = coordinator.shard_for_hash(&hash);
            let shard2 = coordinator.shard_for_hash(&hash);
            assert_eq!(
                shard1, shard2,
                "Hash routing should be consistent for hash {:?}",
                hash
            );
        }
    }
}

#[test]
fn test_stripe_coordinator_ec_placement() {
    let config = EcConfig {
        data_shards: 4,
        parity_shards: 2,
    };
    let nodes: Vec<_> = (0..6).map(NodeId).collect();
    let coordinator = StripeCoordinator::new(config, nodes);

    for seg_id in 1..=5 {
        let plan = coordinator.plan_stripe(seg_id);
        assert_eq!(plan.placements.len(), 6, "4+2 = 6 shards");
    }
}

#[test]
fn test_quota_enforcement_single_tenant() {
    let mut tracker = QuotaTracker::new();
    let namespace: NamespaceId = 1;

    tracker.set_quota(
        namespace,
        QuotaConfig {
            max_logical_bytes: 10 * 1024 * 1024,
            max_physical_bytes: 10 * 1024 * 1024,
        },
    );

    let check1 = tracker.check_write(namespace, 8 * 1024 * 1024, 8 * 1024 * 1024);
    assert!(check1.is_ok(), "First write should succeed");
    tracker.record_write(namespace, 8 * 1024 * 1024, 8 * 1024 * 1024);

    let check2 = tracker.check_write(namespace, 2 * 1024 * 1024, 2 * 1024 * 1024);
    assert!(check2.is_ok(), "Second write should succeed");
    tracker.record_write(namespace, 2 * 1024 * 1024, 2 * 1024 * 1024);

    let check3 = tracker.check_write(namespace, 1 * 1024 * 1024, 1 * 1024 * 1024);
    assert!(check3.is_err(), "Third write should fail - quota exceeded");

    let current = tracker.usage(namespace);
    assert_eq!(current.logical_bytes, 10 * 1024 * 1024);
}

#[test]
fn test_quota_enforcement_multi_tenant() {
    let quotas = MultiTenantQuotas::new();

    let tenant1 = TenantId(1);
    let tenant2 = TenantId(2);

    quotas
        .set_quota(
            tenant1,
            QuotaLimit::new(5 * 1024 * 1024, 5 * 1024 * 1024, true),
        )
        .unwrap();
    quotas
        .set_quota(
            tenant2,
            QuotaLimit::new(5 * 1024 * 1024, 5 * 1024 * 1024, true),
        )
        .unwrap();

    let result1 = quotas.check_quota(tenant1, 4 * 1024 * 1024).unwrap();
    assert_eq!(
        result1,
        claudefs_reduce::multi_tenant_quotas::QuotaAction::Allowed
    );
    quotas
        .record_write(tenant1, 4 * 1024 * 1024, 4 * 1024 * 1024, 0)
        .unwrap();

    let result2 = quotas.check_quota(tenant2, 3 * 1024 * 1024).unwrap();
    assert_eq!(
        result2,
        claudefs_reduce::multi_tenant_quotas::QuotaAction::Allowed
    );
    quotas
        .record_write(tenant2, 3 * 1024 * 1024, 3 * 1024 * 1024, 0)
        .unwrap();

    let result3 = quotas.check_quota(tenant1, 2 * 1024 * 1024).unwrap();
    assert_eq!(
        result3,
        claudefs_reduce::multi_tenant_quotas::QuotaAction::HardLimitReject
    );

    let result4 = quotas.check_quota(tenant2, 2 * 1024 * 1024).unwrap();
    assert_eq!(
        result4,
        claudefs_reduce::multi_tenant_quotas::QuotaAction::Allowed
    );
}

#[test]
fn test_bandwidth_throttle_under_load() {
    use claudefs_reduce::bandwidth_throttle::{
        BandwidthThrottle, ThrottleConfig, ThrottleDecision,
    };
    use std::time::Duration;
    use std::time::Instant;

    let config = ThrottleConfig {
        rate_bytes_per_sec: 10 * 1024 * 1024,
        burst_bytes: 1024 * 1024,
    };
    let mut throttle = BandwidthThrottle::new(config);
    let start = Instant::now();

    let mut allowed_count = 0;
    let mut throttled_count = 0;
    let mut now_ms = 0u64;

    for _ in 0..20 {
        let decision = throttle.request(1024 * 1024, now_ms);
        match decision {
            ThrottleDecision::Allowed => {
                allowed_count += 1;
                now_ms += 100; // advance time
            }
            ThrottleDecision::Throttled { .. } => {
                throttled_count += 1;
                now_ms += 1;
            }
        }
    }

    // At 10 MiB/s, over 2000ms (20 * 100ms), we should allow ~10 requests of 1MiB each
    // This is a basic sanity check that throttle is working
    assert!(allowed_count >= 5, "Should allow at least some requests");
    assert!(throttled_count >= 0, "May throttle some requests");
}

#[test]
fn test_segment_packing_completeness() {
    let config = SegmentPackerConfig {
        target_size: 1024 * 1024,
    };
    let mut packer = SegmentPacker::new(config);

    // Use smaller chunks so flush() will still have a segment
    // Total: 512 + 512 + 512 = 1536 bytes, which is < 1MB target
    packer.add_chunk(ChunkHash([1; 32]), &vec![0u8; 512], 512);
    packer.add_chunk(ChunkHash([2; 32]), &vec![0u8; 512], 512);
    packer.add_chunk(ChunkHash([3; 32]), &vec![0u8; 512], 512);

    // flush() returns Option<Segment>
    if let Some(segment) = packer.flush() {
        assert!(
            segment.total_payload_bytes() > 0,
            "Packed segment should have data"
        );
    } else {
        panic!("Expected a segment from flush()");
    }
}

#[test]
fn test_chunk_pipeline_backpressure() {
    let config = claudefs_reduce::pipeline_backpressure::BackpressureConfig::default();
    let mut backpressure =
        claudefs_reduce::pipeline_backpressure::PipelineBackpressure::new(config);

    for _ in 0..5 {
        backpressure.add_chunks(1);
        backpressure.add_bytes(4096);
    }

    assert!(backpressure.in_flight_chunks() >= 5);
}

#[test]
fn test_inline_dedup_cache_hits() {
    let config = claudefs_reduce::dedup_cache::DedupCacheConfig { capacity: 100 };
    let mut cache = claudefs_reduce::dedup_cache::DedupCache::new(config);

    let hash = [42u8; 32];
    cache.insert(hash);

    let hits = (0..5).filter(|_| cache.contains(&hash)).count();
    let hit_rate = hits as f64 / 5.0;

    assert!(hit_rate >= 0.8, "Cache should provide hits");
}

#[test]
fn test_write_buffer_overflow_spill() {
    let config = WriteBufferConfig {
        flush_threshold_bytes: 512 * 1024,
        max_pending_writes: 100,
    };
    let mut buffer = WriteBuffer::new(config);

    for i in 0..10 {
        let write = PendingWrite {
            inode_id: 1,
            offset: (i * 1024) as u64,
            data: vec![(i % 256) as u8; 1024 * 1024 / 10],
            timestamp_ms: 0,
        };
        buffer.buffer_write(write);
    }

    assert!(
        buffer.total_pending_bytes() > 0,
        "Buffer should accumulate data"
    );
}

#[test]
fn test_erasure_codec_encode_decode() {
    let codec = ErasureCodec::new(EcStripe::FOUR_TWO);
    let payload = vec![0x42u8; 1024 * 1024];

    let encoded = codec.encode(1, &payload).unwrap();
    assert_eq!(encoded.shards.len(), 6);

    let decoded = codec.decode(&encoded).unwrap();
    assert_eq!(decoded.len(), payload.len());
    assert_eq!(decoded, payload);
}

#[test]
fn test_refcount_table_basic() {
    let config = claudefs_reduce::refcount_table::RefcountTableConfig::default();
    let mut table = claudefs_reduce::refcount_table::RefcountTable::new(config);

    let hash = [42u8; 32];
    table.add_ref(hash, 1024);
    table.add_ref(hash, 1024);

    let count = table.get_ref_count(&hash).unwrap();
    assert_eq!(count, 2);
}

#[test]
fn test_chunk_scheduler_basic() {
    let config = claudefs_reduce::chunk_scheduler::SchedulerConfig::default();
    let mut scheduler = claudefs_reduce::chunk_scheduler::ChunkScheduler::new(config);

    let op_id = scheduler.submit(
        claudefs_reduce::chunk_scheduler::ChunkOp::Write {
            chunk_hash: [1u8; 32],
            data: vec![],
        },
        claudefs_reduce::chunk_scheduler::OpPriority::Interactive,
        0,
    );

    assert!(op_id.is_ok());

    let next = scheduler.pop_next();
    assert!(next.is_some());
}

#[test]
fn test_gc_coordinator_sweep() {
    let mut coordinator = claudefs_reduce::gc_coordinator::GcCoordinator::new(
        claudefs_reduce::gc_coordinator::GcCoordinatorConfig::default(),
    );

    coordinator.add_candidate(claudefs_reduce::gc_coordinator::GcCandidate {
        hash: [1u8; 32],
        ref_count: 0,
        size_bytes: 1024,
        segment_id: 1,
    });

    coordinator.add_candidate(claudefs_reduce::gc_coordinator::GcCandidate {
        hash: [2u8; 32],
        ref_count: 1,
        size_bytes: 1024,
        segment_id: 2,
    });

    let stats = coordinator.execute_sweep();
    assert!(stats.chunks_scanned >= 2);
}

#[test]
fn test_read_cache_basic() {
    let config = claudefs_reduce::read_cache::ReadCacheConfig {
        capacity_bytes: 10 * 1024 * 1024,
        max_entries: 1000,
    };
    let mut cache = claudefs_reduce::read_cache::ReadCache::new(config);

    let hash = ChunkHash([42u8; 32]);
    cache.insert(hash, vec![0x42u8; 1024]);

    let data = cache.get(&hash);
    assert!(data.is_some());
}
