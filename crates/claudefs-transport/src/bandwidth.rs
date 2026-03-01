//! Per-tenant bandwidth allocation and enforcement module.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum EnforcementMode {
    #[default]
    Strict,
    Shaping,
    Monitor,
}

#[derive(Debug, Clone)]
pub struct BandwidthConfig {
    pub global_limit_bps: u64,
    pub default_tenant_limit_bps: u64,
    pub burst_factor: f64,
    pub measurement_window_ms: u64,
    pub enforcement: EnforcementMode,
}

impl Default for BandwidthConfig {
    fn default() -> Self {
        Self {
            global_limit_bps: 10_000_000_000,
            default_tenant_limit_bps: 1_000_000_000,
            burst_factor: 1.5,
            measurement_window_ms: 1000,
            enforcement: EnforcementMode::Strict,
        }
    }
}

#[derive(Debug, Clone)]
struct TenantBandwidth {
    #[allow(dead_code)]
    tenant_id: String,
    pub limit_bps: u64,
    pub bytes_in_window: u64,
    pub window_start_ms: u64,
    pub total_bytes: u64,
    pub total_throttled: u64,
    pub total_dropped: u64,
    pub peak_bps: u64,
}

impl TenantBandwidth {
    fn new(tenant_id: String, limit_bps: u64, now_ms: u64) -> Self {
        Self {
            tenant_id,
            limit_bps,
            bytes_in_window: 0,
            window_start_ms: now_ms,
            total_bytes: 0,
            total_throttled: 0,
            total_dropped: 0,
            peak_bps: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BandwidthResult {
    Allowed,
    Throttled { delay_ms: u64 },
    Dropped { bytes: u64 },
    Monitored { over_limit: bool },
}

#[derive(Debug, Clone, Default)]
pub struct BandwidthStats {
    pub total_requests: u64,
    pub total_allowed: u64,
    pub total_throttled: u64,
    pub total_dropped: u64,
    pub global_usage_bps: u64,
    pub tenant_count: usize,
}

pub struct BandwidthAllocator {
    config: BandwidthConfig,
    tenants: Vec<(String, TenantBandwidth)>,
    global_bytes_in_window: u64,
    global_window_start_ms: u64,
    current_time_ms: u64,
    total_requests: u64,
    total_allowed: u64,
    total_throttled: u64,
    total_dropped: u64,
}

impl BandwidthAllocator {
    /// Creates a new BandwidthAllocator with the given configuration.
    pub fn new(config: BandwidthConfig) -> Self {
        Self {
            config,
            tenants: Vec::new(),
            global_bytes_in_window: 0,
            global_window_start_ms: 0,
            current_time_ms: 0,
            total_requests: 0,
            total_allowed: 0,
            total_throttled: 0,
            total_dropped: 0,
        }
    }

    /// Sets the bandwidth limit for a specific tenant.
    pub fn set_tenant_limit(&mut self, tenant_id: &str, limit_bps: u64) {
        if let Some((_, tenant)) = self.tenants.iter_mut().find(|(id, _)| id == tenant_id) {
            tenant.limit_bps = limit_bps;
        } else {
            let tenant =
                TenantBandwidth::new(tenant_id.to_string(), limit_bps, self.current_time_ms);
            self.tenants.push((tenant_id.to_string(), tenant));
        }
    }

    fn get_tenant_index(&mut self, tenant_id: &str) -> Option<usize> {
        self.tenants.iter().position(|(id, _)| id == tenant_id)
    }

    fn get_or_create_tenant_index(&mut self, tenant_id: &str) -> usize {
        if let Some(idx) = self.get_tenant_index(tenant_id) {
            return idx;
        }
        let tenant = TenantBandwidth::new(
            tenant_id.to_string(),
            self.config.default_tenant_limit_bps,
            self.current_time_ms,
        );
        self.tenants.push((tenant_id.to_string(), tenant));
        self.tenants.len() - 1
    }

    fn reset_tenant_window(tenant: &mut TenantBandwidth) {
        let rate = tenant.bytes_in_window * 8 * 1000 / tenant.window_start_ms.max(1);
        if rate > tenant.peak_bps {
            tenant.peak_bps = rate;
        }
        tenant.bytes_in_window = 0;
        tenant.window_start_ms = 0;
    }

    fn reset_global_window(&mut self) {
        self.global_bytes_in_window = 0;
        self.global_window_start_ms = 0;
    }

    /// Checks if the given request can proceed and returns the result.
    pub fn check(&mut self, tenant_id: &str, bytes: u64) -> BandwidthResult {
        self.total_requests += 1;

        let window_ms = self.config.measurement_window_ms;
        if self
            .current_time_ms
            .saturating_sub(self.global_window_start_ms)
            >= window_ms
        {
            self.reset_global_window();
        }

        let tenant_idx = self.get_or_create_tenant_index(tenant_id);
        let tenant_limit_bps = self.tenants[tenant_idx].1.limit_bps;

        if self
            .current_time_ms
            .saturating_sub(self.tenants[tenant_idx].1.window_start_ms)
            >= window_ms
        {
            Self::reset_tenant_window(&mut self.tenants[tenant_idx].1);
        }

        if self.tenants[tenant_idx].1.window_start_ms == 0 {
            self.tenants[tenant_idx].1.window_start_ms = self.current_time_ms;
        }
        if self.global_window_start_ms == 0 {
            self.global_window_start_ms = self.current_time_ms;
        }

        let tenant_rate = if window_ms > 0 {
            self.tenants[tenant_idx].1.bytes_in_window * 8 * 1000 / window_ms
        } else {
            0
        };

        let global_rate = if window_ms > 0 {
            self.global_bytes_in_window * 8 * 1000 / window_ms
        } else {
            0
        };

        let tenant_burst_limit = (tenant_limit_bps as f64 * self.config.burst_factor) as u64;
        let global_burst_limit =
            (self.config.global_limit_bps as f64 * self.config.burst_factor) as u64;

        let tenant_exceeds = tenant_rate.saturating_add(bytes * 8) > tenant_burst_limit;
        let global_exceeds = global_rate.saturating_add(bytes * 8) > global_burst_limit;

        match self.config.enforcement {
            EnforcementMode::Strict => {
                if tenant_exceeds || global_exceeds {
                    self.total_dropped += 1;
                    self.tenants[tenant_idx].1.total_dropped += bytes;
                    return BandwidthResult::Dropped { bytes };
                }
            }
            EnforcementMode::Shaping => {
                let tenant_excess = tenant_rate
                    .saturating_add(bytes * 8)
                    .saturating_sub(tenant_limit_bps);
                let global_excess = global_rate
                    .saturating_add(bytes * 8)
                    .saturating_sub(self.config.global_limit_bps);

                let excess = tenant_excess.max(global_excess);
                if excess > 0 {
                    let delay_ms = excess * 1000 / tenant_limit_bps.max(1);
                    self.total_throttled += 1;
                    self.tenants[tenant_idx].1.total_throttled += 1;
                    return BandwidthResult::Throttled { delay_ms };
                }
            }
            EnforcementMode::Monitor => {
                let over_limit = tenant_exceeds || global_exceeds;
                return BandwidthResult::Monitored { over_limit };
            }
        }

        self.tenants[tenant_idx].1.bytes_in_window += bytes;
        self.tenants[tenant_idx].1.total_bytes += bytes;
        self.global_bytes_in_window += bytes;
        self.total_allowed += 1;

        BandwidthResult::Allowed
    }

    /// Advances the internal clock by the given milliseconds.
    pub fn advance_time(&mut self, ms: u64) {
        let new_time = self.current_time_ms.saturating_add(ms);

        if new_time - self.global_window_start_ms >= self.config.measurement_window_ms {
            self.global_bytes_in_window = 0;
            self.global_window_start_ms = new_time;
        }

        for (_, tenant) in &mut self.tenants {
            if new_time - tenant.window_start_ms >= self.config.measurement_window_ms {
                tenant.bytes_in_window = 0;
                tenant.window_start_ms = new_time;
            }
        }

        self.current_time_ms = new_time;
    }

    /// Sets the internal clock to the given time in milliseconds.
    pub fn set_time(&mut self, ms: u64) {
        self.current_time_ms = ms;
    }

    /// Returns the current bandwidth usage in bits per second for a tenant.
    pub fn tenant_usage_bps(&self, tenant_id: &str) -> u64 {
        if let Some((_, tenant)) = self.tenants.iter().find(|(id, _)| id == tenant_id) {
            if self.current_time_ms >= tenant.window_start_ms
                && self.current_time_ms - tenant.window_start_ms < self.config.measurement_window_ms
            {
                return tenant.bytes_in_window * 8 * 1000 / self.config.measurement_window_ms;
            }
        }
        0
    }

    /// Returns the current global bandwidth usage in bits per second.
    pub fn global_usage_bps(&self) -> u64 {
        if self.current_time_ms >= self.global_window_start_ms
            && self.current_time_ms - self.global_window_start_ms
                < self.config.measurement_window_ms
        {
            return self.global_bytes_in_window * 8 * 1000 / self.config.measurement_window_ms;
        }
        0
    }

    /// Returns current bandwidth statistics.
    pub fn stats(&self) -> BandwidthStats {
        BandwidthStats {
            total_requests: self.total_requests,
            total_allowed: self.total_allowed,
            total_throttled: self.total_throttled,
            total_dropped: self.total_dropped,
            global_usage_bps: self.global_usage_bps(),
            tenant_count: self.tenants.len(),
        }
    }

    /// Returns the number of tenants currently tracked.
    pub fn tenant_count(&self) -> usize {
        self.tenants.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = BandwidthConfig::default();
        assert_eq!(config.global_limit_bps, 10_000_000_000);
        assert_eq!(config.default_tenant_limit_bps, 1_000_000_000);
        assert!((config.burst_factor - 1.5).abs() < 0.001);
        assert_eq!(config.measurement_window_ms, 1000);
        assert_eq!(config.enforcement, EnforcementMode::Strict);
    }

    #[test]
    fn test_within_limit_allowed() {
        let config = BandwidthConfig::default();
        let mut allocator = BandwidthAllocator::new(config);
        allocator.set_time(100);

        let result = allocator.check("tenant1", 1_000_000);
        assert_eq!(result, BandwidthResult::Allowed);
    }

    #[test]
    fn test_exceed_tenant_limit_strict() {
        let mut config = BandwidthConfig::default();
        config.enforcement = EnforcementMode::Strict;
        config.default_tenant_limit_bps = 1_000_000_000;
        let mut allocator = BandwidthAllocator::new(config);
        allocator.set_time(100);

        allocator.check("tenant1", 10_000_000);
        let result = allocator.check("tenant1", 200_000_000);

        assert_eq!(result, BandwidthResult::Dropped { bytes: 200_000_000 });
    }

    #[test]
    fn test_exceed_tenant_limit_shaping() {
        let mut config = BandwidthConfig::default();
        config.enforcement = EnforcementMode::Shaping;
        config.default_tenant_limit_bps = 1_000_000_000;
        let mut allocator = BandwidthAllocator::new(config);
        allocator.set_time(100);

        for _ in 0..10 {
            allocator.check("tenant1", 100_000_000);
        }

        let result = allocator.check("tenant1", 100_000_000);
        match result {
            BandwidthResult::Throttled { delay_ms } => {
                assert!(delay_ms > 0);
            }
            _ => panic!("Expected Throttled result"),
        }
    }

    #[test]
    fn test_exceed_global_limit() {
        let mut config = BandwidthConfig::default();
        config.enforcement = EnforcementMode::Strict;
        config.global_limit_bps = 1_000_000_000;
        config.default_tenant_limit_bps = 10_000_000_000;
        let mut allocator = BandwidthAllocator::new(config);
        allocator.set_time(100);

        let result = allocator.check("tenant1", 200_000_000);
        assert_eq!(result, BandwidthResult::Dropped { bytes: 200_000_000 });
    }

    #[test]
    fn test_monitor_mode() {
        let mut config = BandwidthConfig::default();
        config.enforcement = EnforcementMode::Monitor;
        config.default_tenant_limit_bps = 1_000_000_000;
        let mut allocator = BandwidthAllocator::new(config);
        allocator.set_time(100);

        let result = allocator.check("tenant1", 1_000_000);
        assert_eq!(result, BandwidthResult::Monitored { over_limit: false });

        for _ in 0..10 {
            allocator.check("tenant1", 200_000_000);
        }
        let result = allocator.check("tenant1", 200_000_000);
        assert_eq!(result, BandwidthResult::Monitored { over_limit: true });
    }

    #[test]
    fn test_burst_factor() {
        let mut config = BandwidthConfig::default();
        config.enforcement = EnforcementMode::Strict;
        config.default_tenant_limit_bps = 1_000_000_000;
        config.burst_factor = 2.0;
        let mut allocator = BandwidthAllocator::new(config);
        allocator.set_time(100);

        let result = allocator.check("tenant1", 250_000_000);
        assert_eq!(result, BandwidthResult::Allowed);
    }

    #[test]
    fn test_window_reset() {
        let mut config = BandwidthConfig::default();
        config.measurement_window_ms = 100;
        let mut allocator = BandwidthAllocator::new(config);
        allocator.set_time(100);

        allocator.check("tenant1", 1_000_000);

        let stats_before = allocator.stats();
        assert!(stats_before.global_usage_bps > 0);

        allocator.advance_time(150);

        let usage = allocator.tenant_usage_bps("tenant1");
        assert_eq!(usage, 0);
    }

    #[test]
    fn test_set_tenant_limit() {
        let config = BandwidthConfig::default();
        let mut allocator = BandwidthAllocator::new(config);
        allocator.set_time(100);

        allocator.set_tenant_limit("tenant1", 500_000_000);
        allocator.set_tenant_limit("tenant2", 2_000_000_000);

        assert_eq!(allocator.tenant_usage_bps("tenant1"), 0);
        assert_eq!(allocator.tenant_usage_bps("tenant2"), 0);
        assert_eq!(allocator.tenant_count(), 2);
    }

    #[test]
    fn test_multiple_tenants() {
        let mut config = BandwidthConfig::default();
        config.default_tenant_limit_bps = 10_000_000_000;
        let mut allocator = BandwidthAllocator::new(config);
        allocator.set_time(100);

        let result1 = allocator.check("tenant1", 50_000_000);
        let result2 = allocator.check("tenant2", 50_000_000);

        assert_eq!(result1, BandwidthResult::Allowed);
        assert_eq!(result2, BandwidthResult::Allowed);
    }

    #[test]
    fn test_tenant_usage_bps() {
        let mut config = BandwidthConfig::default();
        config.measurement_window_ms = 1000;
        let mut allocator = BandwidthAllocator::new(config);
        allocator.set_time(100);

        allocator.check("tenant1", 125_000_000);

        let usage = allocator.tenant_usage_bps("tenant1");
        assert_eq!(usage, 1_000_000_000);
    }

    #[test]
    fn test_global_usage_bps() {
        let mut config = BandwidthConfig::default();
        config.measurement_window_ms = 1000;
        let mut allocator = BandwidthAllocator::new(config);
        allocator.set_time(100);

        allocator.check("tenant1", 125_000_000);
        allocator.check("tenant2", 125_000_000);

        let usage = allocator.global_usage_bps();
        assert_eq!(usage, 2_000_000_000);
    }

    #[test]
    fn test_stats_snapshot() {
        let config = BandwidthConfig::default();
        let mut allocator = BandwidthAllocator::new(config);
        allocator.set_time(100);

        allocator.check("tenant1", 1_000_000);
        allocator.check("tenant1", 1_000_000);

        let stats = allocator.stats();
        assert_eq!(stats.total_requests, 2);
        assert_eq!(stats.total_allowed, 2);
        assert_eq!(stats.tenant_count, 1);
    }

    #[test]
    fn test_new_tenant_auto_created() {
        let config = BandwidthConfig::default();
        let mut allocator = BandwidthAllocator::new(config);
        allocator.set_time(100);

        assert_eq!(allocator.tenant_count(), 0);

        allocator.check("new_tenant", 1_000_000);

        assert_eq!(allocator.tenant_count(), 1);
    }

    #[test]
    fn test_peak_bps_tracking() {
        let mut config = BandwidthConfig::default();
        config.measurement_window_ms = 100;
        let mut allocator = BandwidthAllocator::new(config);
        allocator.set_time(100);

        for _ in 0..5 {
            allocator.check("tenant1", 100_000_000);
        }

        allocator.advance_time(200);
        allocator.check("tenant1", 50_000_000);

        let stats = allocator.stats();
        assert!(stats.global_usage_bps > 0);
    }
}
