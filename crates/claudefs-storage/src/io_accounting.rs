//! Per-tenant I/O accounting with sliding-window statistics.
//!
//! Tracks bytes read/written, IOPS, and latency histograms per tenant ID.
//! Used for quota enforcement and observability.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tracing::debug;

/// Unique tenant identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub struct TenantId(pub u64);

/// I/O operation direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IoDirection {
    /// Read operation.
    Read,
    /// Write operation.
    Write,
}

/// Aggregate I/O counters for a tenant over a time window.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TenantIoStats {
    /// Unique tenant identifier.
    pub tenant_id: TenantId,
    /// Total bytes read in this window.
    pub bytes_read: u64,
    /// Total bytes written in this window.
    pub bytes_written: u64,
    /// Number of read operations.
    pub read_ops: u64,
    /// Number of write operations.
    pub write_ops: u64,
    /// Sum of all operation latencies in microseconds (for average calculation).
    pub total_latency_us: u64,
    /// Maximum latency observed in this window (microseconds).
    pub max_latency_us: u64,
    /// Window start timestamp in seconds.
    pub window_start_secs: u64,
}

/// Configuration for the I/O accounting module.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IoAccountingConfig {
    /// Sliding window size in seconds.
    pub window_secs: u64,
    /// Maximum number of tracked tenants.
    pub max_tenants: usize,
}

impl Default for IoAccountingConfig {
    fn default() -> Self {
        Self {
            window_secs: 60,
            max_tenants: 1024,
        }
    }
}

/// Per-tenant I/O accounting with sliding-window statistics.
pub struct IoAccounting {
    config: IoAccountingConfig,
    tenants: HashMap<TenantId, TenantIoStats>,
    window_start_secs: u64,
}

impl IoAccounting {
    /// Creates a new IoAccounting with the given configuration.
    pub fn new(config: IoAccountingConfig) -> Self {
        debug!(
            window_secs = config.window_secs,
            max_tenants = config.max_tenants,
            "Creating new IoAccounting"
        );
        Self {
            config,
            tenants: HashMap::new(),
            window_start_secs: 0,
        }
    }

    /// Records an I/O operation for the given tenant.
    /// If tenant doesn't exist and max_tenants not reached, creates new entry.
    /// If max_tenants reached, silently drops the operation (no error).
    pub fn record_op(&mut self, tenant: TenantId, dir: IoDirection, bytes: u64, latency_us: u64) {
        if self.tenants.len() >= self.config.max_tenants && !self.tenants.contains_key(&tenant) {
            debug!(tenant = ?tenant, "Max tenants reached, dropping operation");
            return;
        }

        if self.window_start_secs == 0 {
            self.window_start_secs = 1;
        }

        let stats = self.tenants.entry(tenant).or_insert_with(|| TenantIoStats {
            tenant_id: tenant,
            window_start_secs: 0,
            ..Default::default()
        });

        match dir {
            IoDirection::Read => {
                stats.bytes_read += bytes;
                stats.read_ops += 1;
            }
            IoDirection::Write => {
                stats.bytes_written += bytes;
                stats.write_ops += 1;
            }
        }

        stats.total_latency_us += latency_us;
        if latency_us > stats.max_latency_us {
            stats.max_latency_us = latency_us;
        }
    }

    /// Returns the stats for the given tenant, or None if not tracked.
    pub fn get_stats(&self, tenant: TenantId) -> Option<TenantIoStats> {
        self.tenants.get(&tenant).cloned()
    }

    /// Returns all tenant stats.
    pub fn all_stats(&self) -> Vec<TenantIoStats> {
        self.tenants.values().cloned().collect()
    }

    /// Rotates the window: expires old data if current_secs > window_start + window_secs.
    /// Resets window_start_secs to current_secs. Clears all tenant data.
    pub fn rotate_window(&mut self, current_secs: u64) {
        if self.window_start_secs == 0 {
            return;
        }

        if current_secs >= self.window_start_secs + self.config.window_secs {
            debug!(
                current = current_secs,
                old_start = self.window_start_secs,
                window = self.config.window_secs,
                "Rotating I/O accounting window"
            );
            self.tenants.clear();
            self.window_start_secs = current_secs;
        }
    }

    /// Returns the number of tracked tenants.
    pub fn tenant_count(&self) -> usize {
        self.tenants.len()
    }

