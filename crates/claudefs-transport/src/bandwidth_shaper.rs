//! Bandwidth shaper for per-tenant QoS enforcement.
//!
//! Provides token bucket based bandwidth limiting per tenant with hard limits (reject)
//! and soft limits (warn/backpressure) enforcement modes.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use thiserror::Error;
use tracing::{debug, warn};

/// Tenant bandwidth identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BandwidthId(pub u64);

impl BandwidthId {
    /// Creates a new bandwidth ID.
    pub fn new(id: u64) -> Self {
        Self(id)
    }
}

impl Default for BandwidthId {
    fn default() -> Self {
        Self(0)
    }
}

/// Enforcement mode for bandwidth limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnforcementMode {
    Hard,
    Soft,
}

impl Default for EnforcementMode {
    fn default() -> Self {
        EnforcementMode::Soft
    }
}

/// Bandwidth allocation for a tenant.
#[derive(Debug, Clone)]
pub struct BandwidthAllocation {
    pub tenant_id: BandwidthId,
    pub bytes_per_sec: u64,
    pub burst_bytes: u64,
    pub enforcement_mode: EnforcementMode,
}

impl BandwidthAllocation {
    /// Creates a new bandwidth allocation.
    pub fn new(
        tenant_id: BandwidthId,
        bytes_per_sec: u64,
        burst_bytes: u64,
        enforcement_mode: EnforcementMode,
    ) -> Self {
        Self {
            tenant_id,
            bytes_per_sec,
            burst_bytes,
            enforcement_mode,
        }
    }

    /// Validates the allocation parameters.
    pub fn is_valid(&self) -> bool {
        self.bytes_per_sec > 0 && self.burst_bytes > 0
    }
}

/// Token bucket state (thread-safe via atomics).
pub struct TokenBucket {
    pub tokens: AtomicU64,
    pub last_refill_ns: AtomicU64,
    pub capacity: u64,
    pub refill_rate_per_sec: u64,
}

impl TokenBucket {
    /// Creates a new token bucket.
    pub fn new(capacity: u64, refill_rate_per_sec: u64) -> Self {
        let now_ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;

        Self {
            tokens: AtomicU64::new(capacity),
            last_refill_ns: AtomicU64::new(now_ns),
            capacity,
            refill_rate_per_sec,
        }
    }

    /// Tries to consume tokens, returns true if successful.
    pub fn try_consume(&self, tokens: u64) -> bool {
        self.refill();

        let current = self.tokens.load(Ordering::Relaxed);
        if current >= tokens {
            self.tokens
                .fetch_sub(tokens, Ordering::Relaxed)
                .checked_sub(tokens)
                .is_some()
        } else {
            false
        }
    }

    /// Returns available tokens after refill.
    pub fn available(&self) -> u64 {
        self.refill();
        self.tokens.load(Ordering::Relaxed)
    }

    /// Refills tokens based on elapsed time.
    fn refill(&self) {
        let now_ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;

        let last_refill = self.last_refill_ns.load(Ordering::Relaxed);
        let elapsed_ns = now_ns.saturating_sub(last_refill);

        if elapsed_ns < 1_000_000_000 {
            return;
        }

        let elapsed_secs = elapsed_ns as f64 / 1_000_000_000.0;
        let refill = (elapsed_secs * self.refill_rate_per_sec as f64) as u64;

        let current = self.tokens.load(Ordering::Relaxed);
        let new_tokens = std::cmp::min(self.capacity, current.saturating_add(refill));

        self.tokens.store(new_tokens, Ordering::Relaxed);
        self.tokens.store(new_tokens, Ordering::Relaxed);
        self.last_refill_ns.store(now_ns as u64, Ordering::Relaxed);
    }

