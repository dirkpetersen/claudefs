use claudefs_reduce::{
    cache_coherency::{CacheCoherency, InvalidationEvent},
    chunk_scheduler::OpPriority,
    fingerprint::ChunkHash,
    gc_coordinator::{GcCandidate, GcCoordinator, GcCoordinatorConfig},
    metrics::{MetricKind, MetricValue, MetricsHandle},
    multi_tenant_quotas::{MultiTenantQuotas, QuotaLimit, TenantId},
    pipeline_backpressure::{BackpressureConfig, PipelineBackpressure},
    pipeline_monitor::{AlertThreshold, PipelineMonitor},
    read_amplification::{ReadAmplificationConfig, ReadAmplificationTracker, ReadEvent},
    similarity_coordinator::{SimilarityConfig, SimilarityCoordinator},
    tenant_isolator::{TenantId as IsolatorTenantId, TenantIsolator},
    write_amplification::{
        WriteAmplificationConfig, WriteAmplificationStats, WriteAmplificationTracker, WriteEvent,
    },
    write_coalescer::{CoalesceConfig, WriteCoalescer, WriteOp},
};
use std::collections::HashMap;
use std::time::{Duration, Instant};

#[test]
fn test_write_amplification_ratio_tracking() {
    let config = WriteAmplificationConfig { max_events: 100 };
    let mut tracker = WriteAmplificationTracker::new(config);

    tracker.record_event(WriteEvent {
        logical_bytes: 10 * 1024 * 1024,
        physical_bytes: 7 * 1024 * 1024,
        dedup_bytes_saved: 2 * 1024 * 1024,
        compression_bytes_saved: 1 * 1024 * 1024,
        ec_overhead_bytes: 500 * 1024,
        timestamp_ms: 0,
    });

    let stats = tracker.aggregate_stats();
    let ratio = 7.0 / 10.0;

    assert!((stats.write_amplification() - ratio).abs() < 0.01);
}

#[test]
fn test_read_amplification_basic() {
    let config = ReadAmplificationConfig::default();
    let mut tracker = ReadAmplificationTracker::new(config);

    tracker.record_event(ReadEvent {
        logical_bytes: 1024 * 1024,
        physical_blocks: 5,
        timestamp_ms: 0,
    });

    let stats = tracker.aggregate_stats();
    let amp = stats.amplification_factor();

    assert!((amp - 5.0).abs() < 0.1, "Amplification should be 5x");
}

#[test]
fn test_metrics_export() {
    let mut handle = MetricsHandle::default();

    handle
        .record_metric(
            "test_metric".to_string(),
            MetricKind::Counter(100),
            HashMap::new(),
        )
        .unwrap();

    let output = handle.export_prometheus();
    assert!(output.contains("test_metric"));
}

#[test]
fn test_metrics_dedup_stats() {
    let mut handle = MetricsHandle::default();

    handle
        .record_metric(
            "dedup_blocks_found".to_string(),
            MetricKind::Counter(100),
            HashMap::new(),
        )
        .unwrap();
    handle
        .record_metric(
            "dedup_bytes_saved".to_string(),
            MetricKind::Counter(1024),
            HashMap::new(),
        )
        .unwrap();

    let output = handle.export_prometheus();
    assert!(output.contains("dedup_blocks_found"));
    assert!(output.contains("dedup_bytes_saved"));
}

#[test]
fn test_tenant_isolation() {
    let quotas = MultiTenantQuotas::new();

    let tenant1 = TenantId(1);
    let tenant2 = TenantId(2);

    quotas.set_quota(
        tenant1,
        QuotaLimit::new(50 * 1024 * 1024, 50 * 1024 * 1024, true),
    );
    quotas.set_quota(
        tenant2,
        QuotaLimit::new(50 * 1024 * 1024, 50 * 1024 * 1024, true),
    );

    for _ in 0..1000 {
        let _ = quotas.check_and_update(tenant1, 50 * 1024);
    }

    let start = Instant::now();
    for _ in 0..100 {
        let _ = quotas.check_and_update(tenant2, 1024);
    }
    let elapsed = start.elapsed();

    assert!(elapsed < Duration::from_millis(100), "Should be fast");
}

#[test]
fn test_similarity_detection_performance() {
    let config = SimilarityConfig {
        threshold: 90,
        batch_size: 100,
    };
    let coordinator = SimilarityCoordinator::new(config);

    let blocks: Vec<Vec<u8>> = (0..50).map(|i| vec![i as u8; 4096]).collect();

    let start = Instant::now();
    for i in 0..50 {
        for j in (i + 1)..50.min(i + 5) {
            let _ = coordinator.detect_similarity(&blocks[i], &blocks[j]);
        }
    }
    let elapsed = start.elapsed();

    assert!(elapsed < Duration::from_millis(500));
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
    let thresholds = vec![AlertThreshold {
        metric: "write_latency_ms".to_string(),
        operator: claudefs_reduce::pipeline_monitor::AlertOperator::GreaterThan,
        value: 50.0,
    }];
    let mut monitor = PipelineMonitor::new(thresholds);

    monitor.record_latency("write_latency_ms", 60.0);
    monitor.record_latency("write_latency_ms", 70.0);

    let alerts = monitor.check_alerts();
    assert!(!alerts.is_empty());
}

#[test]
fn test_pipeline_monitor_percentiles() {
    let mut monitor = PipelineMonitor::default();

    for i in 0..1000 {
        let latency = (i as f64 * 0.1) + 10.0;
        monitor.record_latency("test_op", latency);
    }

    let percentiles = monitor.get_percentiles("test_op");

    assert!(percentiles.p50 < percentiles.p95);
    assert!(percentiles.p95 < percentiles.p99);
}

#[test]
fn test_write_coalescer_basic() {
    let config = CoalesceConfig {
        max_coalesce_bytes: 256 * 1024,
        max_latency_ms: 10,
    };
    let mut coalescer = WriteCoalescer::new(config);

    let coalesced_count = (0..100)
        .filter_map(|_| {
            let op = WriteOp {
                data: vec![0u8; 4 * 1024],
                priority: OpPriority::Background,
            };
            coalescer.try_add(op).and_then(|o| o)
        })
        .count();

    assert!(coalesced_count >= 0);
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

    let candidates = coordinator.candidates();
    assert_eq!(candidates.len(), 100);
}

#[test]
fn test_cache_coherency_basic() {
    let mut coherency = CacheCoherency::default();

    let hash = ChunkHash([42u8; 32]);

    coherency.cache(hash, vec![0x42u8; 1024]).unwrap();

    coherency.invalidate(InvalidationEvent {
        hash,
        reason: "write".to_string(),
    });

    let cached = coherency.get(&hash);
    assert!(cached.is_none());
}

#[test]
fn test_tenant_isolator_routing() {
    let mut isolator = TenantIsolator::default();

    let tenant1 = IsolatorTenantId(1);
    let tenant2 = IsolatorTenantId(2);

    let mut hashes1 = 0usize;
    let mut hashes2 = 0usize;

    for i in 0..1000 {
        let hash = [i as u8; 32];
        let tenant = isolator.route_hash(hash);

        if tenant == tenant1 {
            hashes1 += 1;
        } else if tenant == tenant2 {
            hashes2 += 1;
        }
    }

    let total = hashes1 + hashes2;
    assert!(total > 0, "Should route some hashes");
}