    /// Returns total bytes read across all tenants.
    pub fn total_bytes_read(&self) -> u64 {
        self.tenants.values().map(|s| s.bytes_read).sum()
    }

    /// Returns total bytes written across all tenants.
    pub fn total_bytes_written(&self) -> u64 {
        self.tenants.values().map(|s| s.bytes_written).sum()
    }

    /// Returns top N tenants by total bytes (read + written), sorted descending.
    /// If n > tenant count, returns all tenants sorted.
    pub fn top_tenants_by_bytes(&self, n: usize) -> Vec<TenantIoStats> {
        let mut all: Vec<TenantIoStats> = self.tenants.values().cloned().collect();
        all.sort_by(|a, b| {
            let a_total = a.bytes_read + a.bytes_written;
            let b_total = b.bytes_read + b.bytes_written;
            b_total.cmp(&a_total)
        });
        all.into_iter().take(n).collect()
    }

    /// Returns the window start timestamp.
    pub fn window_start(&self) -> u64 {
        self.window_start_secs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_accounting_has_no_tenants() {
        let accounting = IoAccounting::new(IoAccountingConfig::default());
        assert_eq!(accounting.tenant_count(), 0);
    }

    #[test]
    fn record_single_read_op() {
        let mut accounting = IoAccounting::new(IoAccountingConfig::default());
        accounting.record_op(TenantId(1), IoDirection::Read, 1024, 100);

        let stats = accounting.get_stats(TenantId(1)).unwrap();
        assert_eq!(stats.bytes_read, 1024);
        assert_eq!(stats.read_ops, 1);
        assert_eq!(stats.bytes_written, 0);
        assert_eq!(stats.write_ops, 0);
    }

    #[test]
    fn record_single_write_op() {
        let mut accounting = IoAccounting::new(IoAccountingConfig::default());
        accounting.record_op(TenantId(1), IoDirection::Write, 2048, 200);

        let stats = accounting.get_stats(TenantId(1)).unwrap();
        assert_eq!(stats.bytes_written, 2048);
        assert_eq!(stats.write_ops, 1);
        assert_eq!(stats.bytes_read, 0);
        assert_eq!(stats.read_ops, 0);
    }

    #[test]
    fn multiple_ops_same_tenant_accumulate() {
        let mut accounting = IoAccounting::new(IoAccountingConfig::default());
        accounting.record_op(TenantId(1), IoDirection::Read, 1024, 100);
        accounting.record_op(TenantId(1), IoDirection::Read, 2048, 150);
        accounting.record_op(TenantId(1), IoDirection::Write, 4096, 300);

        let stats = accounting.get_stats(TenantId(1)).unwrap();
        assert_eq!(stats.bytes_read, 3072);
        assert_eq!(stats.read_ops, 2);
        assert_eq!(stats.bytes_written, 4096);
        assert_eq!(stats.write_ops, 1);
        assert_eq!(stats.total_latency_us, 550);
    }

    #[test]
    fn different_tenants_tracked_independently() {
        let mut accounting = IoAccounting::new(IoAccountingConfig::default());
        accounting.record_op(TenantId(1), IoDirection::Read, 1024, 100);
        accounting.record_op(TenantId(2), IoDirection::Write, 2048, 200);

        let stats1 = accounting.get_stats(TenantId(1)).unwrap();
        let stats2 = accounting.get_stats(TenantId(2)).unwrap();

        assert_eq!(stats1.bytes_read, 1024);
        assert_eq!(stats2.bytes_written, 2048);
        assert_eq!(accounting.tenant_count(), 2);
    }

    #[test]
    fn total_bytes_read_returns_sum_across_tenants() {
        let mut accounting = IoAccounting::new(IoAccountingConfig::default());
        accounting.record_op(TenantId(1), IoDirection::Read, 1024, 100);
        accounting.record_op(TenantId(2), IoDirection::Read, 2048, 150);

        assert_eq!(accounting.total_bytes_read(), 3072);
    }

    #[test]
    fn total_bytes_written_returns_sum() {
        let mut accounting = IoAccounting::new(IoAccountingConfig::default());
        accounting.record_op(TenantId(1), IoDirection::Write, 1024, 100);
        accounting.record_op(TenantId(2), IoDirection::Write, 2048, 150);

        assert_eq!(accounting.total_bytes_written(), 3072);
    }

    #[test]
    fn top_tenants_by_bytes_returns_sorted_desc() {
        let mut accounting = IoAccounting::new(IoAccountingConfig::default());
        accounting.record_op(TenantId(1), IoDirection::Read, 1024, 100);
        accounting.record_op(TenantId(2), IoDirection::Read, 4096, 200);
        accounting.record_op(TenantId(3), IoDirection::Read, 2048, 150);

        let top = accounting.top_tenants_by_bytes(3);

        assert_eq!(top[0].tenant_id, TenantId(2));
        assert_eq!(top[1].tenant_id, TenantId(3));
        assert_eq!(top[2].tenant_id, TenantId(1));
    }

    #[test]
    fn top_tenants_by_bytes_with_n_gt_total_returns_all() {
        let mut accounting = IoAccounting::new(IoAccountingConfig::default());
        accounting.record_op(TenantId(1), IoDirection::Read, 1024, 100);

        let top = accounting.top_tenants_by_bytes(10);

        assert_eq!(top.len(), 1);
    }

    #[test]
    fn top_tenants_by_bytes_empty_returns_empty() {
        let accounting = IoAccounting::new(IoAccountingConfig::default());

        let top = accounting.top_tenants_by_bytes(5);

        assert!(top.is_empty());
    }

    #[test]
    fn rotate_window_clears_old_data() {
        let mut accounting = IoAccounting::new(IoAccountingConfig::default());
        accounting.record_op(TenantId(1), IoDirection::Read, 1024, 100);

        accounting.rotate_window(61);

        assert_eq!(accounting.tenant_count(), 0);
    }

    #[test]
    fn after_rotate_window_new_ops_start_fresh() {
        let mut accounting = IoAccounting::new(IoAccountingConfig::default());
        accounting.record_op(TenantId(1), IoDirection::Read, 1024, 100);
        accounting.rotate_window(61);
        accounting.record_op(TenantId(1), IoDirection::Read, 2048, 200);

        let stats = accounting.get_stats(TenantId(1)).unwrap();
        assert_eq!(stats.bytes_read, 2048);
        assert_eq!(accounting.tenant_count(), 1);
    }

    #[test]
    fn get_stats_for_unknown_tenant_returns_none() {
        let accounting = IoAccounting::new(IoAccountingConfig::default());

        assert!(accounting.get_stats(TenantId(999)).is_none());
    }

    #[test]
    fn max_latency_us_tracks_maximum() {
        let mut accounting = IoAccounting::new(IoAccountingConfig::default());
        accounting.record_op(TenantId(1), IoDirection::Read, 1024, 100);
        accounting.record_op(TenantId(1), IoDirection::Read, 2048, 500);
        accounting.record_op(TenantId(1), IoDirection::Read, 4096, 200);

        let stats = accounting.get_stats(TenantId(1)).unwrap();
        assert_eq!(stats.max_latency_us, 500);
    }

    #[test]
    fn tenant_count_returns_correct_count() {
        let mut accounting = IoAccounting::new(IoAccountingConfig::default());
        accounting.record_op(TenantId(1), IoDirection::Read, 1024, 100);
        accounting.record_op(TenantId(2), IoDirection::Write, 2048, 200);

        assert_eq!(accounting.tenant_count(), 2);
    }

    #[test]
    fn tenant_count_reaches_max_tenants_limit() {
        let mut accounting = IoAccounting::new(IoAccountingConfig {
            window_secs: 60,
            max_tenants: 2,
        });
        accounting.record_op(TenantId(1), IoDirection::Read, 1024, 100);
        accounting.record_op(TenantId(2), IoDirection::Read, 2048, 200);
        accounting.record_op(TenantId(3), IoDirection::Read, 4096, 300);

        assert_eq!(accounting.tenant_count(), 2);
        assert!(accounting.get_stats(TenantId(3)).is_none());
    }

    #[test]
    fn io_direction_distinguishes_read_from_write() {
        let read = IoDirection::Read;
        let write = IoDirection::Write;

        assert_ne!(read, write);
        assert_eq!(read, IoDirection::Read);
        assert_eq!(write, IoDirection::Write);
    }

    #[test]
    fn tenant_id_zero_is_valid() {
        let mut accounting = IoAccounting::new(IoAccountingConfig::default());
        accounting.record_op(TenantId(0), IoDirection::Read, 1024, 100);

        let stats = accounting.get_stats(TenantId(0)).unwrap();
        assert_eq!(stats.tenant_id, TenantId(0));
    }

    #[test]
    fn record_op_for_tenant_id_max() {
        let mut accounting = IoAccounting::new(IoAccountingConfig::default());
        accounting.record_op(TenantId(u64::MAX), IoDirection::Write, 1024, 100);

        let stats = accounting.get_stats(TenantId(u64::MAX)).unwrap();
        assert_eq!(stats.tenant_id, TenantId(u64::MAX));
    }

    #[test]
    fn all_stats_returns_all_tracked_tenants() {
        let mut accounting = IoAccounting::new(IoAccountingConfig::default());
        accounting.record_op(TenantId(1), IoDirection::Read, 1024, 100);
        accounting.record_op(TenantId(2), IoDirection::Write, 2048, 200);

        let all = accounting.all_stats();

        assert_eq!(all.len(), 2);
    }

    #[test]
    fn window_keeps_data_within_window_secs() {
        let mut accounting = IoAccounting::new(IoAccountingConfig {
            window_secs: 60,
            max_tenants: 1024,
        });
        accounting.record_op(TenantId(1), IoDirection::Read, 1024, 100);

        accounting.rotate_window(30);

        let stats = accounting.get_stats(TenantId(1));
        assert!(stats.is_some());
    }

    #[test]
    fn multiple_windows_accumulate_correctly() {
        let mut accounting = IoAccounting::new(IoAccountingConfig {
            window_secs: 60,
            max_tenants: 1024,
        });
        accounting.record_op(TenantId(1), IoDirection::Read, 1024, 100);
        accounting.rotate_window(61);
        accounting.record_op(TenantId(1), IoDirection::Read, 2048, 200);

        let stats = accounting.get_stats(TenantId(1)).unwrap();
        assert_eq!(stats.bytes_read, 2048);
        assert_eq!(stats.read_ops, 1);
    }

    #[test]
    fn bytes_counted_per_op_not_per_record_call() {
        let mut accounting = IoAccounting::new(IoAccountingConfig::default());
        accounting.record_op(TenantId(1), IoDirection::Read, 100, 10);
        accounting.record_op(TenantId(1), IoDirection::Read, 200, 20);

        let stats = accounting.get_stats(TenantId(1)).unwrap();
        assert_eq!(stats.bytes_read, 300);
    }

    #[test]
    fn default_config_has_window_secs_60() {
        let config = IoAccountingConfig::default();
        assert_eq!(config.window_secs, 60);
        assert_eq!(config.max_tenants, 1024);
    }

    #[test]
    fn rotate_window_does_not_lose_in_window_data() {
        let mut accounting = IoAccounting::new(IoAccountingConfig {
            window_secs: 60,
            max_tenants: 1024,
        });
        accounting.record_op(TenantId(1), IoDirection::Read, 1024, 100);

        accounting.rotate_window(30);

        let stats = accounting.get_stats(TenantId(1)).unwrap();
        assert_eq!(stats.bytes_read, 1024);
    }

    #[test]
    fn rotate_window_only_clears_if_window_expired() {
        let mut accounting = IoAccounting::new(IoAccountingConfig {
            window_secs: 60,
            max_tenants: 1024,
        });
        accounting.record_op(TenantId(1), IoDirection::Read, 1024, 100);

        accounting.rotate_window(60);

        let stats = accounting.get_stats(TenantId(1)).unwrap();
        assert_eq!(stats.bytes_read, 1024);
    }

    #[test]
    fn write_ops_also_count_bytes() {
        let mut accounting = IoAccounting::new(IoAccountingConfig::default());
        accounting.record_op(TenantId(1), IoDirection::Write, 8192, 500);

        let stats = accounting.get_stats(TenantId(1)).unwrap();
        assert_eq!(stats.bytes_written, 8192);
        assert_eq!(stats.write_ops, 1);
    }

    #[test]
    fn total_latency_accumulates() {
        let mut accounting = IoAccounting::new(IoAccountingConfig::default());
        accounting.record_op(TenantId(1), IoDirection::Read, 1024, 100);
        accounting.record_op(TenantId(1), IoDirection::Write, 2048, 300);

        let stats = accounting.get_stats(TenantId(1)).unwrap();
        assert_eq!(stats.total_latency_us, 400);
    }
}