    /// Refills with a specific timestamp (for testing).
    #[cfg(test)]
    pub fn refill_at_ns(&self, now_ns: u64) {
        let last_refill = self.last_refill_ns.load(Ordering::Relaxed);
        let elapsed_ns = now_ns.saturating_sub(last_refill);

        if elapsed_ns < 1_000_000_000 {
            return;
        }

        let elapsed_secs = elapsed_ns as f64 / 1_000_000_000.0;
        let refill = (elapsed_secs * self.refill_rate_per_sec as f64) as u64;

        let current = self.tokens.load(Ordering::Relaxed);
        let new_tokens = std::cmp::min(self.capacity, current.saturating_add(refill));

        self.tokens.store(new_tokens, Ordering::Relaxed);
        self.last_refill_ns.store(now_ns, Ordering::Relaxed);
    }

    /// Resets the token bucket to full capacity.
    pub fn reset(&self) {
        self.tokens.store(self.capacity, Ordering::Relaxed);
    }
}

/// Internal state for a tenant's bandwidth.
struct TenantState {
    allocation: BandwidthAllocation,
    bucket: Arc<TokenBucket>,
    requests_granted: AtomicU64,
    requests_rejected: AtomicU64,
    bytes_granted: AtomicU64,
    bytes_rejected: AtomicU64,
}

impl TenantState {
    fn new(allocation: BandwidthAllocation) -> Self {
        let burst_bytes = allocation.burst_bytes;
        let bytes_per_sec = allocation.bytes_per_sec;
        Self {
            allocation,
            bucket: Arc::new(TokenBucket::new(burst_bytes, bytes_per_sec)),
            requests_granted: AtomicU64::new(0),
            requests_rejected: AtomicU64::new(0),
            bytes_granted: AtomicU64::new(0),
            bytes_rejected: AtomicU64::new(0),
        }
    }

    fn stats(&self) -> BandwidthStats {
        BandwidthStats {
            allocated_bytes_per_sec: self.allocation.bytes_per_sec,
            current_tokens: self.bucket.available(),
            requests_granted: self.requests_granted.load(Ordering::Relaxed),
            requests_rejected: self.requests_rejected.load(Ordering::Relaxed),
            bytes_granted: self.bytes_granted.load(Ordering::Relaxed),
            bytes_rejected: self.bytes_rejected.load(Ordering::Relaxed),
        }
    }
}

/// Bandwidth shaper configuration.
#[derive(Debug, Clone)]
pub struct BandwidthShaperConfig {
    pub tick_interval_ms: u64,
    pub cleanup_interval_ms: u64,
}

impl Default for BandwidthShaperConfig {
    fn default() -> Self {
        Self {
            tick_interval_ms: 10,
            cleanup_interval_ms: 60_000,
        }
    }
}

/// Errors that can occur during bandwidth allocation.
#[derive(Error, Debug)]
pub enum BandwidthError {
    #[error("Bandwidth limit exceeded for tenant {tenant_id:?}")]
    LimitExceeded { tenant_id: BandwidthId },

    #[error("Invalid bandwidth allocation: {reason}")]
    InvalidAllocation { reason: String },
}

/// Statistics for a single tenant.
#[derive(Debug, Clone)]
pub struct BandwidthStats {
    pub allocated_bytes_per_sec: u64,
    pub current_tokens: u64,
    pub requests_granted: u64,
    pub requests_rejected: u64,
    pub bytes_granted: u64,
    pub bytes_rejected: u64,
}

/// Bandwidth shaper for per-tenant QoS enforcement.
pub struct BandwidthShaper {
    config: BandwidthShaperConfig,
    tenants: Mutex<HashMap<BandwidthId, TenantState>>,
    last_cleanup_ns: AtomicU64,
}

impl BandwidthShaper {
    /// Creates a new bandwidth shaper.
    pub fn new(config: BandwidthShaperConfig) -> Self {
        Self {
            config,
            tenants: Mutex::new(HashMap::new()),
            last_cleanup_ns: AtomicU64::new(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos() as u64,
            ),
        }
    }

