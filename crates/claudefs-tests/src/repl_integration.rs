//! Replication crate integration tests
//!
//! Tests for claudefs-repl crate: compression, backpressure, metrics, and conduit.

use claudefs_repl::backpressure::{BackpressureConfig, BackpressureController, BackpressureLevel};
use claudefs_repl::compression::{
    BatchCompressor, CompressedBatch, CompressionAlgo, CompressionConfig,
};
use claudefs_repl::conduit::{ConduitConfig, EntryBatch};
use claudefs_repl::metrics::{Metric, MetricsAggregator, ReplMetrics};

#[test]
fn test_compression_algo_default_is_lz4() {
    let algo = CompressionAlgo::default();
    assert_eq!(algo, CompressionAlgo::Lz4);
}

#[test]
fn test_compression_algo_none_is_compressed_false() {
    assert!(!CompressionAlgo::None.is_compressed());
}

#[test]
fn test_compression_algo_lz4_is_compressed_true() {
    assert!(CompressionAlgo::Lz4.is_compressed());
}

#[test]
fn test_compression_algo_zstd_is_compressed_true() {
    assert!(CompressionAlgo::Zstd.is_compressed());
}

#[test]
fn test_compression_config_default() {
    let config = CompressionConfig::default();
    assert_eq!(config.algo, CompressionAlgo::Lz4);
    assert_eq!(config.zstd_level, 3);
    assert_eq!(config.min_compress_bytes, 256);
}

#[test]
fn test_compression_config_custom() {
    let config = CompressionConfig {
        algo: CompressionAlgo::Zstd,
        zstd_level: 10,
        min_compress_bytes: 512,
    };
    assert_eq!(config.algo, CompressionAlgo::Zstd);
    assert_eq!(config.zstd_level, 10);
    assert_eq!(config.min_compress_bytes, 512);
}

#[test]
fn test_compressed_batch_compression_ratio_equal_size() {
    let batch = CompressedBatch {
        batch_seq: 1,
        source_site_id: 1,
        original_bytes: 100,
        compressed_bytes: 100,
        algo: CompressionAlgo::None,
        data: vec![],
    };
    let ratio = batch.compression_ratio();
    assert!((ratio - 1.0).abs() < 0.001);
}

#[test]
fn test_compressed_batch_is_beneficial_when_compressed() {
    let batch = CompressedBatch {
        batch_seq: 1,
        source_site_id: 1,
        original_bytes: 1000,
        compressed_bytes: 500,
        algo: CompressionAlgo::Lz4,
        data: vec![],
    };
    assert!(batch.is_beneficial());
}

#[test]
fn test_compressed_batch_is_beneficial_false_when_not_compressed() {
    let batch = CompressedBatch {
        batch_seq: 1,
        source_site_id: 1,
        original_bytes: 500,
        compressed_bytes: 500,
        algo: CompressionAlgo::None,
        data: vec![],
    };
    assert!(!batch.is_beneficial());
}

#[test]
fn test_backpressure_level_ordering() {
    assert!(BackpressureLevel::None < BackpressureLevel::Mild);
    assert!(BackpressureLevel::Mild < BackpressureLevel::Moderate);
    assert!(BackpressureLevel::Moderate < BackpressureLevel::Severe);
    assert!(BackpressureLevel::Severe < BackpressureLevel::Halt);
}

#[test]
fn test_backpressure_level_suggested_delay_ms() {
    assert_eq!(BackpressureLevel::None.suggested_delay_ms(), 0);
    assert_eq!(BackpressureLevel::Mild.suggested_delay_ms(), 5);
    assert_eq!(BackpressureLevel::Moderate.suggested_delay_ms(), 50);
    assert_eq!(BackpressureLevel::Severe.suggested_delay_ms(), 500);
    assert_eq!(BackpressureLevel::Halt.suggested_delay_ms(), u64::MAX);
}

#[test]
fn test_backpressure_level_is_halted_only_halt() {
    assert!(!BackpressureLevel::None.is_halted());
    assert!(!BackpressureLevel::Mild.is_halted());
    assert!(!BackpressureLevel::Moderate.is_halted());
    assert!(!BackpressureLevel::Severe.is_halted());
    assert!(BackpressureLevel::Halt.is_halted());
}

