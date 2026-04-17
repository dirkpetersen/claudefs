use claudefs_reduce::{
    cache_coherency::{CacheKey, CacheVersion, CoherencyTracker, InvalidationEvent},
    gc_coordinator::{GcCandidate, GcCoordinator, GcCoordinatorConfig},
    metrics::ReductionMetrics,
    multi_tenant_quotas::{MultiTenantQuotas, QuotaLimit, TenantId},
    pipeline_backpressure::{BackpressureConfig, PipelineBackpressure},
    pipeline_monitor::{AlertThreshold, PipelineAlert, PipelineMonitor, StageMetrics},
    read_amplification::{ReadAmplificationConfig, ReadAmplificationTracker, ReadEvent},
    tenant_isolator::{TenantId as IsolatorTenantId, TenantIsolator, TenantPolicy, TenantPriority},
    write_amplification::{
        WriteAmplificationConfig, WriteAmplificationStats, WriteAmplificationTracker, WriteEvent,
    },
    write_coalescer::{CoalesceConfig, WriteCoalescer, WriteOp},
};
use std::time::{Duration, Instant};

#[test]
fn test_write_amplification_ratio_tracking() {
    let config = WriteAmplificationConfig { max_events: 100 };
    let mut tracker = WriteAmplificationTracker::with_config(config);

    tracker.record(WriteEvent {
        logical_bytes: 10 * 1024 * 1024,
        physical_bytes: 7 * 1024 * 1024,
        dedup_bytes_saved: 2 * 1024 * 1024,
        compression_bytes_saved: 1 * 1024 * 1024,
        ec_overhead_bytes: 500 * 1024,
        timestamp_ms: 0,
    });

    let stats = tracker.stats();
    let ratio = 7.0 / 10.0;

    assert!((stats.write_amplification() - ratio).abs() < 0.01);
}

#[test]
fn test_read_amplification_basic() {
    let config = ReadAmplificationConfig::default();
    let mut tracker = ReadAmplificationTracker::new(config);

    tracker.record(ReadEvent {
        logical_bytes: 1024 * 1024,
        physical_bytes: 5 * 1024 * 1024,
        io_count: 5,
        cache_hit: false,
    });

    let stats = tracker.stats();
    let amp = stats.amplification_ratio();

    assert!((amp - 5.0).abs() < 0.1, "Amplification should be 5x");
}

#[test]
fn test_metrics_export() {
    let metrics = ReductionMetrics::new();

    metrics.record_chunk(100, 50);
    metrics.record_dedup_hit();
    metrics.record_dedup_miss();
    metrics.record_compress(100, 50);
    metrics.record_encrypt();
    metrics.record_gc_cycle(100);
    metrics.record_key_rotation();

    let collected = metrics.collect();
    assert!(!collected.is_empty());

    let output = metrics.render_prometheus();
    assert!(output.contains("claudefs_reduce_chunks_processed_total"));
}

#[test]
fn test_metrics_dedup_stats() {
    let metrics = ReductionMetrics::new();

    metrics.record_dedup_hit();
    metrics.record_dedup_hit();
    metrics.record_dedup_miss();

    let ratio = metrics.dedup_ratio();
    assert!((ratio - 2.0 / 3.0).abs() < 0.01);

    let output = metrics.render_prometheus();
    assert!(output.contains("claudefs_reduce_dedup_ratio"));
}

#[test]
fn test_tenant_isolation() {
    let quotas = MultiTenantQuotas::new();

    let tenant1 = TenantId(1);
    let tenant2 = TenantId(2);

    quotas
        .set_quota(
            tenant1,
            QuotaLimit::new(50 * 1024 * 1024, 50 * 1024 * 1024, true),
        )
        .unwrap();
    quotas
        .set_quota(
            tenant2,
            QuotaLimit::new(50 * 1024 * 1024, 50 * 1024 * 1024, true),
        )
        .unwrap();

    for _ in 0..1000 {
        let _ = quotas.record_write(tenant1, 50 * 1024, 25 * 1024, 25 * 1024);
    }

    let start = Instant::now();
    for _ in 0..100 {
        let _ = quotas.record_write(tenant2, 1024, 512, 512);
    }
    let elapsed = start.elapsed();

    assert!(elapsed < Duration::from_millis(100), "Should be fast");
}