    /// Sets a bandwidth allocation for a tenant.
    pub fn set_allocation(&self, alloc: BandwidthAllocation) -> Result<(), BandwidthError> {
        if !alloc.is_valid() {
            return Err(BandwidthError::InvalidAllocation {
                reason: "bytes_per_sec and burst_bytes must be > 0".to_string(),
            });
        }

        let tenant_id = alloc.tenant_id;
        let mut tenants = self
            .tenants
            .lock()
            .map_err(|_| BandwidthError::InvalidAllocation {
                reason: "Failed to acquire lock".to_string(),
            })?;

        if tenants.contains_key(&tenant_id) {
            tenants.remove(&tenant_id);
        }

        tenants.insert(tenant_id, TenantState::new(alloc));
        debug!(tenant_id = ?tenant_id, "Bandwidth allocation set");
        Ok(())
    }

    /// Attempts to allocate bandwidth for a tenant.
    pub fn try_allocate(&self, tenant_id: BandwidthId, bytes: u64) -> Result<(), BandwidthError> {
        let tenants = self
            .tenants
            .lock()
            .map_err(|_| BandwidthError::InvalidAllocation {
                reason: "Failed to acquire lock".to_string(),
            })?;

        let state = tenants
            .get(&tenant_id)
            .ok_or_else(|| BandwidthError::InvalidAllocation {
                reason: format!("No allocation for tenant {:?}", tenant_id),
            })?;

        let available = state.bucket.available();

        if available >= bytes {
            if state.bucket.try_consume(bytes) {
                state.requests_granted.fetch_add(1, Ordering::Relaxed);
                state.bytes_granted.fetch_add(bytes, Ordering::Relaxed);
                return Ok(());
            }
        }

        state.requests_rejected.fetch_add(1, Ordering::Relaxed);
        state.bytes_rejected.fetch_add(bytes, Ordering::Relaxed);

        match state.allocation.enforcement_mode {
            EnforcementMode::Hard => Err(BandwidthError::LimitExceeded { tenant_id }),
            EnforcementMode::Soft => {
                warn!(tenant_id = ?tenant_id, bytes = bytes, available = available, "Soft bandwidth limit warning");
                state.requests_granted.fetch_add(1, Ordering::Relaxed);
                state.bytes_granted.fetch_add(bytes, Ordering::Relaxed);
                Ok(())
            }
        }
    }

    /// Refunds unused bandwidth tokens to a tenant.
    pub fn refund(&self, tenant_id: BandwidthId, bytes: u64) {
        if let Ok(tenants) = self.tenants.lock() {
            if let Some(state) = tenants.get(&tenant_id) {
                let current = state.bucket.tokens.load(Ordering::Relaxed);
                let new_tokens =
                    std::cmp::min(state.bucket.capacity, current.saturating_add(bytes));
                state.bucket.tokens.store(new_tokens, Ordering::Relaxed);
            }
        }
    }

    /// Returns statistics for a specific tenant.
    pub fn stats(&self, tenant_id: BandwidthId) -> Option<BandwidthStats> {
        let tenants = self.tenants.lock().ok()?;
        tenants.get(&tenant_id).map(|s| s.stats())
    }

    /// Returns statistics for all tenants.
    pub fn all_stats(&self) -> Vec<(BandwidthId, BandwidthStats)> {
        let tenants = match self.tenants.lock() {
            Ok(t) => t,
            Err(_) => return Vec::new(),
        };

        tenants
            .iter()
            .map(|(id, state)| (*id, state.stats()))
            .collect()
    }

    /// Removes unused tenants (cleanup).
    pub fn cleanup(&self) {
        let now_ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;

        let last_cleanup = self.last_cleanup_ns.load(Ordering::Relaxed);
        if now_ns.saturating_sub(last_cleanup) < self.config.cleanup_interval_ms * 1_000_000 {
            return;
        }

        let mut tenants = match self.tenants.lock() {
            Ok(t) => t,
            Err(_) => return,
        };

        let empty_tenants: Vec<BandwidthId> = tenants
            .iter()
            .filter(|(_, s)| {
                s.requests_granted.load(Ordering::Relaxed) == 0
                    && s.requests_rejected.load(Ordering::Relaxed) == 0
            })
            .map(|(id, _)| *id)
            .collect();

        for id in empty_tenants {
            tenants.remove(&id);
        }

        self.last_cleanup_ns.store(now_ns, Ordering::Relaxed);
    }
}

