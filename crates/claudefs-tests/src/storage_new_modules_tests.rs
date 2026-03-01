//! Tests for new storage crate modules: atomic_write, block_cache, io_scheduler, smart, write_journal.

use claudefs_storage::{
    atomic_write::{AtomicWriteCapability, AtomicWriteRequest, AtomicWriteStats},
    block::{BlockId, BlockRef, BlockSize},
    block_cache::{BlockCache, BlockCacheConfig, CacheStats},
    checksum::{compute, ChecksumAlgorithm},
    io_scheduler::{IoPriority, IoScheduler, IoSchedulerConfig, IoSchedulerStats, ScheduledIo},
    io_uring_bridge::{IoOpType, IoRequestId},
    smart::{
        AlertSeverity, HealthStatus, NvmeSmartLog, SmartAlert, SmartAttribute, SmartMonitor,
        SmartMonitorConfig,
    },
    write_journal::{JournalConfig, JournalEntry, JournalOp, JournalStats, SyncMode, WriteJournal},
};

#[cfg(test)]
mod tests {
    use super::*;

    // AtomicWriteCapability tests
    #[test]
    fn test_atomic_capability_unsupported_not_supported() {
        let cap = AtomicWriteCapability::unsupported();
        assert!(!cap.supported);
    }

    #[test]
    fn test_atomic_capability_unsupported_max_bytes_zero() {
        let cap = AtomicWriteCapability::unsupported();
        assert_eq!(cap.max_atomic_write_bytes, 0);
    }

    #[test]
    fn test_atomic_capability_can_atomic_write_when_unsupported() {
        let cap = AtomicWriteCapability::unsupported();
        assert!(!cap.can_atomic_write(100));
    }

    #[test]
    fn test_atomic_capability_can_atomic_write_zero() {
        let cap = AtomicWriteCapability {
            supported: true,
            max_atomic_write_bytes: 4096,
            alignment: 4096,
        };
        assert!(!cap.can_atomic_write(0));
    }

    #[test]
    fn test_atomic_capability_can_atomic_write_within_limit() {
        let cap = AtomicWriteCapability {
            supported: true,
            max_atomic_write_bytes: 4096,
            alignment: 4096,
        };
        assert!(cap.can_atomic_write(4096));
    }

    #[test]
    fn test_atomic_capability_can_atomic_write_over_limit() {
        let cap = AtomicWriteCapability {
            supported: true,
            max_atomic_write_bytes: 4096,
            alignment: 4096,
        };
        assert!(!cap.can_atomic_write(4097));
    }

    // AtomicWriteRequest tests
    #[test]
    fn test_atomic_request_new_size() {
        let block_id = BlockId::new(0, 0);
        let block_ref = BlockRef {
            id: block_id,
            size: BlockSize::B4K,
        };
        let data = b"hello world".to_vec();
        let request = AtomicWriteRequest::new(block_ref.clone(), data.clone(), false);
        assert_eq!(request.size() as usize, data.len());
    }

    #[test]
    fn test_atomic_request_new_checksum_computed() {
        let block_id = BlockId::new(0, 0);
        let block_ref = BlockRef {
            id: block_id,
            size: BlockSize::B4K,
        };
        let data = b"hello world".to_vec();
        let request = AtomicWriteRequest::new(block_ref, data, false);
        assert_ne!(request.checksum.value, 0);
    }

    #[test]
    fn test_atomic_request_fence_false() {
        let block_id = BlockId::new(0, 0);
        let block_ref = BlockRef {
            id: block_id,
            size: BlockSize::B4K,
        };
        let data = b"hello world".to_vec();
        let request = AtomicWriteRequest::new(block_ref, data, false);
        assert!(!request.fence);
    }

    #[test]
    fn test_atomic_request_fence_true() {
        let block_id = BlockId::new(0, 0);
        let block_ref = BlockRef {
            id: block_id,
            size: BlockSize::B4K,
        };
        let data = b"hello world".to_vec();
        let request = AtomicWriteRequest::new(block_ref, data, true);
        assert!(request.fence);
    }

    // AtomicWriteStats tests
    #[test]
    fn test_atomic_stats_initial_zero() {
        let stats = AtomicWriteStats::default();
        assert_eq!(stats.atomic_writes_submitted, 0);
        assert_eq!(stats.atomic_writes_completed, 0);
        assert_eq!(stats.atomic_writes_failed, 0);
        assert_eq!(stats.bytes_written_atomic, 0);
    }

    #[test]
    fn test_atomic_stats_submitted() {
        let mut stats = AtomicWriteStats::default();
        stats.submitted();
        assert_eq!(stats.atomic_writes_submitted, 1);
    }

    #[test]
    fn test_atomic_stats_completed() {
        let mut stats = AtomicWriteStats::default();
        stats.completed(100);
        assert_eq!(stats.atomic_writes_completed, 1);
        assert_eq!(stats.bytes_written_atomic, 100);
    }

    #[test]
    fn test_atomic_stats_failed() {
        let mut stats = AtomicWriteStats::default();
        stats.failed();
        assert_eq!(stats.atomic_writes_failed, 1);
    }