#[test]
fn test_backpressure_level_is_active_non_none() {
    assert!(!BackpressureLevel::None.is_active());
    assert!(BackpressureLevel::Mild.is_active());
    assert!(BackpressureLevel::Moderate.is_active());
    assert!(BackpressureLevel::Severe.is_active());
    assert!(BackpressureLevel::Halt.is_active());
}

#[test]
fn test_backpressure_controller_starts_at_none() {
    let controller = BackpressureController::new(BackpressureConfig::default());
    assert_eq!(controller.current_level(), BackpressureLevel::None);
}

#[test]
fn test_backpressure_controller_queue_depth_triggers_mild() {
    let config = BackpressureConfig {
        mild_queue_depth: 1000,
        moderate_queue_depth: 10000,
        severe_queue_depth: 100000,
        halt_queue_depth: 1000000,
        ..Default::default()
    };
    let mut controller = BackpressureController::new(config);
    controller.set_queue_depth(1000);
    let level = controller.compute_level();
    assert_eq!(level, BackpressureLevel::Mild);
}

#[test]
fn test_backpressure_controller_error_count_triggers_moderate() {
    let mut controller = BackpressureController::new(BackpressureConfig::default());
    for _ in 0..3 {
        controller.record_error();
    }
    let level = controller.compute_level();
    assert_eq!(level, BackpressureLevel::Moderate);
}

#[test]
fn test_backpressure_controller_force_halt() {
    let mut controller = BackpressureController::new(BackpressureConfig::default());
    controller.force_halt();
    let level = controller.compute_level();
    assert_eq!(level, BackpressureLevel::Halt);
}

#[test]
fn test_backpressure_controller_clear_halt() {
    let mut controller = BackpressureController::new(BackpressureConfig::default());
    controller.force_halt();
    controller.clear_halt();
    let level = controller.compute_level();
    assert_eq!(level, BackpressureLevel::None);
}

#[test]
fn test_metric_counter_format_contains_type() {
    let metric = Metric::counter("test_counter", "A test counter", vec![], 42.0);
    let output = metric.format();
    assert!(output.contains("# TYPE test_counter counter"));
}

#[test]
fn test_metric_gauge_format_contains_type() {
    let metric = Metric::gauge("test_gauge", "A test gauge", vec![], 42.0);
    let output = metric.format();
    assert!(output.contains("# TYPE test_gauge gauge"));
}

#[test]
fn test_metric_with_labels_formats_correctly() {
    let metric = Metric::counter(
        "test_counter",
        "A test counter",
        vec![("site_id".to_string(), "1".to_string())],
        42.0,
    );
    let output = metric.format();
    assert!(output.contains("site_id=\"1\""));
}

#[test]
fn test_conduit_config_default() {
    let config = ConduitConfig::default();
    assert_eq!(config.local_site_id, 0);
    assert_eq!(config.remote_site_id, 0);
}

#[test]
fn test_conduit_config_new() {
    let config = ConduitConfig::new(1, 2);
    assert_eq!(config.local_site_id, 1);
    assert_eq!(config.remote_site_id, 2);
}

#[test]
fn test_entry_batch_new() {
    let batch = EntryBatch {
        batch_seq: 42,
        source_site_id: 1,
        entries: vec![],
    };
    assert_eq!(batch.batch_seq, 42);
    assert_eq!(batch.source_site_id, 1);
    assert!(batch.entries.is_empty());
}

#[test]
fn test_entry_batch_bincode_roundtrip() {
    let batch = EntryBatch {
        batch_seq: 42,
        source_site_id: 1,
        entries: vec![],
    };
    let encoded = bincode::serialize(&batch).unwrap();
    let decoded: EntryBatch = bincode::deserialize(&encoded).unwrap();
    assert_eq!(decoded.batch_seq, 42);
    assert_eq!(decoded.source_site_id, 1);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_repl_metrics_default() {
        let metrics = ReplMetrics::default();
        assert_eq!(metrics.site_id, 0);
        assert_eq!(metrics.entries_tailed, 0);
    }

    #[test]
    fn test_metrics_aggregator_new() {
        let aggregator = MetricsAggregator::new();
        assert_eq!(aggregator.site_count(), 0);
    }
}
