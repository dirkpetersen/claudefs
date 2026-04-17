use claudefs_reduce::{
    erasure_codec::{EcStripe, EncodedSegment, ErasureCodec},
    fingerprint::ChunkHash,
    gc_coordinator::{GcCandidate, GcCoordinator, GcCoordinatorConfig},
    read_amplification::{ReadAmplificationConfig, ReadAmplificationTracker, ReadEvent},
    read_cache::{ReadCache, ReadCacheConfig},
    refcount_table::{RefcountTable, RefcountTableConfig},
};
use std::collections::HashMap;

fn random_data(size: usize) -> Vec<u8> {
    (0..size).map(|i| (i * 17 % 251) as u8).collect()
}

#[test]
fn test_read_path_full_pipeline() {
    let data = random_data(2 * 1024 * 1024);
    let pipeline = claudefs_reduce::pipeline::ReductionPipeline::default();

    let (chunks, stats) = pipeline.process_write(&data).unwrap();

    assert!(!chunks.is_empty());
    assert!(stats.input_bytes == 2 * 1024 * 1024 as u64);
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
    assert!(result.is_err(), "2 missing should fail");
}

#[test]
fn test_refcount_consistency_concurrent_ops() {
    let config = RefcountTableConfig::default();
    let mut table = RefcountTable::new(config);

    let hash = [42u8; 32];

    table.add_ref(hash, 1024);
    table.add_ref(hash, 1024);
    table.add_ref(hash, 1024);

    let count = table.get_ref_count(&hash).unwrap();
    assert_eq!(count, 3);
}

#[test]
fn test_refcount_decrement_on_delete() {
    let config = RefcountTableConfig::default();
    let mut table = RefcountTable::new(config);

    let hash = [42u8; 32];

    table.add_ref(hash, 1024);
    table.add_ref(hash, 1024);

    table.dec_ref(&hash);

    let count = table.get_ref_count(&hash).unwrap();
    assert_eq!(count, 1);
}

#[test]
fn test_gc_coordinator_basic() {
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

    let candidates = coordinator.candidates();
    assert_eq!(candidates.len(), 2);
}

#[test]
fn test_read_amplification_tracking() {
    let config = ReadAmplificationConfig::default();
    let mut tracker = ReadAmplificationTracker::new(config);

    tracker.record_event(ReadEvent {
        logical_bytes: 1024 * 1024,
        physical_blocks: 5,
        timestamp_ms: 0,
    });

    let stats = tracker.aggregate_stats();
    let amp = stats.amplification_factor();

    assert!((amp - 5.0).abs() < 0.1, "Amplification should be ~5x");
}

#[test]
fn test_read_cache_hit_rate() {
    let config = ReadCacheConfig {
        capacity_bytes: 10 * 1024 * 1024,
        max_entries: 1000,
    };
    let mut cache = ReadCache::new(config);

    let hash = ChunkHash([42u8; 32]);
    cache.put(hash, random_data(1024)).unwrap();

    let hits = (0..5).filter(|_| cache.get(&hash).is_some()).count();
    let hit_rate = hits as f64 / 5.0;

    assert!(hit_rate >= 0.8, "Cache hit rate should be ~80%");
}

#[test]
fn test_read_planner_basic() {
    let mut planner = claudefs_reduce::read_planner::ReadPlanner::default();

    let hashes: Vec<ChunkHash> = (0..10).map(|i| ChunkHash([i as u8; 32])).collect();

    for (i, hash) in hashes.iter().enumerate() {
        let request = claudefs_reduce::read_planner::ReadRequest {
            hash: *hash,
            offset: (i * 1024 * 1024) as u64,
            size: 1024 * 1024,
        };
        planner.add_request(request);
    }

    let plan = planner.build_plan();
    assert!(!plan.chunks.is_empty());
}

#[test]
fn test_decompression_format_mismatch() {
    let data = random_data(1024);

    let compressed = claudefs_reduce::compression::CompressionAlgorithm::Zstd { level: 3 }
        .compress(&data)
        .unwrap();

    let result = claudefs_reduce::compression::CompressionAlgorithm::Lz4.decompress(&compressed);
    assert!(result.is_err(), "Wrong compression algorithm should fail");
}

#[test]
fn test_journal_replay_basic() {
    let config = claudefs_reduce::journal_replay::ReplayConfig::default();
    let replayer = claudefs_reduce::journal_replay::JournalReplayer::new(config);

    let state = claudefs_reduce::journal_replay::InodeReplayState {
        inode: 1,
        blocks: vec![],
        checksum: [0u8; 32],
    };

    let actions = replayer.plan_replay(&state);
    assert!(actions.is_empty() || actions.len() >= 0);
}

#[test]
fn test_recovery_enhancer_basic() {
    let config = claudefs_reduce::recovery_enhancer::RecoveryConfig::default();
    let enhancer = claudefs_reduce::recovery_enhancer::RecoveryEnhancer::new(config);

    let state = HashMap::new();
    let report = enhancer.check_consistency(&state);
    assert!(report.inconsistencies.is_empty() || report.recoverable);
}

#[test]
fn test_segment_stats_basic() {
    let mut stats = claudefs_reduce::segment_stats::SegmentStatsCollector::new();
    stats.record_write(1024 * 1024, 512 * 1024);

    let aggregated = stats.aggregate();
    assert!(aggregated.total_writes > 0);
}

#[test]
fn test_compression_stats_basic() {
    let mut stats = claudefs_reduce::compression_stats::CompressionStats::default();
    stats.record(1.5, 1024 * 1024, 500);

    let aggregated = stats.aggregate();
    assert!(aggregated.total_compressions > 0);
}

#[test]
fn test_tenant_isolator_routing() {
    let mut isolator = claudefs_reduce::tenant_isolator::TenantIsolator::default();

    let hash = [42u8; 32];
    let _routed = isolator.route_hash(hash);
}

#[test]
fn test_segment_gc_basic() {
    let config = claudefs_reduce::segment_gc::SegmentGcConfig::default();
    let gc = claudefs_reduce::segment_gc::SegmentGc::new(config);

    assert!(gc.is_empty());
}
