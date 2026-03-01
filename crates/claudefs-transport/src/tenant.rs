//! Multi-tenant traffic isolation module for ClaudeFS transport layer.
//!
//! Provides per-tenant bandwidth and IOPS guarantees with configurable burst allowances.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Unique identifier for a tenant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TenantId(pub u64);

/// Configuration for a tenant's traffic limits.
#[derive(Debug, Clone)]
pub struct TenantConfig {
    /// The tenant's unique identifier.
    pub tenant_id: TenantId,
    /// The tenant's name.
    pub name: String,
    /// Minimum guaranteed bandwidth in bytes per second.
    pub min_bandwidth_bytes_sec: u64,
    /// Maximum allowed bandwidth in bytes per second.
    pub max_bandwidth_bytes_sec: u64,
    /// Minimum guaranteed IOPS.
    pub min_iops: u64,
    /// Maximum allowed IOPS.
    pub max_iops: u64,
    /// Weight for fair scheduling.
    pub weight: u32,
}

impl TenantConfig {
    /// Creates a new TenantConfig with default values.
    pub fn new(tenant_id: TenantId, name: impl Into<String>) -> Self {
        Self {
            tenant_id,
            name: name.into(),
            min_bandwidth_bytes_sec: 10_485_760,
            max_bandwidth_bytes_sec: 104_857_600,
            min_iops: 1000,
            max_iops: 10000,
            weight: 100,
        }
    }
}

impl Default for TenantConfig {
    fn default() -> Self {
        Self::new(TenantId(0), "default")
    }
}

/// Tracks current usage for a single tenant.
pub struct TenantTracker {
    config: TenantConfig,
    bytes_this_window: AtomicU64,
    ops_this_window: AtomicU64,
    total_bytes: AtomicU64,
    total_ops: AtomicU64,
    total_throttled: AtomicU64,
    window_start: AtomicU64,
}

impl TenantTracker {
    /// Creates a new TenantTracker with the given configuration.
    pub fn new(config: TenantConfig) -> Self {
        Self {
            config,
            bytes_this_window: AtomicU64::new(0),
            ops_this_window: AtomicU64::new(0),
            total_bytes: AtomicU64::new(0),
            total_ops: AtomicU64::new(0),
            total_throttled: AtomicU64::new(0),
            window_start: AtomicU64::new(Self::now_millis()),
        }
    }

