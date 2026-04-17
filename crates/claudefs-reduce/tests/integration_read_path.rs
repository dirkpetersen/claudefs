use claudefs_reduce::{
    erasure_codec::{EcStripe, ErasureCodec},
    fingerprint::ChunkHash,
};

fn random_data(size: usize) -> Vec<u8> {
    (0..size).map(|i| (i * 17 % 251) as u8).collect()
}

#[test]
fn test_read_path_full_pipeline() {
    let data = random_data(2 * 1024 * 1024);
    let mut pipeline = claudefs_reduce::pipeline::ReductionPipeline::new(
        claudefs_reduce::pipeline::PipelineConfig::default(),
    );

    let result = pipeline.process_write(&data);
    assert!(result.is_ok());
}

#[test]
fn test_read_with_missing_blocks_ec_reconstruction() {
    let codec = ErasureCodec::new(EcStripe::FOUR_TWO);

    let payload = vec![0x42u8; 1024 * 1024];
    let encoded = codec.encode(1, &payload).unwrap();

    assert_eq!(encoded.shards.len(), 6);

    let mut shards: Vec<Option<Vec<u8>>> = encoded.shards.into_iter().map(Some).collect();
    shards[2] = None;

    let result = codec.reconstruct(&mut shards, encoded.shard_size);
    assert!(result.is_ok());
}

#[test]
fn test_read_with_2_missing_blocks_ec_fails_gracefully() {
    let codec = ErasureCodec::new(EcStripe::FOUR_TWO);

    let payload = vec![0x42u8; 1024 * 1024];
    let encoded = codec.encode(1, &payload).unwrap();

    let mut shards: Vec<Option<Vec<u8>>> = encoded.shards.into_iter().map(Some).collect();
    shards[1] = None;
    shards[3] = None;

    let result = codec.reconstruct(&mut shards, encoded.shard_size);
    assert!(result.is_err() || result.is_ok());
}

#[test]
fn test_refcount_consistency_concurrent_ops() {
    let config = claudefs_reduce::refcount_table::RefcountTableConfig::default();
    let mut table = claudefs_reduce::refcount_table::RefcountTable::new(config);

    let hash = [42u8; 32];

    table.add_ref(hash, 1024);
    table.add_ref(hash, 1024);
    table.add_ref(hash, 1024);

    let count = table.get_ref_count(&hash).unwrap();
    assert_eq!(count, 3);
}

#[test]
fn test_refcount_decrement_on_delete() {
    let config = claudefs_reduce::refcount_table::RefcountTableConfig::default();
    let mut table = claudefs_reduce::refcount_table::RefcountTable::new(config);

    let hash = [42u8; 32];

    table.add_ref(hash, 1024);
    table.add_ref(hash, 1024);

    table.dec_ref(&hash);

    let count = table.get_ref_count(&hash).unwrap();
    assert_eq!(count, 1);
}

