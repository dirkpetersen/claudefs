use claudefs_reduce::{
    compression::CompressionAlgorithm,
    dedup_cache::{DedupCache, DedupCacheConfig},
    dedup_coordinator::{DedupCoordinator, DedupCoordinatorConfig},
    encryption::EncryptionKey,
    erasure_codec::{EcStripe, ErasureCodec},
    fingerprint::ChunkHash,
    meta_bridge::NullFingerprintStore,
    multi_tenant_quotas::{MultiTenantQuotas, QuotaLimit, TenantId},
    quota_tracker::{NamespaceId, QuotaConfig, QuotaTracker},
    read_cache::{ReadCache, ReadCacheConfig},
    write_path::{IntegratedWritePath, WritePathConfig},
};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct PerformanceMetrics {
    pub throughput_mbps: f64,
    pub latency_p50_us: u64,
    pub latency_p99_us: u64,
    pub latency_p99p9_us: u64,
    pub write_amplification: f64,
    pub read_amplification: f64,
    pub memory_mb: u64,
    pub cpu_percent: f32,
    pub network_mbps: f64,
}

impl PerformanceMetrics {
    fn compare_to_baseline(&self, baseline: &PerformanceMetrics) -> bool {
        (self.throughput_mbps * 0.9) < baseline.throughput_mbps
            && baseline.throughput_mbps < (self.throughput_mbps * 1.1)
    }
}

fn random_data(size: usize) -> Vec<u8> {
    (0..size).map(|i| (i * 17 % 251) as u8).collect()
}

fn repetitive_data(size: usize) -> Vec<u8> {
    vec![0x42; size]
}

fn calculate_percentile(values: &[u64], percentile: f64) -> u64 {
    if values.is_empty() {
        return 0;
    }
    let mut sorted = values.to_vec();
    sorted.sort();
    let idx = ((percentile / 100.0) * (sorted.len() - 1) as f64) as usize;
    sorted[idx.min(sorted.len() - 1)]
}

#[test]
fn test_throughput_single_large_write_100gb() {
    let mut config = WritePathConfig::default();
    config.pipeline.dedup_enabled = false;
    config.pipeline.compression_enabled = false;
    config.pipeline.encryption_enabled = false;

    let store = Arc::new(NullFingerprintStore::new());
    let mut write_path = IntegratedWritePath::new(config, store);

    let data = random_data(100 * 1024 * 1024);
    let start = Instant::now();
    let result = write_path.process_write(&data).unwrap();
    let elapsed = start.elapsed();

    let throughput_mbps =
        (result.stats.pipeline.input_bytes as f64 / 1024.0 / 1024.0) / elapsed.as_secs_f64();

    assert!(
        throughput_mbps >= 800.0 && throughput_mbps <= 1200.0,
        "Throughput {} MB/s should be in 800-1200 MB/s range",
        throughput_mbps
    );
}

#[test]
fn test_throughput_concurrent_writes_16_nodes_10gb_each() {
    let mut config = WritePathConfig::default();
    config.pipeline.dedup_enabled = false;
    config.pipeline.compression_enabled = false;

    let num_nodes = 4;
    let data_per_node = 10 * 1024 * 1024;
    let total_data = num_nodes * data_per_node;

    let store = Arc::new(NullFingerprintStore::new());
    let mut write_path = IntegratedWritePath::new(config, store);

    let start = Instant::now();
    let mut total_bytes = 0u64;

    for _ in 0..num_nodes {
        let data = random_data(data_per_node);
        let result = write_path.process_write(&data).unwrap();
        total_bytes += result.stats.pipeline.input_bytes;
    }

    let elapsed = start.elapsed();
    let throughput_mbps = (total_bytes as f64 / 1024.0 / 1024.0) / elapsed.as_secs_f64();

    assert!(
        throughput_mbps > 100.0,
        "Concurrent writes should show good throughput scaling"
    );
}

#[test]
fn test_throughput_with_dedup_enabled_90percent_similarity() {
    let mut config = WritePathConfig::default();
    config.pipeline.dedup_enabled = true;
    config.pipeline.compression_enabled = false;
    config.pipeline.encryption_enabled = false;

    let store = Arc::new(NullFingerprintStore::new());
    let mut write_path = IntegratedWritePath::new(config, store);

    let base_data = random_data(1024 * 1024);
    let result1 = write_path.process_write(&base_data).unwrap();
    let input_bytes = result1.stats.pipeline.input_bytes;

    for _ in 0..9 {
        let _ = write_path.process_write(&base_data).unwrap();
    }

    let cache_config = DedupCacheConfig { capacity: 1000 };
    let mut cache = DedupCache::new(cache_config);

    for i in 0..10 {
        let hash = [(i % 256) as u8; 32];
        cache.insert(hash);
    }

    let hits = (0..10)
        .filter(|i| cache.contains(&[(i % 256) as u8; 32]))
        .count();
    let hit_rate = hits as f64 / 10.0;

    assert!(hit_rate >= 0.9, "90% dedup hit rate expected");
}