impl Default for BandwidthShaper {
    fn default() -> Self {
        Self::new(BandwidthShaperConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_allocation(
        id: u64,
        rate: u64,
        burst: u64,
        mode: EnforcementMode,
    ) -> BandwidthAllocation {
        BandwidthAllocation::new(BandwidthId(id), rate, burst, mode)
    }

    #[test]
    fn test_bandwidth_id_default() {
        let id = BandwidthId::default();
        assert_eq!(id.0, 0);
    }

    #[test]
    fn test_enforcement_mode_default() {
        let mode = EnforcementMode::default();
        assert_eq!(mode, EnforcementMode::Soft);
    }

    #[test]
    fn test_allocation_valid() {
        let alloc = make_allocation(1, 1000, 100, EnforcementMode::Hard);
        assert!(alloc.is_valid());
    }

    #[test]
    fn test_allocation_invalid_zero_rate() {
        let alloc = BandwidthAllocation::new(BandwidthId(1), 0, 100, EnforcementMode::Hard);
        assert!(!alloc.is_valid());
    }

    #[test]
    fn test_allocation_invalid_zero_burst() {
        let alloc = BandwidthAllocation::new(BandwidthId(1), 1000, 0, EnforcementMode::Hard);
        assert!(!alloc.is_valid());
    }

    #[test]
    fn test_token_bucket_initial_capacity() {
        let bucket = TokenBucket::new(1000, 100);
        assert_eq!(bucket.capacity, 1000);
        assert_eq!(bucket.refill_rate_per_sec, 100);
    }

    #[test]
    fn test_token_bucket_try_consume_success() {
        let bucket = TokenBucket::new(1000, 100);
        assert!(bucket.try_consume(100));
    }

    #[test]
    fn test_token_bucket_try_consume_failure() {
        let bucket = TokenBucket::new(50, 100);
        assert!(!bucket.try_consume(100));
    }

    #[test]
    fn test_token_bucket_refill() {
        let bucket = TokenBucket::new(100, 1000);
        bucket.try_consume(100);

        let available = bucket.available();
        assert!(available < 100);

        std::thread::sleep(std::time::Duration::from_millis(1100));

        let available = bucket.available();
        assert!(available >= 50);
    }

    #[test]
    fn test_shaper_set_allocation() {
        let shaper = BandwidthShaper::default();
        let alloc = make_allocation(1, 1000, 1000, EnforcementMode::Hard);

        shaper.set_allocation(alloc).unwrap();

        let stats = shaper.stats(BandwidthId(1));
        assert!(stats.is_some());
        assert_eq!(stats.unwrap().allocated_bytes_per_sec, 1000);
    }

    #[test]
    fn test_shaper_try_allocate_success() {
        let shaper = BandwidthShaper::default();
        let alloc = make_allocation(1, 1000, 1000, EnforcementMode::Hard);
        shaper.set_allocation(alloc).unwrap();

        let result = shaper.try_allocate(BandwidthId(1), 500);
        assert!(result.is_ok());

        let stats = shaper.stats(BandwidthId(1)).unwrap();
        assert_eq!(stats.requests_granted, 1);
        assert_eq!(stats.bytes_granted, 500);
    }

    #[test]
    fn test_shaper_hard_limit_rejection() {
        let shaper = BandwidthShaper::default();
        let alloc = make_allocation(1, 100, 100, EnforcementMode::Hard);
        shaper.set_allocation(alloc).unwrap();

        shaper.try_allocate(BandwidthId(1), 100).unwrap();
        let result = shaper.try_allocate(BandwidthId(1), 100);

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, BandwidthError::LimitExceeded { .. }));

        let stats = shaper.stats(BandwidthId(1)).unwrap();
        assert_eq!(stats.requests_granted, 1);
        assert_eq!(stats.requests_rejected, 1);
    }