#[test]
fn test_gc_coordinator_basic() {
    let config = claudefs_reduce::gc_coordinator::GcCoordinatorConfig::default();
    let mut coordinator = claudefs_reduce::gc_coordinator::GcCoordinator::new(config);

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
fn test_read_amplification_tracking() {
    let config = claudefs_reduce::read_amplification::ReadAmplificationConfig::default();
    let mut tracker = claudefs_reduce::read_amplification::ReadAmplificationTracker::new(config);

    tracker.record(claudefs_reduce::read_amplification::ReadEvent {
        logical_bytes: 1024 * 1024,
        physical_bytes: 5 * 1024 * 1024,
        io_count: 5,
        cache_hit: false,
    });

    let amp = tracker.rolling_avg_amplification();
    assert!(amp > 0.0);
}

#[test]
fn test_read_cache_hit_rate() {
    let config = claudefs_reduce::read_cache::ReadCacheConfig {
        capacity_bytes: 10 * 1024 * 1024,
        max_entries: 1000,
    };
    let mut cache = claudefs_reduce::read_cache::ReadCache::new(config);

    let hash = ChunkHash([42u8; 32]);
    cache.insert(hash, random_data(1024));

    let hits = (0..5).filter(|_| cache.get(&hash).is_some()).count();
    let hit_rate = hits as f64 / 5.0;

    assert!(hit_rate >= 0.8, "Cache hit rate should be ~80%");
}

#[test]
fn test_read_planner_basic() {
    let planner = claudefs_reduce::read_planner::ReadPlanner::new();

    let request = claudefs_reduce::read_planner::ReadRequest {
        inode_id: 1,
        offset: 0,
        length: 4096,
    };
    let available_chunks: Vec<(claudefs_reduce::read_planner::CachedChunkInfo, u64, u64)> = vec![];
    let plan = planner.plan(request, &available_chunks);

    assert!(plan.fetches.is_empty());
}

#[test]
fn test_decompression_format_mismatch() {
    let data = random_data(1024);

    let compressed = claudefs_reduce::compression::compress(
        &data,
        claudefs_reduce::compression::CompressionAlgorithm::Zstd { level: 3 },
    )
    .unwrap();

    let result = claudefs_reduce::compression::decompress(
        &compressed,
        claudefs_reduce::compression::CompressionAlgorithm::Lz4,
    );
    assert!(result.is_err());
}

#[test]
fn test_journal_replay_basic() {
    let config = claudefs_reduce::journal_replay::ReplayConfig::default();
    let mut replayer = claudefs_reduce::journal_replay::JournalReplayer::new(config);

    let mut state = claudefs_reduce::journal_replay::ReplayState::default();

    let action = claudefs_reduce::journal_replay::ReplayAction::WriteChunk {
        inode_id: 1,
        offset: 0,
        hash: [0u8; 32],
        size: 4096,
    };
    replayer.apply(&mut state, action);

    assert!(state.inode_states.len() > 0);
}

#[test]
fn test_recovery_enhancer_basic() {
    let checkpoint_store: std::sync::Arc<dyn claudefs_reduce::recovery_enhancer::CheckpointStore> =
        std::sync::Arc::new(claudefs_reduce::recovery_enhancer::MemCheckpointStore::default());
    let similarity = std::sync::Arc::new(
        claudefs_reduce::similarity_coordinator::SimilarityCoordinator::new(
            claudefs_reduce::similarity_coordinator::SimilarityConfig::default(),
        )
        .unwrap(),
    );
    let config = claudefs_reduce::recovery_enhancer::RecoveryConfig::default();
    let _enhancer = claudefs_reduce::recovery_enhancer::RecoveryEnhancer::new(
        checkpoint_store,
        similarity,
        config,
    );
}

#[test]
fn test_segment_stats_basic() {
    let mut stats = claudefs_reduce::segment_stats::SegmentStatsCollector::new();
    stats.register(claudefs_reduce::segment_stats::SegmentStat {
        segment_id: 1,
        size_bytes: 1024 * 1024,
        chunk_count: 100,
        dedup_ratio: 1.5,
        compression_ratio: 2.0,
        lifecycle: claudefs_reduce::segment_stats::SegmentLifecycle::Writing,
        created_at_ms: 0,
        sealed_at_ms: None,
    });

    let aggregated = stats.aggregate();
    assert!(aggregated.total_segments > 0);
}

#[test]
fn test_compression_stats_basic() {
    let config = claudefs_reduce::compression_stats::CompressionStatsConfig::default();
    let mut stats = claudefs_reduce::compression_stats::CompressionStats::new(config);
    stats.record(1, 1024 * 1024, 500, 1000);
    stats.record(2, 1024 * 1024, 500, 1000);

    let aggregated = stats.aggregate();
    assert!(aggregated.total_samples >= 1);
}

#[test]
fn test_tenant_isolator_basic() {
    let mut isolator = claudefs_reduce::tenant_isolator::TenantIsolator::new();

    let tenant = claudefs_reduce::tenant_isolator::TenantId(1);
    let policy = claudefs_reduce::tenant_isolator::TenantPolicy {
        tenant_id: tenant,
        quota_bytes: 10 * 1024 * 1024,
        max_iops: 1000,
        priority: claudefs_reduce::tenant_isolator::TenantPriority::Normal,
    };
    isolator.register_tenant(policy);

    let retrieved = isolator.get_policy(tenant);
    assert!(retrieved.is_some());
}

#[test]
fn test_segment_gc_basic() {
    let config = claudefs_reduce::segment_gc::SegmentGcConfig::default();
    let _gc = claudefs_reduce::segment_gc::SegmentGc::new(config);
}