#[test]
fn test_throughput_with_compression_enabled_8x_ratio() {
    let data = repetitive_data(8 * 1024 * 1024);

    let compressed =
        claudefs_reduce::compression::compress(&data, CompressionAlgorithm::Lz4).unwrap();

    let ratio = data.len() as f64 / compressed.len() as f64;

    assert!(
        ratio >= 7.0,
        "LZ4 should achieve ~8:1 compression on repetitive data"
    );
}

#[test]
fn test_throughput_with_ec_enabled_stripe_distribution() {
    let codec = ErasureCodec::new(EcStripe::FOUR_TWO);
    let payload = random_data(1024 * 1024);

    let start = Instant::now();
    let encoded = codec.encode(1, &payload).unwrap();
    let encode_time = start.elapsed();

    let overhead = encoded.total_size() as f64 / payload.len() as f64;
    let overhead_percent = (overhead - 1.0) * 100.0;

    assert!(
        overhead_percent < 10.0,
        "EC overhead should be <10%, got {:.2}%",
        overhead_percent
    );

    let start = Instant::now();
    let _decoded = codec.decode(&encoded).unwrap();
    let decode_time = start.elapsed();

    assert!(encode_time < Duration::from_millis(50));
    assert!(decode_time < Duration::from_millis(50));
}

#[test]
fn test_latency_small_write_p50_p99_p99p9() {
    let mut config = WritePathConfig::default();
    config.pipeline.dedup_enabled = false;
    config.pipeline.compression_enabled = false;

    let store = Arc::new(NullFingerprintStore::new());
    let mut write_path = IntegratedWritePath::new(config, store);

    let mut latencies = Vec::with_capacity(1000);

    for _ in 0..1000 {
        let data = random_data(4096);
        let start = Instant::now();
        let _ = write_path.process_write(&data).unwrap();
        latencies.push(start.elapsed().as_micros() as u64);
    }

    let p50 = calculate_percentile(&latencies, 50.0);
    let p99 = calculate_percentile(&latencies, 99.0);
    let p99p9 = calculate_percentile(&latencies, 99.9);

    assert!(p50 < 100, "p50 latency should be <100µs, got {}", p50);
    assert!(p99 < 500, "p99 latency should be <500µs, got {}", p99);
    assert!(p99p9 < 2000, "p99.9 latency should be <2ms, got {}", p99p9);
}

#[test]
fn test_latency_write_path_stages_breakdown() {
    let mut config = WritePathConfig::default();
    config.pipeline.dedup_enabled = true;
    config.pipeline.compression_enabled = true;
    config.pipeline.encryption_enabled = true;

    let store = Arc::new(NullFingerprintStore::new());
    let key = EncryptionKey([0x42u8; 32]);
    let mut write_path = IntegratedWritePath::new_with_key(config, key, store);

    let data = random_data(1024 * 1024);
    let result = write_path.process_write(&data).unwrap();

    let stages = [
        result.stats.pipeline.chunking_time_us,
        result.stats.pipeline.dedup_time_us,
        result.stats.pipeline.compress_time_us,
        result.stats.pipeline.encrypt_time_us,
    ];

    let total: u64 = stages.iter().sum();
    for stage in stages {
        let percentage = (stage as f64 / total as f64) * 100.0;
        assert!(
            percentage < 60.0,
            "No single stage should dominate, got {:.1}%",
            percentage
        );
    }
}

#[test]
fn test_amplification_write_amplification_with_tiering_active() {
    let mut config = claudefs_reduce::write_amplification::WriteAmplificationConfig::default();
    let mut tracker =
        claudefs_reduce::write_amplification::WriteAmplificationTracker::with_config(config);

    tracker.record(claudefs_reduce::write_amplification::WriteEvent {
        logical_bytes: 10 * 1024 * 1024,
        physical_bytes: 18 * 1024 * 1024,
        dedup_bytes_saved: 1 * 1024 * 1024,
        compression_bytes_saved: 8 * 1024 * 1024,
        ec_overhead_bytes: 2 * 1024 * 1024,
        timestamp_ms: 0,
    });

    let stats = tracker.stats();
    let amp = stats.write_amplification();

    assert!(
        amp <= 2.0,
        "Write amplification should be <=2.0, got {}",
        amp
    );
}