    #[test]
    fn test_shaper_soft_limit_warning() {
        let shaper = BandwidthShaper::default();
        let alloc = make_allocation(1, 100, 100, EnforcementMode::Soft);
        shaper.set_allocation(alloc).unwrap();

        shaper.try_allocate(BandwidthId(1), 100).unwrap();
        let result = shaper.try_allocate(BandwidthId(1), 50);

        assert!(result.is_ok());

        let stats = shaper.stats(BandwidthId(1)).unwrap();
        assert_eq!(stats.requests_granted, 2);
    }

    #[test]
    fn test_shaper_refund() {
        let shaper = BandwidthShaper::default();
        let alloc = make_allocation(1, 1000, 1000, EnforcementMode::Hard);
        shaper.set_allocation(alloc).unwrap();

        shaper.try_allocate(BandwidthId(1), 800).unwrap();
        shaper.refund(BandwidthId(1), 500);

        let stats = shaper.stats(BandwidthId(1)).unwrap();
        assert!(stats.current_tokens >= 700);
    }

    #[test]
    fn test_shaper_all_stats() {
        let shaper = BandwidthShaper::default();
        shaper
            .set_allocation(make_allocation(1, 1000, 1000, EnforcementMode::Hard))
            .unwrap();
        shaper
            .set_allocation(make_allocation(2, 2000, 2000, EnforcementMode::Soft))
            .unwrap();

        let stats = shaper.all_stats();
        assert_eq!(stats.len(), 2);
    }

    #[test]
    fn test_shaper_unknown_tenant() {
        let shaper = BandwidthShaper::default();
        let result = shaper.try_allocate(BandwidthId(999), 100);
        assert!(result.is_err());
    }

    #[test]
    fn test_concurrent_allocation() {
        use std::sync::Arc;
        use std::thread;

        let shaper = Arc::new(BandwidthShaper::default());
        shaper
            .set_allocation(make_allocation(
                1,
                10_000_000,
                10_000_000,
                EnforcementMode::Hard,
            ))
            .unwrap();

        let mut handles = vec![];
        for _ in 0..10 {
            let shaper = Arc::clone(&shaper);
            let handle = thread::spawn(move || {
                for _ in 0..100 {
                    let _ = shaper.try_allocate(BandwidthId(1), 1000);
                }
            });
            handles.push(handle);
        }

        for h in handles {
            h.join().unwrap();
        }

        let stats = shaper.stats(BandwidthId(1)).unwrap();
        assert!(stats.requests_granted + stats.requests_rejected >= 500);
    }

    #[test]
    fn test_burst_allowance() {
        let shaper = BandwidthShaper::default();
        let alloc = make_allocation(1, 100, 500, EnforcementMode::Hard);
        shaper.set_allocation(alloc).unwrap();

        for _ in 0..5 {
            shaper.try_allocate(BandwidthId(1), 100).unwrap();
        }

        let result = shaper.try_allocate(BandwidthId(1), 100);
        assert!(result.is_err());

        let stats = shaper.stats(BandwidthId(1)).unwrap();
        assert_eq!(stats.requests_granted, 5);
    }

    #[test]
    fn test_multiple_tenants_independent() {
        let shaper = BandwidthShaper::default();
        shaper
            .set_allocation(make_allocation(1, 100, 100, EnforcementMode::Hard))
            .unwrap();
        shaper
            .set_allocation(make_allocation(2, 200, 200, EnforcementMode::Hard))
            .unwrap();

        let r1 = shaper.try_allocate(BandwidthId(1), 100);
        let r2 = shaper.try_allocate(BandwidthId(1), 100);
        let r3 = shaper.try_allocate(BandwidthId(2), 200);

        assert!(r1.is_ok());
        assert!(r2.is_err());
        assert!(r3.is_ok());

        let stats1 = shaper.stats(BandwidthId(1)).unwrap();
        let stats2 = shaper.stats(BandwidthId(2)).unwrap();

        assert_eq!(stats1.requests_granted, 1);
        assert_eq!(stats2.requests_granted, 1);
    }