    // BlockCache tests
    #[test]
    fn test_block_cache_new_empty() {
        let cache = BlockCache::new(BlockCacheConfig::default());
        assert_eq!(cache.stats().hits, 0);
    }

    #[test]
    fn test_block_cache_stats_hit_rate_empty() {
        let stats = CacheStats::default();
        assert_eq!(stats.hit_rate(), 0.0);
    }

    #[test]
    fn test_block_cache_stats_hit_rate_all_hits() {
        let stats = CacheStats {
            hits: 10,
            misses: 0,
            ..Default::default()
        };
        assert_eq!(stats.hit_rate(), 1.0);
    }

    #[test]
    fn test_block_cache_stats_hit_rate_half() {
        let stats = CacheStats {
            hits: 5,
            misses: 5,
            ..Default::default()
        };
        assert!((stats.hit_rate() - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_block_cache_config_default_write_through() {
        let config = BlockCacheConfig::default();
        assert!(config.write_through);
    }

    #[test]
    fn test_block_cache_config_default_max_entries() {
        let config = BlockCacheConfig::default();
        assert_eq!(config.max_entries, 65536);
    }

    // IoPriority tests
    #[test]
    fn test_io_priority_critical_index() {
        assert_eq!(IoPriority::Critical.as_index(), 0);
    }

    #[test]
    fn test_io_priority_high_index() {
        assert_eq!(IoPriority::High.as_index(), 1);
    }

    #[test]
    fn test_io_priority_normal_index() {
        assert_eq!(IoPriority::Normal.as_index(), 2);
    }

    #[test]
    fn test_io_priority_low_index() {
        assert_eq!(IoPriority::Low.as_index(), 3);
    }

    #[test]
    fn test_io_priority_critical_is_high() {
        assert!(IoPriority::Critical.is_high());
    }

    #[test]
    fn test_io_priority_high_is_high() {
        assert!(IoPriority::High.is_high());
    }

    #[test]
    fn test_io_priority_normal_not_high() {
        assert!(!IoPriority::Normal.is_high());
    }

    #[test]
    fn test_io_priority_low_not_high() {
        assert!(!IoPriority::Low.is_high());
    }

    #[test]
    fn test_io_priority_ordering() {
        assert!(IoPriority::Critical < IoPriority::High);
        assert!(IoPriority::High < IoPriority::Normal);
        assert!(IoPriority::Normal < IoPriority::Low);
    }

    // NvmeSmartLog tests
    #[test]
    fn test_smart_log_temperature_celsius() {
        let log = NvmeSmartLog {
            critical_warning: 0,
            temperature_kelvin: 373,
            available_spare_pct: 50,
            available_spare_threshold: 10,
            percent_used: 50,
            data_units_read: 0,
            data_units_written: 0,
            host_read_commands: 0,
            host_write_commands: 0,
            power_on_hours: 0,
            unsafe_shutdowns: 0,
            media_errors: 0,
            error_log_entries: 0,
        };
        assert!((log.temperature_celsius() - 99.85).abs() < 0.1);
    }

    #[test]
    fn test_smart_log_is_critical_no_warning() {
        let log = NvmeSmartLog {
            critical_warning: 0,
            temperature_kelvin: 373,
            available_spare_pct: 50,
            available_spare_threshold: 10,
            percent_used: 50,
            data_units_read: 0,
            data_units_written: 0,
            host_read_commands: 0,
            host_write_commands: 0,
            power_on_hours: 0,
            unsafe_shutdowns: 0,
            media_errors: 0,
            error_log_entries: 0,
        };
        assert!(!log.is_critical());
    }

    #[test]
    fn test_smart_log_is_critical_with_warning() {
        let log = NvmeSmartLog {
            critical_warning: 1,
            temperature_kelvin: 373,
            available_spare_pct: 50,
            available_spare_threshold: 10,
            percent_used: 50,
            data_units_read: 0,
            data_units_written: 0,
            host_read_commands: 0,
            host_write_commands: 0,
            power_on_hours: 0,
            unsafe_shutdowns: 0,
            media_errors: 0,
            error_log_entries: 0,
        };
        assert!(log.is_critical());
    }

    #[test]
    fn test_smart_log_spare_ok_above_threshold() {
        let log = NvmeSmartLog {
            critical_warning: 0,
            temperature_kelvin: 373,
            available_spare_pct: 50,
            available_spare_threshold: 10,
            percent_used: 50,
            data_units_read: 0,
            data_units_written: 0,
            host_read_commands: 0,
            host_write_commands: 0,
            power_on_hours: 0,
            unsafe_shutdowns: 0,
            media_errors: 0,
            error_log_entries: 0,
        };
        assert!(log.spare_ok());
    }

    #[test]
    fn test_smart_log_spare_ok_below_threshold() {
        let log = NvmeSmartLog {
            critical_warning: 0,
            temperature_kelvin: 373,
            available_spare_pct: 5,
            available_spare_threshold: 10,
            percent_used: 50,
            data_units_read: 0,
            data_units_written: 0,
            host_read_commands: 0,
            host_write_commands: 0,
            power_on_hours: 0,
            unsafe_shutdowns: 0,
            media_errors: 0,
            error_log_entries: 0,
        };
        assert!(!log.spare_ok());
    }

    #[test]
    fn test_smart_log_endurance_ok_below_100() {
        let log = NvmeSmartLog {
            critical_warning: 0,
            temperature_kelvin: 373,
            available_spare_pct: 50,
            available_spare_threshold: 10,
            percent_used: 50,
            data_units_read: 0,
            data_units_written: 0,
            host_read_commands: 0,
            host_write_commands: 0,
            power_on_hours: 0,
            unsafe_shutdowns: 0,
            media_errors: 0,
            error_log_entries: 0,
        };
        assert!(log.endurance_ok());
    }

    #[test]
    fn test_smart_log_endurance_not_ok_at_100() {
        let log = NvmeSmartLog {
            critical_warning: 0,
            temperature_kelvin: 373,
            available_spare_pct: 50,
            available_spare_threshold: 10,
            percent_used: 100,
            data_units_read: 0,
            data_units_written: 0,
            host_read_commands: 0,
            host_write_commands: 0,
            power_on_hours: 0,
            unsafe_shutdowns: 0,
            media_errors: 0,
            error_log_entries: 0,
        };
        assert!(!log.endurance_ok());
    }

    // WriteJournal / JournalConfig tests
    #[test]
    fn test_journal_config_default_sync_mode() {
        let config = JournalConfig::default();
        assert_eq!(config.sync_mode, SyncMode::BatchSync);
    }

    #[test]
    fn test_journal_config_default_max_size() {
        let config = JournalConfig::default();
        assert_eq!(config.max_journal_size, 256 * 1024 * 1024);
    }

    #[test]
    fn test_journal_op_write_data_len() {
        let op = JournalOp::Write {
            data: vec![1, 2, 3],
        };
        assert_eq!(op.data_len(), 3);
    }

    #[test]
    fn test_journal_op_delete_data_len() {
        let op = JournalOp::Delete;
        assert_eq!(op.data_len(), 0);
    }

    #[test]
    fn test_journal_op_fsync_data_len() {
        let op = JournalOp::Fsync;
        assert_eq!(op.data_len(), 0);
    }

    #[test]
    fn test_journal_stats_default_zero() {
        let stats = JournalStats::default();
        assert_eq!(stats.entries_appended, 0);
        assert_eq!(stats.entries_committed, 0);
        assert_eq!(stats.entries_truncated, 0);
        assert_eq!(stats.bytes_written, 0);
        assert_eq!(stats.commits, 0);
    }

    // Additional SmartMonitor tests
    #[test]
    fn test_smart_monitor_config_default() {
        let config = SmartMonitorConfig::default();
        assert_eq!(config.poll_interval_secs, 60);
    }

    #[test]
    fn test_smart_monitor_new() {
        let monitor = SmartMonitor::new(SmartMonitorConfig::default());
        assert_eq!(monitor.device_count(), 0);
    }

    #[test]
    fn test_alert_severity_info() {
        assert!(matches!(AlertSeverity::Info, AlertSeverity::Info));
    }

    #[test]
    fn test_alert_severity_warning() {
        assert!(matches!(AlertSeverity::Warning, AlertSeverity::Warning));
    }

    #[test]
    fn test_alert_severity_critical() {
        assert!(matches!(AlertSeverity::Critical, AlertSeverity::Critical));
    }

    // IoScheduler tests
    #[test]
    fn test_io_scheduler_config_default() {
        let config = IoSchedulerConfig::default();
        assert_eq!(config.max_queue_depth, 1024);
    }

    #[test]
    fn test_io_scheduler_new() {
        let scheduler = IoScheduler::new(IoSchedulerConfig::default());
        assert!(scheduler.is_empty());
    }

    #[test]
    fn test_io_scheduler_stats_default() {
        let stats = IoSchedulerStats::default();
        assert_eq!(stats.enqueued, 0);
        assert_eq!(stats.dequeued, 0);
    }

    #[test]
    fn test_scheduled_io_new() {
        let block_id = BlockId::new(0, 0);
        let block_ref = BlockRef {
            id: block_id,
            size: BlockSize::B4K,
        };
        let io = ScheduledIo::new(
            IoRequestId(1),
            IoPriority::High,
            IoOpType::Read,
            block_ref,
            1000,
        );
        assert_eq!(io.priority, IoPriority::High);
    }

    // BlockRef tests
    #[test]
    fn test_block_ref_new() {
        let block_id = BlockId::new(0, 100);
        let block_ref = BlockRef {
            id: block_id,
            size: BlockSize::B4K,
        };
        assert_eq!(block_ref.id.offset, 100);
    }

    // Checksum tests
    #[test]
    fn test_checksum_compute() {
        let data = b"hello world";
        let checksum = compute(ChecksumAlgorithm::Crc32c, data);
        assert_ne!(checksum.value, 0);
    }

    #[test]
    fn test_checksum_algorithm_crc32c() {
        let algo = ChecksumAlgorithm::Crc32c;
        assert!(matches!(algo, ChecksumAlgorithm::Crc32c));
    }
}