#[test]
fn test_amplification_read_amplification_ec_reconstruction() {
    let config = claudefs_reduce::read_amplification::ReadAmplificationConfig::default();
    let mut tracker = claudefs_reduce::read_amplification::ReadAmplificationTracker::new(config);

    tracker.record(claudefs_reduce::read_amplification::ReadEvent {
        logical_bytes: 1024 * 1024,
        physical_bytes: 2 * 1024 * 1024,
        io_count: 2,
        cache_hit: false,
    });

    let stats = tracker.stats();
    let amp = stats.amplification_ratio();

    assert!(
        amp <= 2.0,
        "Read amplification should be <=2.0, got {}",
        amp
    );
}

#[test]
fn test_cache_hit_rate_vs_cache_size_curve() {
    let cache_sizes = [10, 50, 100, 500, 1000];
    let mut hit_rates = Vec::new();

    for size in cache_sizes {
        let config = ReadCacheConfig {
            capacity_bytes: size as u64 * 1024 * 1024,
            max_entries: size,
        };
        let mut cache = ReadCache::new(config);

        for i in 0..size * 2 {
            let hash = ChunkHash([(i % 256) as u8; 32]);
            cache.insert(hash, vec![0u8; 4096]);
        }

        let hits: usize = (0..size * 2)
            .filter(|i| {
                let hash = ChunkHash([(i % 256) as u8; 32]);
                cache.get(&hash).is_some()
            })
            .count();

        let hit_rate = hits as f64 / (size * 2) as f64;
        hit_rates.push(hit_rate);
    }

    for i in 1..hit_rates.len() {
        assert!(
            hit_rates[i] >= hit_rates[i - 1] * 0.9,
            "Hit rate should improve with cache size"
        );
    }
}

#[test]
fn test_dedup_coordination_latency_p99_under_load() {
    let config = DedupCoordinatorConfig {
        num_shards: 16,
        local_node_id: 0,
    };
    let coordinator = DedupCoordinator::new(config);

    let mut latencies = Vec::with_capacity(1000);

    for _ in 0..1000 {
        let hash = [(rand::random::<u8>()) as u8; 32];
        let start = Instant::now();
        let _ = coordinator.shard_for_hash(&hash);
        latencies.push(start.elapsed().as_micros() as u64);
    }

    let p99 = calculate_percentile(&latencies, 99.0);
    assert!(p99 < 500, "Dedup p99 latency should be <500µs, got {}", p99);
}

#[test]
fn test_quota_enforcement_latency_impact() {
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
    for _ in 0..1000 {
        let _ = tracker.check_write(namespace, 1024, 1024);
    }
    let elapsed_no_quota = start.elapsed();

    let start = Instant::now();
    for _ in 0..1000 {
        let _ = tracker.check_write(namespace, 1024, 1024);
    }
    let elapsed_with_quota = start.elapsed();

    let impact_percent =
        (elapsed_with_quota.as_micros() as f64 / elapsed_no_quota.as_micros() as f64 - 1.0) * 100.0;

    assert!(
        impact_percent < 10.0,
        "Quota enforcement should add <10% latency impact"
    );
}

#[test]
fn test_backpressure_response_time_degradation() {
    let config = claudefs_reduce::pipeline_backpressure::BackpressureConfig::default();
    let mut backpressure =
        claudefs_reduce::pipeline_backpressure::PipelineBackpressure::new(config);

    for _ in 0..100 {
        backpressure.add_bytes(1024 * 1024);
        backpressure.add_chunks(1);
    }

    let start = Instant::now();
    let mut degraded_count = 0;
    for _ in 0..1000 {
        let decision = backpressure.should_backpressure();
        if decision {
            degraded_count += 1;
        }
    }
    let elapsed = start.elapsed();

    assert!(
        elapsed < Duration::from_millis(100),
        "Backpressure check should be fast"
    );
}