    fn now_millis() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    fn check_and_advance_window(&self) {
        let now = Self::now_millis();
        let start = self.window_start.load(Ordering::Relaxed);
        if now.saturating_sub(start) >= 1000
            && self
                .window_start
                .compare_exchange(start, now, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
        {
            self.bytes_this_window.store(0, Ordering::Relaxed);
            self.ops_this_window.store(0, Ordering::Relaxed);
        }
    }

    /// Records I/O operations for this tenant.
    pub fn record_io(&self, bytes: u64) {
        self.check_and_advance_window();
        self.bytes_this_window.fetch_add(bytes, Ordering::Relaxed);
        self.ops_this_window.fetch_add(1, Ordering::Relaxed);
        self.total_bytes.fetch_add(bytes, Ordering::Relaxed);
        self.total_ops.fetch_add(1, Ordering::Relaxed);
    }

    /// Attempts to admit a new I/O request.
    pub fn try_admit(&self, bytes: u64) -> bool {
        self.check_and_advance_window();
        let current_bytes = self.bytes_this_window.load(Ordering::Relaxed);
        let current_ops = self.ops_this_window.load(Ordering::Relaxed);

        if current_bytes.saturating_add(bytes) > self.config.max_bandwidth_bytes_sec
            || current_ops.saturating_add(1) > self.config.max_iops
        {
            self.total_throttled.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        self.bytes_this_window.fetch_add(bytes, Ordering::Relaxed);
        self.ops_this_window.fetch_add(1, Ordering::Relaxed);
        self.total_bytes.fetch_add(bytes, Ordering::Relaxed);
        self.total_ops.fetch_add(1, Ordering::Relaxed);
        true
    }

    /// Returns the current bandwidth usage in this window.
    pub fn current_bandwidth(&self) -> u64 {
        self.check_and_advance_window();
        self.bytes_this_window.load(Ordering::Relaxed)
    }

    /// Returns the current IOPS in this window.
    pub fn current_iops(&self) -> u64 {
        self.check_and_advance_window();
        self.ops_this_window.load(Ordering::Relaxed)
    }

    /// Returns true if the tenant is currently throttled.
    pub fn is_throttled(&self) -> bool {
        self.current_bandwidth() >= self.config.max_bandwidth_bytes_sec
            || self.current_iops() >= self.config.max_iops
    }

    /// Returns a snapshot of tenant statistics.
    pub fn stats(&self) -> TenantStats {
        TenantStats {
            tenant_id: self.config.tenant_id,
            name: self.config.name.clone(),
            current_bandwidth: self.current_bandwidth(),
            current_iops: self.current_iops(),
            total_bytes: self.total_bytes.load(Ordering::Relaxed),
            total_ops: self.total_ops.load(Ordering::Relaxed),
            total_throttled: self.total_throttled.load(Ordering::Relaxed),
            is_throttled: self.is_throttled(),
        }
    }

    /// Resets the current window statistics.
    pub fn reset(&self) {
        self.bytes_this_window.store(0, Ordering::Relaxed);
        self.ops_this_window.store(0, Ordering::Relaxed);
        self.window_start
            .store(Self::now_millis(), Ordering::Relaxed);
    }
}

/// Statistics snapshot for a tenant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenantStats {
    /// The tenant's identifier.
    pub tenant_id: TenantId,
    /// The tenant's name.
    pub name: String,
    /// Current bandwidth usage.
    pub current_bandwidth: u64,
    /// Current IOPS.
    pub current_iops: u64,
    /// Total bytes transferred.
    pub total_bytes: u64,
    /// Total operations performed.
    pub total_ops: u64,
    /// Total number of throttled requests.
    pub total_throttled: u64,
    /// Whether the tenant is currently throttled.
    pub is_throttled: bool,
}

/// Result of attempting to admit a tenant request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TenantAdmitResult {
    /// Request was admitted.
    Admitted,
    /// Request was throttled due to limits.
    Throttled,
    /// Tenant is not known.
    UnknownTenant,
}

/// Manages multiple tenants with traffic isolation.
pub struct TenantManager {
    tenants: Mutex<HashMap<TenantId, Arc<TenantTracker>>>,
}

impl TenantManager {
    /// Creates a new TenantManager.
    pub fn new() -> Self {
        Self {
            tenants: Mutex::new(HashMap::new()),
        }
    }

    /// Adds a new tenant to the manager.
    pub fn add_tenant(&self, config: TenantConfig) {
        let mut tenants = self.tenants.lock().unwrap();
        tenants.insert(config.tenant_id, Arc::new(TenantTracker::new(config)));
    }

    /// Removes a tenant from the manager.
    pub fn remove_tenant(&self, tenant_id: TenantId) {
        let mut tenants = self.tenants.lock().unwrap();
        tenants.remove(&tenant_id);
    }

    /// Attempts to admit a request for a tenant.
    pub fn try_admit(&self, tenant_id: TenantId, bytes: u64) -> TenantAdmitResult {
        let tenants = self.tenants.lock().unwrap();
        match tenants.get(&tenant_id) {
            Some(tracker) => {
                if tracker.try_admit(bytes) {
                    TenantAdmitResult::Admitted
                } else {
                    TenantAdmitResult::Throttled
                }
            }
            None => TenantAdmitResult::UnknownTenant,
        }
    }

    /// Gets statistics for a specific tenant.
    pub fn get_stats(&self, tenant_id: TenantId) -> Option<TenantStats> {
        let tenants = self.tenants.lock().unwrap();
        tenants.get(&tenant_id).map(|t| t.stats())
    }

    /// Gets statistics for all tenants.
    pub fn all_stats(&self) -> Vec<TenantStats> {
        let tenants = self.tenants.lock().unwrap();
        tenants.values().map(|t| t.stats()).collect()
    }

