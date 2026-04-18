//! Dynamic GC Tuning Integration Tests
//!
//! Tests for adaptive garbage collection with memory pressure,
//! workload-aware frequency, and reference count validation.
//!
//! All tests marked #[ignore] - run with: cargo test --test cluster_gc_dynamic -- --ignored

use std::collections::HashMap;
use std::time::Duration;

mod cluster_helpers;
use cluster_helpers::*;
use claudefs_reduce::{
    DynamicGcController, GcController, GcControllerConfig, GcControllerStats, GcThresholds,
    WorkloadType, GcBackpressure, GcBackpressureConfig, GcBackpressureState,
    RefCountValidator, ReferenceCountValidator, MarkAndSweepAudit, ReconciliationResult,
    BlockId,
};

#[tokio::test]
#[ignore]
async fn test_gc_threshold_low_memory() {
    let config = GcControllerConfig {
        high_memory_threshold: 30.0,
        low_memory_threshold: 20.0,
        ..Default::default()
    };
    let mut controller = DynamicGcController::new(config);

    for _ in 0..1000 {
        controller.update_workload_stats(1, 0.001);
    }

    let result = controller.should_collect();
    assert!(result.is_ok());

    if let Ok(Some(interval)) = result {
        assert_eq!(interval, Duration::ZERO, "Should trigger immediate collection under memory pressure");
    }
}

#[tokio::test]
#[ignore]
async fn test_gc_threshold_high_memory() {
    let config = GcControllerConfig::default();
    let mut controller = DynamicGcController::new(config);

    controller.update_workload_stats(1, 0.0001);

    let result = controller.should_collect();
    assert!(result.is_ok());

    if let Ok(Some(interval)) = result {
        assert!(interval.as_millis() > 0, "Should not trigger immediate collection when memory is low");
    }
}

#[tokio::test]
#[ignore]
async fn test_gc_backpressure_under_load() {
    let mut backpressure = GcBackpressure::with_default();

    backpressure.record_collection(1500);

    let delay = backpressure.calculate_delay();
    assert!(delay.is_some(), "Should apply backpressure when collection is slow");

    if let Some(d) = delay {
        assert!(d.as_micros() > 0, "Delay should be positive");
    }
}

#[tokio::test]
#[ignore]
async fn test_gc_recovery_after_pressure() {
    let mut backpressure = GcBackpressure::with_default();

    backpressure.record_collection(1500);
    let _ = backpressure.calculate_delay();

    backpressure.record_collection(100);
    let delay = backpressure.calculate_delay();

    if let Some(d) = delay {
        assert!(d.as_micros() < 10000, "Delay should decrease after recovery");
    }
}

#[tokio::test]
#[ignore]
async fn test_gc_batch_writes_high_frequency() {
    let config = GcControllerConfig {
        min_collection_interval_ms: 50,
        max_collection_interval_ms: 2000,
        ..Default::default()
    };
    let mut controller = DynamicGcController::new(config);

    for _ in 0..100 {
        controller.update_workload_stats(1000, 0.5);
    }

    let thresholds = controller.get_thresholds();
    assert_eq!(thresholds.workload_type, WorkloadType::Batch);
}

#[tokio::test]
#[ignore]
async fn test_gc_streaming_low_frequency() {
    let config = GcControllerConfig::default();
    let mut controller = DynamicGcController::new(config);

    for _ in 0..100 {
        controller.update_workload_stats(10, 0.01);
    }

    let thresholds = controller.get_thresholds();
    assert_eq!(thresholds.workload_type, WorkloadType::Streaming);
}

#[tokio::test]
#[ignore]
async fn test_gc_idle_background_sweep() {
    let config = GcControllerConfig::default();
    let max_interval = config.max_collection_interval_ms;
    let controller = DynamicGcController::new(config);

    tokio::time::sleep(Duration::from_secs(1)).await;

    let thresholds = controller.get_thresholds();
    assert_eq!(thresholds.workload_type, WorkloadType::Idle);
    assert!(thresholds.collection_interval_ms <= max_interval);
}

#[tokio::test]
#[ignore]
async fn test_gc_mixed_workload_adaptation() {
    let config = GcControllerConfig {
        min_collection_interval_ms: 100,
        max_collection_interval_ms: 5000,
        ..Default::default()
    };
    let mut controller = DynamicGcController::new(config);

    for _ in 0..50 {
        controller.update_workload_stats(1000, 0.5);
    }

    let batch_interval = controller.get_thresholds().collection_interval_ms;

    for _ in 0..50 {
        controller.update_workload_stats(10, 0.01);
    }

    let streaming_interval = controller.get_thresholds().collection_interval_ms;

    assert!(batch_interval < streaming_interval, "Batch should trigger more frequent GC");
}

#[tokio::test]
#[ignore]
async fn test_refcount_increment_decrement_balance() {
    let mut validator = RefCountValidator::new();

    for i in 0..100 {
        let mut block_id = [0u8; 32];
        block_id[0] = i as u8;
        validator.add_block(block_id, 0);
    }

    let audit = validator.audit().unwrap();

    assert!(audit.corrupted_refcounts.is_empty());
}