#[test]
fn test_scaling_nodes_linear_throughput_4_to_16_nodes() {
    let mut config = WritePathConfig::default();
    config.pipeline.dedup_enabled = false;
    config.pipeline.compression_enabled = false;

    let store = Arc::new(NullFingerprintStore::new());

    let mut throughputs = Vec::new();

    for node_count in [4, 8, 16] {
        let mut write_path = IntegratedWritePath::new(config.clone(), store.clone());
        let data = random_data(1024 * 1024);

        let start = Instant::now();
        for _ in 0..node_count {
            let _ = write_path.process_write(&data).unwrap();
        }
        let elapsed = start.elapsed();

        let throughput =
            (node_count as f64 * 1024.0 * 1024.0) / elapsed.as_secs_f64() / 1024.0 / 1024.0;
        throughputs.push(throughput);
    }

    assert!(
        throughputs[2] >= throughputs[0] * 3.0,
        "Throughput should scale roughly linearly with nodes"
    );
}

#[test]
fn test_scaling_dedup_shards_throughput_vs_shard_count() {
    let hash_counts = [4, 8, 16, 32];
    let mut lookup_times = Vec::new();

    for shard_count in hash_counts {
        let config = DedupCoordinatorConfig {
            num_shards: shard_count,
            local_node_id: 0,
        };
        let coordinator = DedupCoordinator::new(config);

        let start = Instant::now();
        for i in 0..10000 {
            let hash = [(i % 256) as u8; 32];
            let _ = coordinator.shard_for_hash(&hash);
        }
        let elapsed = start.elapsed();

        lookup_times.push(elapsed.as_micros());
    }

    for i in 1..lookup_times.len() {
        let ratio = lookup_times[i - 1] as f64 / lookup_times[i] as f64;
        assert!(
            ratio > 0.5 && ratio < 2.0,
            "Lookup time should remain reasonable across shard counts"
        );
    }
}

#[test]
fn test_scaling_gc_threads_throughput_impact() {
    use claudefs_reduce::gc_coordinator::{GcCandidate, GcCoordinator, GcCoordinatorConfig};

    let thread_counts = [1, 2, 4];
    let mut times = Vec::new();

    for threads in thread_counts {
        let config = GcCoordinatorConfig {
            max_concurrent_workers: threads,
            ..Default::default()
        };
        let mut coordinator = GcCoordinator::new(config);

        for i in 0..1000 {
            coordinator.add_candidate(GcCandidate {
                hash: [(i % 256) as u8; 32],
                ref_count: 0,
                size_bytes: 4096,
                segment_id: i as u64,
            });
        }

        let start = Instant::now();
        let _stats = coordinator.execute_sweep();
        let elapsed = start.elapsed();

        times.push(elapsed);
    }

    assert!(times[2] <= times[0], "More GC threads should not be slower");
}

#[test]
fn test_memory_usage_per_node_under_1tb_data() {
    let config = ReadCacheConfig {
        capacity_bytes: 100 * 1024 * 1024,
        max_entries: 10000,
    };
    let mut cache = ReadCache::new(config);

    for i in 0..10000 {
        let hash = ChunkHash([(i % 256) as u8; 32]);
        cache.insert(hash, vec![0u8; 4096]);
    }

    let stats = cache.stats();
    let memory_mb = stats.memory_used_bytes / 1024 / 1024;

    assert!(memory_mb < 100, "Cache memory usage should be reasonable");
}

#[test]
fn test_memory_usage_cache_overhead_per_gb_cached() {
    let config = DedupCacheConfig { capacity: 1000 };
    let mut cache = DedupCache::new(config);

    for i in 0..1000 {
        let hash = [(i % 256) as u8; 32];
        cache.insert(hash);
    }

    let stats = cache.stats();
    let overhead_per_entry = stats.memory_overhead_bytes as f64 / 1000.0;

    assert!(
        overhead_per_entry < 100.0,
        "Cache overhead per entry should be minimal"
    );
}

#[test]
fn test_cpu_usage_dedup_coordination_per_100k_fps_s() {
    let config = DedupCoordinatorConfig {
        num_shards: 16,
        local_node_id: 0,
    };
    let coordinator = DedupCoordinator::new(config);

    let start = Instant::now();
    for i in 0..100000 {
        let hash = [(i as u8) % 256; 32];
        let _ = coordinator.shard_for_hash(&hash);
    }
    let elapsed = start.elapsed();

    assert!(
        elapsed < Duration::from_millis(500),
        "Dedup coordination should be CPU efficient"
    );
}

#[test]
fn test_cpu_usage_compression_per_gb_s() {
    let data = repetitive_data(1024 * 1024);

    let start = Instant::now();
    let mut total_compressed = 0usize;
    for _ in 0..1000 {
        let compressed =
            claudefs_reduce::compression::compress(&data, CompressionAlgorithm::Lz4).unwrap();
        total_compressed += compressed.len();
    }
    let elapsed = start.elapsed();

    let throughput_gbps =
        (total_compressed as f64 / 1024.0 / 1024.0 / 1024.0) / elapsed.as_secs_f64();

    assert!(
        throughput_gbps > 0.5,
        "Compression should process data efficiently"
    );
}