    /// Returns the number of tenants.
    pub fn tenant_count(&self) -> usize {
        let tenants = self.tenants.lock().unwrap();
        tenants.len()
    }

    /// Returns the total bandwidth across all tenants.
    pub fn total_bandwidth(&self) -> u64 {
        let tenants = self.tenants.lock().unwrap();
        tenants.values().map(|t| t.current_bandwidth()).sum()
    }
}

impl Default for TenantManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tenant_id() {
        let id1 = TenantId(42);
        let id2 = TenantId(42);
        let id3 = TenantId(100);
        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
        assert_eq!(format!("{:?}", id1), "TenantId(42)");
    }

    #[test]
    fn test_tenant_config_defaults() {
        let config = TenantConfig::default();
        assert_eq!(config.tenant_id, TenantId(0));
        assert_eq!(config.name, "default");
        assert_eq!(config.min_bandwidth_bytes_sec, 10_485_760);
        assert_eq!(config.max_bandwidth_bytes_sec, 104_857_600);
        assert_eq!(config.min_iops, 1000);
        assert_eq!(config.max_iops, 10000);
        assert_eq!(config.weight, 100);
    }

    #[test]
    fn test_tenant_tracker_new() {
        let config = TenantConfig::new(TenantId(1), "test-tenant");
        let tracker = TenantTracker::new(config);
        assert_eq!(tracker.current_bandwidth(), 0);
        assert_eq!(tracker.current_iops(), 0);
    }

    #[test]
    fn test_record_io() {
        let config = TenantConfig::new(TenantId(1), "test");
        let tracker = TenantTracker::new(config);
        tracker.record_io(4096);
        assert_eq!(tracker.current_bandwidth(), 4096);
        assert_eq!(tracker.current_iops(), 1);
        tracker.record_io(8192);
        assert_eq!(tracker.current_bandwidth(), 12288);
        assert_eq!(tracker.current_iops(), 2);
    }

    #[test]
    fn test_try_admit_below_limits() {
        let config = TenantConfig {
            tenant_id: TenantId(1),
            name: "test".to_string(),
            min_bandwidth_bytes_sec: 10_485_760,
            max_bandwidth_bytes_sec: 100_000_000,
            min_iops: 1000,
            max_iops: 100_000,
            weight: 100,
        };
        let tracker = TenantTracker::new(config);
        assert!(tracker.try_admit(4096));
    }

    #[test]
    fn test_try_admit_above_bandwidth() {
        let config = TenantConfig {
            tenant_id: TenantId(1),
            name: "test".to_string(),
            min_bandwidth_bytes_sec: 10_485_760,
            max_bandwidth_bytes_sec: 1000,
            min_iops: 1000,
            max_iops: 10000,
            weight: 100,
        };
        let tracker = TenantTracker::new(config);
        assert!(tracker.try_admit(500));
        assert!(tracker.try_admit(500));
        assert!(!tracker.try_admit(1));
    }

    #[test]
    fn test_try_admit_above_iops() {
        let config = TenantConfig {
            tenant_id: TenantId(1),
            name: "test".to_string(),
            min_bandwidth_bytes_sec: 10_485_760,
            max_bandwidth_bytes_sec: 104_857_600,
            min_iops: 1000,
            max_iops: 5,
            weight: 100,
        };
        let tracker = TenantTracker::new(config);
        for _ in 0..5 {
            assert!(tracker.try_admit(4096));
        }
        assert!(!tracker.try_admit(4096));
    }

    #[test]
    fn test_is_throttled_false_initially() {
        let config = TenantConfig::new(TenantId(1), "test");
        let tracker = TenantTracker::new(config);
        assert!(!tracker.is_throttled());
    }

    #[test]
    fn test_is_throttled_true_when_over() {
        let config = TenantConfig {
            tenant_id: TenantId(1),
            name: "test".to_string(),
            min_bandwidth_bytes_sec: 10_485_760,
            max_bandwidth_bytes_sec: 1000,
            min_iops: 1000,
            max_iops: 10000,
            weight: 100,
        };
        let tracker = TenantTracker::new(config);
        tracker.record_io(2000);
        assert!(tracker.is_throttled());
    }

    #[test]
    fn test_stats_snapshot() {
        let config = TenantConfig::new(TenantId(42), "test-tenant");
        let tracker = TenantTracker::new(config);
        tracker.record_io(4096);
        let stats = tracker.stats();
        assert_eq!(stats.tenant_id, TenantId(42));
        assert_eq!(stats.name, "test-tenant");
        assert_eq!(stats.current_bandwidth, 4096);
        assert_eq!(stats.current_iops, 1);
        assert_eq!(stats.total_bytes, 4096);
        assert_eq!(stats.total_ops, 1);
    }

    #[test]
    fn test_reset() {
        let config = TenantConfig::new(TenantId(1), "test");
        let tracker = TenantTracker::new(config);
        tracker.record_io(4096);
        assert_eq!(tracker.current_bandwidth(), 4096);
        tracker.reset();
        assert_eq!(tracker.current_bandwidth(), 0);
    }

    #[test]
    fn test_manager_new() {
        let manager = TenantManager::new();
        assert_eq!(manager.tenant_count(), 0);
    }

    #[test]
    fn test_manager_add_tenant() {
        let manager = TenantManager::new();
        let config = TenantConfig::new(TenantId(1), "tenant1");
        manager.add_tenant(config);
        assert_eq!(manager.tenant_count(), 1);
    }

    #[test]
    fn test_manager_remove_tenant() {
        let manager = TenantManager::new();
        let config = TenantConfig::new(TenantId(1), "tenant1");
        manager.add_tenant(config);
        assert_eq!(manager.tenant_count(), 1);
        manager.remove_tenant(TenantId(1));
        assert_eq!(manager.tenant_count(), 0);
    }

    #[test]
    fn test_manager_try_admit_known() {
        let manager = TenantManager::new();
        let config = TenantConfig::new(TenantId(1), "tenant1");
        manager.add_tenant(config);
        let result = manager.try_admit(TenantId(1), 4096);
        assert_eq!(result, TenantAdmitResult::Admitted);
    }

    #[test]
    fn test_manager_try_admit_unknown() {
        let manager = TenantManager::new();
        let result = manager.try_admit(TenantId(999), 4096);
        assert_eq!(result, TenantAdmitResult::UnknownTenant);
    }

    #[test]
    fn test_manager_all_stats() {
        let manager = TenantManager::new();
        let config1 = TenantConfig::new(TenantId(1), "tenant1");
        let config2 = TenantConfig::new(TenantId(2), "tenant2");
        manager.add_tenant(config1);
        manager.add_tenant(config2);
        let stats = manager.all_stats();
        assert_eq!(stats.len(), 2);
    }

    #[test]
    fn test_manager_total_bandwidth() {
        let manager = TenantManager::new();
        let config = TenantConfig::new(TenantId(1), "tenant1");
        manager.add_tenant(config);
        manager.try_admit(TenantId(1), 4096);
        let total = manager.total_bandwidth();
        assert_eq!(total, 4096);
    }

    #[test]
    fn test_tenant_weight() {
        let mut config = TenantConfig::new(TenantId(1), "test");
        config.weight = 200;
        assert_eq!(config.weight, 200);
        let tracker = TenantTracker::new(config.clone());
        let stats = tracker.stats();
        assert_eq!(stats.name, "test");
    }

    #[test]
    fn test_try_admit_throttled_counts() {
        let config = TenantConfig {
            tenant_id: TenantId(1),
            name: "test".to_string(),
            min_bandwidth_bytes_sec: 10_485_760,
            max_bandwidth_bytes_sec: 1000,
            min_iops: 1000,
            max_iops: 10000,
            weight: 100,
        };
        let tracker = TenantTracker::new(config);
        let _ = tracker.try_admit(500);
        let _ = tracker.try_admit(500);
        let _ = tracker.try_admit(1);
        let stats = tracker.stats();
        assert_eq!(stats.total_throttled, 1);
    }
}