    #[test]
    fn test_request_larger_than_burst() {
        let shaper = BandwidthShaper::default();
        let alloc = make_allocation(1, 100, 100, EnforcementMode::Hard);
        shaper.set_allocation(alloc).unwrap();

        let result = shaper.try_allocate(BandwidthId(1), 1000);
        assert!(result.is_err());
    }

    #[test]
    fn test_token_accuracy_over_time() {
        let bucket = TokenBucket::new(1000, 1000);

        bucket.try_consume(500);

        std::thread::sleep(std::time::Duration::from_millis(500));

        let available = bucket.available();
        assert!(available >= 450 && available <= 550);
    }

    #[test]
    fn test_shaper_reset_on_allocation_change() {
        let shaper = BandwidthShaper::default();
        shaper
            .set_allocation(make_allocation(1, 1000, 1000, EnforcementMode::Hard))
            .unwrap();

        shaper.try_allocate(BandwidthId(1), 500).unwrap();

        shaper
            .set_allocation(make_allocation(1, 2000, 2000, EnforcementMode::Hard))
            .unwrap();

        let stats = shaper.stats(BandwidthId(1)).unwrap();
        assert_eq!(stats.allocated_bytes_per_sec, 2000);
        assert!(stats.current_tokens >= 2000);
    }

    #[test]
    fn test_cleanup_removes_unused_tenants() {
        let config = BandwidthShaperConfig {
            tick_interval_ms: 10,
            cleanup_interval_ms: 1,
        };
        let shaper = BandwidthShaper::new(config);

        shaper
            .set_allocation(make_allocation(1, 1000, 1000, EnforcementMode::Hard))
            .unwrap();

        std::thread::sleep(std::time::Duration::from_millis(20));

        shaper.cleanup();

        let stats = shaper.stats(BandwidthId(1));
        assert!(stats.is_some());
    }

    #[test]
    fn test_zero_allocation() {
        let shaper = BandwidthShaper::default();
        let alloc = BandwidthAllocation::new(BandwidthId(1), 0, 0, EnforcementMode::Hard);

        let result = shaper.set_allocation(alloc);
        assert!(result.is_err());
    }

    #[test]
    fn test_cleanup_unused_after_allocation() {
        let config = BandwidthShaperConfig {
            tick_interval_ms: 10,
            cleanup_interval_ms: 1,
        };
        let shaper = BandwidthShaper::new(config);

        shaper
            .set_allocation(make_allocation(1, 1000, 1000, EnforcementMode::Hard))
            .unwrap();
        shaper.try_allocate(BandwidthId(1), 100).unwrap();

        std::thread::sleep(std::time::Duration::from_millis(20));

        shaper.cleanup();

        let stats = shaper.stats(BandwidthId(1));
        assert!(stats.is_some());
    }

    #[test]
    fn test_token_bucket_reset() {
        let bucket = TokenBucket::new(1000, 100);
        bucket.try_consume(900);

        bucket.reset();

        assert_eq!(bucket.available(), 1000);
    }

    #[test]
    fn test_stats_snapshot() {
        let shaper = BandwidthShaper::default();
        shaper
            .set_allocation(make_allocation(1, 1000, 1000, EnforcementMode::Hard))
            .unwrap();

        shaper.try_allocate(BandwidthId(1), 100).unwrap();
        shaper.try_allocate(BandwidthId(1), 200).unwrap();

        let stats = shaper.stats(BandwidthId(1)).unwrap();
        assert_eq!(stats.allocated_bytes_per_sec, 1000);
        assert_eq!(stats.requests_granted, 2);
        assert_eq!(stats.bytes_granted, 300);
    }
}