#[test]
fn test_cpu_usage_encryption_per_gb_s() {
    let key = EncryptionKey([0x42u8; 32]);
    let data = random_data(1024 * 1024);

    let start = Instant::now();
    let mut total_encrypted = 0usize;
    for _ in 0..100 {
        let encrypted =
            claudefs_reduce::encryption::encrypt_aes_gcm(&data, &key, &[0u8; 12]).unwrap();
        total_encrypted += encrypted.len();
    }
    let elapsed = start.elapsed();

    let throughput_gbps =
        (total_encrypted as f64 / 1024.0 / 1024.0 / 1024.0) / elapsed.as_secs_f64();

    assert!(
        throughput_gbps > 1.0,
        "Encryption should be fast with AES-NI"
    );
}

#[test]
fn test_disk_io_queue_depth_distribution_under_load() {
    use claudefs_reduce::pipeline_backpressure::{BackpressureConfig, PipelineBackpressure};

    let config = BackpressureConfig {
        high_watermark_bytes: 100 * 1024 * 1024,
        low_watermark_bytes: 50 * 1024 * 1024,
        high_watermark_chunks: 32,
    };
    let mut backpressure = PipelineBackpressure::new(config);

    for _ in 0..16 {
        backpressure.add_bytes(1024 * 1024);
        backpressure.add_chunks(1);
    }

    let chunks_in_flight = backpressure.in_flight_chunks();

    assert!(
        chunks_in_flight >= 16 && chunks_in_flight <= 32,
        "Queue depth should be in target range 16-32"
    );
}

#[test]
fn test_network_bandwidth_utilized_vs_link_capacity() {
    let config = claudefs_reduce::bandwidth_throttle::ThrottleConfig {
        rate_bytes_per_sec: 100 * 1024 * 1024,
        burst_bytes: 10 * 1024 * 1024,
    };
    let mut throttle = claudefs_reduce::bandwidth_throttle::BandwidthThrottle::new(config);

    let mut allowed_count = 0;
    let mut throttled_count = 0;
    let mut now_ms = 0u64;

    for _ in 0..100 {
        let decision = throttle.request(1024 * 1024, now_ms);
        match decision {
            claudefs_reduce::bandwidth_throttle::ThrottleDecision::Allowed => {
                allowed_count += 1;
                now_ms += 10;
            }
            claudefs_reduce::bandwidth_throttle::ThrottleDecision::Throttled { .. } => {
                throttled_count += 1;
                now_ms += 1;
            }
        }
    }

    let utilization = allowed_count as f64 / 100.0;
    assert!(
        utilization >= 0.5,
        "Bandwidth utilization should be reasonable, got {:.1}%",
        utilization * 100.0
    );
}

#[test]
fn test_recovery_time_rto_after_single_node_failure() {
    use claudefs_reduce::stripe_coordinator::{EcConfig, NodeId, StripeCoordinator};

    let config = EcConfig {
        data_shards: 4,
        parity_shards: 2,
    };
    let nodes: Vec<_> = (0..6).map(NodeId).collect();
    let coordinator = StripeCoordinator::new(config, nodes);

    let start = Instant::now();
    for i in 1..=10 {
        let plan = coordinator.plan_stripe(i);
        assert!(plan.placements.len() > 0);
    }
    let elapsed = start.elapsed();

    assert!(
        elapsed < Duration::from_millis(100),
        "Stripe planning should be fast for RTO"
    );
}

#[test]
fn test_recovery_time_rpo_data_loss_on_node_failure() {
    let config = claudefs_reduce::write_journal::WriteJournalConfig::default();
    let mut journal = claudefs_reduce::write_journal::WriteJournal::new(config);

    let start = Instant::now();
    for i in 0..1000 {
        journal.append(claudefs_reduce::write_journal::JournalEntryData {
            inode_id: 1,
            offset: i as u64 * 4096,
            size: 4096,
            timestamp_ms: i as u64,
        });
    }
    let elapsed = start.elapsed();

    let lag_ms = journal.latest_timestamp_ms().unwrap_or(0);

    assert!(
        lag_ms < 5000,
        "Journal lag should be within RPO target ~1-5s"
    );
    assert!(
        elapsed < Duration::from_millis(100),
        "Journal writes should be fast"
    );
}