#[test]
fn test_pipeline_backpressure_basic() {
    let config = BackpressureConfig {
        high_watermark_bytes: 100 * 1024 * 1024,
        low_watermark_bytes: 50 * 1024 * 1024,
        high_watermark_chunks: 100,
    };
    let mut backpressure = PipelineBackpressure::new(config);

    for _ in 0..100 {
        backpressure.add_bytes(1024 * 1024);
        backpressure.add_chunks(1);
    }

    assert!(backpressure.in_flight_bytes() >= 100 * 1024 * 1024);
}

#[test]
fn test_pipeline_monitor_alerts() {
    let mut monitor = PipelineMonitor::new();

    monitor.record_stage(StageMetrics {
        stage_name: "write".to_string(),
        chunks_in: 100,
        chunks_out: 100,
        bytes_in: 1024,
        bytes_out: 512,
        errors: 0,
        latency_sum_us: 60000,
        latency_count: 10,
    });

    let threshold = AlertThreshold {
        max_error_rate: 0.01,
        min_reduction_ratio: 1.0,
        max_latency_us: 5000,
    };

    let alerts = monitor.check_alerts(&threshold);
    assert!(!alerts.is_empty());
    assert!(matches!(alerts[0], PipelineAlert::HighLatency { .. }));
}

#[test]
fn test_pipeline_monitor_snapshot() {
    let mut monitor = PipelineMonitor::new();

    monitor.record_stage(StageMetrics {
        stage_name: "dedup".to_string(),
        chunks_in: 100,
        chunks_out: 80,
        bytes_in: 1024,
        bytes_out: 800,
        errors: 0,
        latency_sum_us: 1000,
        latency_count: 100,
    });

    let snapshot = monitor.snapshot();
    assert_eq!(snapshot.stages.len(), 1);
    assert_eq!(snapshot.total_bytes_in, 1024);
}

#[test]
fn test_write_coalescer_basic() {
    let config = CoalesceConfig {
        max_gap_bytes: 0,
        max_coalesced_bytes: 256 * 1024,
        window_ms: 10,
    };
    let mut coalescer = WriteCoalescer::new(config);

    for _ in 0..100 {
        coalescer.add(WriteOp {
            inode_id: 1,
            offset: 0,
            data: vec![0u8; 4 * 1024],
            timestamp_ms: 100,
        });
    }

    let flushed = coalescer.flush_all();
    assert!(!flushed.is_empty());
}

#[test]
fn test_gc_coordinator_stats() {
    let config = GcCoordinatorConfig::default();
    let mut coordinator = GcCoordinator::new(config);

    for i in 0..100 {
        let hash = [i as u8; 32];
        let ref_count = if i < 50 { 1 } else { 0 };
        coordinator.add_candidate(GcCandidate {
            hash,
            ref_count,
            size_bytes: 1024,
            segment_id: i as u64,
        });
    }

    assert_eq!(coordinator.candidate_count(), 100);
}

#[test]
fn test_cache_coherency_basic() {
    let mut tracker = CoherencyTracker::new();

    let key = CacheKey {
        inode_id: 1,
        chunk_index: 0,
    };
    let version = CacheVersion::new();

    tracker.register(key, version, 1024);

    let is_valid = tracker.is_valid(&key, &version);
    assert!(is_valid);

    tracker.invalidate(&InvalidationEvent::ChunkInvalidated { key });

    let is_valid_after = tracker.is_valid(&key, &version);
    assert!(!is_valid_after);
}

#[test]
fn test_tenant_isolator_usage() {
    let mut isolator = TenantIsolator::default();

    let tenant1 = IsolatorTenantId(1);
    let tenant2 = IsolatorTenantId(2);

    isolator.register_tenant(TenantPolicy {
        tenant_id: tenant1,
        quota_bytes: 10 * 1024 * 1024,
        max_iops: 1000,
        priority: TenantPriority::Normal,
    });
    isolator.register_tenant(TenantPolicy {
        tenant_id: tenant2,
        quota_bytes: 10 * 1024 * 1024,
        max_iops: 1000,
        priority: TenantPriority::Normal,
    });

    isolator.record_write(tenant1, 1024).unwrap();
    isolator.record_write(tenant1, 2048).unwrap();

    isolator.record_write(tenant2, 512).unwrap();

    let usage1 = isolator.get_usage(tenant1).unwrap();
    let usage2 = isolator.get_usage(tenant2).unwrap();

    assert_eq!(usage1.bytes_used, 3072);
    assert_eq!(usage2.bytes_used, 512);
}