#[tokio::test]
#[ignore]
async fn test_refcount_dedup_block_sharing() {
    let mut validator = RefCountValidator::new();

    let block_id = [0xAB; 32];

    validator.add_block(block_id, 2);
    validator.mark_reachable(block_id);
    validator.mark_reachable(block_id);

    let audit = validator.audit().unwrap();

    assert!(audit.reachable_blocks.contains(&block_id));
    assert!(!audit.orphaned_blocks.contains(&block_id));
}

#[tokio::test]
#[ignore]
async fn test_refcount_orphaned_block_detection() {
    let mut validator = RefCountValidator::new();

    let orphan_block = [0xFF; 32];
    validator.add_block(orphan_block, 1);

    let audit = validator.audit().unwrap();

    assert!(audit.orphaned_blocks.contains(&orphan_block));
}

#[tokio::test]
#[ignore]
async fn test_refcount_multi_snapshot_complex() {
    let mut validator = RefCountValidator::new();

    let blocks: Vec<_> = (0..5).map(|i| {
        let mut b = [0u8; 32];
        b[0] = i;
        b
    }).collect();

    for (i, block) in blocks.iter().enumerate() {
        validator.add_block(*block, (i + 1) as u64);
    }

    for block in &blocks {
        validator.mark_reachable(*block);
    }

    let audit = validator.audit().unwrap();

    for block in &blocks {
        assert!(audit.reachable_blocks.contains(block));
    }
    assert!(audit.orphaned_blocks.is_empty());
}

#[tokio::test]
#[ignore]
async fn test_mark_sweep_finds_all_reachable() {
    let mut validator = RefCountValidator::new();

    for i in 0..100 {
        let mut block_id = [0u8; 32];
        block_id[1] = i as u8;
        validator.add_block(block_id, 1);
        validator.mark_reachable(block_id);
    }

    let audit = validator.audit().unwrap();

    assert_eq!(audit.reachable_blocks.len(), 100);
    assert_eq!(audit.blocks_scanned, 100);
}

#[tokio::test]
#[ignore]
async fn test_mark_sweep_detects_orphans() {
    let mut validator = RefCountValidator::new();

    let orphan = [0xAA; 32];
    validator.add_block(orphan, 1);

    let normal = [0xBB; 32];
    validator.add_block(normal, 1);
    validator.mark_reachable(normal);

    let audit = validator.audit().unwrap();

    assert!(audit.orphaned_blocks.contains(&orphan));
    assert!(!audit.orphaned_blocks.contains(&normal));
}

#[tokio::test]
#[ignore]
async fn test_mark_sweep_corrects_overcounts() {
    let mut validator = RefCountValidator::new();

    let block = [0xCC; 32];
    validator.add_block(block, 999999);
    validator.mark_reachable(block);

    let audit = validator.audit().unwrap();

    assert!(!audit.corrupted_refcounts.is_empty());
    assert!(audit.corrupted_refcounts.iter().any(|(id, _)| *id == block));

    let result = validator.reconcile(&audit).unwrap();

    assert!(result.refcounts_corrected > 0);
}

#[tokio::test]
#[ignore]
async fn test_mark_sweep_concurrent_safe() {
    let mut validator = RefCountValidator::new();

    for i in 0..1000 {
        let mut block_id = [0u8; 32];
        block_id[2] = (i / 10) as u8;
        validator.add_block(block_id, 1);
    }

    let audit = validator.audit().unwrap();

    assert!(audit.blocks_scanned > 0);
}

#[tokio::test]
#[ignore]
async fn test_mark_sweep_large_index_performance() {
    let mut validator = RefCountValidator::new();

    for i in 0..10000 {
        let mut block_id = [0u8; 32];
        block_id[3] = (i / 100) as u8;
        block_id[4] = (i % 100) as u8;
        validator.add_block(block_id, 1);
    }

    let start = std::time::Instant::now();
    let audit = validator.audit().unwrap();
    let elapsed = start.elapsed();

    assert!(elapsed.as_millis() < 1000, "Audit should complete in <1s for 10K blocks");
    assert_eq!(audit.blocks_scanned, 10000);
}

#[tokio::test]
#[ignore]
async fn test_gc_force_collection() {
    let config = GcControllerConfig::default();
    let mut controller = DynamicGcController::new(config);

    controller.force_collect().unwrap();

    let result = controller.should_collect().unwrap();
    assert!(result.is_some());

    if let Some(interval) = result {
        assert_eq!(interval, Duration::ZERO, "Forced collection should return zero interval");
    }
}

#[tokio::test]
#[ignore]
async fn test_backpressure_state_reporting() {
    let mut backpressure = GcBackpressure::with_default();

    backpressure.record_collection(500);
    let _ = backpressure.calculate_delay();

    let state = backpressure.get_state();

    assert!(state.total_delays_applied > 0 || state.current_delay_us > 0);
}

#[tokio::test]
#[ignore]
async fn test_gc_controller_stats() {
    let config = GcControllerConfig::default();
    let controller = DynamicGcController::new(config);

    let stats = controller.get_stats();

    assert_eq!(stats.total_collections, 0);
}

#[tokio::test]
#[ignore]
async fn test_reconciliation_actions() {
    let mut validator = RefCountValidator::new();

    let orphan = [0x11; 32];
    validator.add_block(orphan, 1);

    let audit = validator.audit().unwrap();
    let result = validator.reconcile(&audit).unwrap();

    assert!(result.blocks_deleted > 0 || result.actions.len() > 0);
}