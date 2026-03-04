//! Credit-window flow control for per-connection in-flight byte budgets.
//!
//! Implements an explicit credit-grant/consume protocol used by the replication layer
//! to prevent journal backlog buildup and by the FUSE client to manage prefetch budgets.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// State of a credit window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CreditWindowState {
    /// Normal operation — credits available.
    Normal,
    /// Warning — credits running low (below 25%).
    Warning,
    /// Throttled — credits below 10%, slow down.
    Throttled,
    /// Exhausted — no credits available.
    Exhausted,
}

/// Configuration for a credit window.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreditWindowConfig {
    /// Total credit budget in bytes (default: 64MB = 67_108_864).
    pub total_credits: u64,
    /// Warning threshold as fraction 0.0..1.0 (default: 0.25).
    pub warning_threshold: f64,
    /// Throttle threshold as fraction 0.0..1.0 (default: 0.10).
    pub throttle_threshold: f64,
    /// Maximum single allocation in bytes (default: 8MB = 8_388_608).
    pub max_single_alloc: u64,
}

impl Default for CreditWindowConfig {
    fn default() -> Self {
        Self {
            total_credits: 67_108_864,
            warning_threshold: 0.25,
            throttle_threshold: 0.10,
            max_single_alloc: 8_388_608,
        }
    }
}

/// Atomic stats for credit window.
pub struct CreditWindowStats {
    pub grants_issued: AtomicU64,
    pub grants_denied: AtomicU64,
    pub credits_granted: AtomicU64,
    pub credits_returned: AtomicU64,
    pub throttle_events: AtomicU64,
    pub exhaustion_events: AtomicU64,
}

impl CreditWindowStats {
    fn new() -> Self {
        Self {
            grants_issued: AtomicU64::new(0),
            grants_denied: AtomicU64::new(0),
            credits_granted: AtomicU64::new(0),
            credits_returned: AtomicU64::new(0),
            throttle_events: AtomicU64::new(0),
            exhaustion_events: AtomicU64::new(0),
        }
    }

    pub fn snapshot(
        &self,
        available: u64,
        total: u64,
        state: CreditWindowState,
    ) -> CreditWindowStatsSnapshot {
        CreditWindowStatsSnapshot {
            grants_issued: self.grants_issued.load(Ordering::Relaxed),
            grants_denied: self.grants_denied.load(Ordering::Relaxed),
            credits_granted: self.credits_granted.load(Ordering::Relaxed),
            credits_returned: self.credits_returned.load(Ordering::Relaxed),
            throttle_events: self.throttle_events.load(Ordering::Relaxed),
            exhaustion_events: self.exhaustion_events.load(Ordering::Relaxed),
            available_credits: available,
            total_credits: total,
            state,
        }
    }
}

/// Snapshot of credit window stats.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreditWindowStatsSnapshot {
    pub grants_issued: u64,
    pub grants_denied: u64,
    pub credits_granted: u64,
    pub credits_returned: u64,
    pub throttle_events: u64,
    pub exhaustion_events: u64,
    pub available_credits: u64,
    pub total_credits: u64,
    pub state: CreditWindowState,
}

struct CreditWindowInner {
    available: AtomicU64,
    total: u64,
    config: CreditWindowConfig,
    stats: Arc<CreditWindowStats>,
}

/// A granted credit allocation — release on drop (but also has explicit `release()`).
pub struct CreditGrant {
    credits: u64,
    window: Arc<CreditWindowInner>,
    released: bool,
}

impl CreditGrant {
    /// Credits held by this grant.
    pub fn credits(&self) -> u64 {
        self.credits
    }

    /// Explicitly release credits back to the window.
    pub fn release(mut self) {
        if !self.released {
            self.window
                .available
                .fetch_add(self.credits, Ordering::Relaxed);
            self.window
                .stats
                .credits_returned
                .fetch_add(self.credits, Ordering::Relaxed);
            self.released = true;
        }
    }
}

impl Drop for CreditGrant {
    fn drop(&mut self) {
        if !self.released && self.credits > 0 {
            self.window
                .available
                .fetch_add(self.credits, Ordering::Relaxed);
            self.window
                .stats
                .credits_returned
                .fetch_add(self.credits, Ordering::Relaxed);
        }
    }
}

/// Credit window manager.
pub struct CreditWindow {
    inner: Arc<CreditWindowInner>,
}

impl CreditWindow {
    pub fn new(config: CreditWindowConfig) -> Self {
        let inner = Arc::new(CreditWindowInner {
            available: AtomicU64::new(config.total_credits),
            total: config.total_credits,
            config: config.clone(),
            stats: Arc::new(CreditWindowStats::new()),
        });
        Self { inner }
    }

    /// Attempt to acquire `bytes` credits.
    /// Returns Some(CreditGrant) if credits available, None if exhausted.
    pub fn try_acquire(&self, bytes: u64) -> Option<CreditGrant> {
        if bytes > self.inner.config.max_single_alloc {
            self.inner
                .stats
                .grants_denied
                .fetch_add(1, Ordering::Relaxed);
            return None;
        }

        let mut current = self.inner.available.load(Ordering::Relaxed);
        loop {
            if current < bytes {
                self.inner
                    .stats
                    .grants_denied
                    .fetch_add(1, Ordering::Relaxed);
                let state = self.state();
                if state == CreditWindowState::Exhausted {
                    self.inner
                        .stats
                        .exhaustion_events
                        .fetch_add(1, Ordering::Relaxed);
                } else if state == CreditWindowState::Throttled {
                    self.inner
                        .stats
                        .throttle_events
                        .fetch_add(1, Ordering::Relaxed);
                }
                return None;
            }
            let new_val = current - bytes;
            match self.inner.available.compare_exchange_weak(
                current,
                new_val,
                Ordering::Acquire,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    self.inner
                        .stats
                        .grants_issued
                        .fetch_add(1, Ordering::Relaxed);
                    self.inner
                        .stats
                        .credits_granted
                        .fetch_add(bytes, Ordering::Relaxed);
                    return Some(CreditGrant {
                        credits: bytes,
                        window: Arc::clone(&self.inner),
                        released: false,
                    });
                }
                Err(actual) => {
                    current = actual;
                }
            }
        }
    }

    /// Force-return `bytes` credits (used when receiver sends credit grants back to sender).
    pub fn return_credits(&self, bytes: u64) {
        let current = self.inner.available.load(Ordering::Relaxed);
        let max_add = self.inner.total.saturating_sub(current);
        let to_add = bytes.min(max_add);
        if to_add > 0 {
            self.inner.available.fetch_add(to_add, Ordering::Relaxed);
            self.inner
                .stats
                .credits_returned
                .fetch_add(to_add, Ordering::Relaxed);
        }
    }

    /// Current available credits.
    pub fn available(&self) -> u64 {
        self.inner.available.load(Ordering::Relaxed)
    }

    /// Total configured credits.
    pub fn total(&self) -> u64 {
        self.inner.total
    }

    /// Current state based on available/total ratio.
    pub fn state(&self) -> CreditWindowState {
        let available = self.available();
        let total = self.total();
        if total == 0 {
            return CreditWindowState::Exhausted;
        }
        let ratio = available as f64 / total as f64;
        if available == 0 {
            CreditWindowState::Exhausted
        } else if ratio <= self.inner.config.throttle_threshold {
            CreditWindowState::Throttled
        } else if ratio <= self.inner.config.warning_threshold {
            CreditWindowState::Warning
        } else {
            CreditWindowState::Normal
        }
    }

    /// Utilization as a value 0.0..=1.0 (consumed / total).
    pub fn utilization(&self) -> f64 {
        let total = self.total();
        if total == 0 {
            return 1.0;
        }
        let available = self.available();
        (total - available) as f64 / total as f64
    }

    /// Stats reference.
    pub fn stats(&self) -> Arc<CreditWindowStats> {
        Arc::clone(&self.inner.stats)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_default_config() {
        let window = CreditWindow::new(CreditWindowConfig::default());
        assert_eq!(window.available(), 67_108_864);
        assert_eq!(window.total(), 67_108_864);
    }

    #[test]
    fn test_try_acquire_success() {
        let window = CreditWindow::new(CreditWindowConfig::default());
        let grant = window.try_acquire(1_048_576);
        assert!(grant.is_some());
        assert_eq!(window.available(), 67_108_864 - 1_048_576);
    }

    #[test]
    fn test_try_acquire_returns_on_drop() {
        let window = CreditWindow::new(CreditWindowConfig::default());
        {
            let _grant = window.try_acquire(1_048_576).unwrap();
        }
        assert_eq!(window.available(), 67_108_864);
    }

    #[test]
    fn test_try_acquire_explicit_release() {
        let window = CreditWindow::new(CreditWindowConfig::default());
        let grant = window.try_acquire(1_048_576).unwrap();
        grant.release();
        assert_eq!(window.available(), 67_108_864);
    }

    #[test]
    fn test_try_acquire_exact_total() {
        let config = CreditWindowConfig {
            total_credits: 1000,
            max_single_alloc: 1000,
            ..Default::default()
        };
        let window = CreditWindow::new(config);
        let grant = window.try_acquire(1000);
        assert!(grant.is_some());
        assert_eq!(window.available(), 0);
    }

    #[test]
    fn test_try_acquire_over_total() {
        let config = CreditWindowConfig {
            total_credits: 1000,
            max_single_alloc: 2000,
            ..Default::default()
        };
        let window = CreditWindow::new(config);
        let grant = window.try_acquire(1001);
        assert!(grant.is_none());
    }

    #[test]
    fn test_try_acquire_max_single_alloc() {
        let window = CreditWindow::new(CreditWindowConfig::default());
        let grant = window.try_acquire(8_388_609);
        assert!(grant.is_none());
    }

    #[test]
    fn test_try_acquire_exhausts_window() {
        let config = CreditWindowConfig {
            total_credits: 100,
            max_single_alloc: 100,
            ..Default::default()
        };
        let window = CreditWindow::new(config);
        let _grant = window.try_acquire(100).unwrap();
        let grant2 = window.try_acquire(1);
        assert!(grant2.is_none());
    }

    #[test]
    fn test_state_normal() {
        let config = CreditWindowConfig {
            total_credits: 100,
            ..Default::default()
        };
        let window = CreditWindow::new(config);
        let _grant = window.try_acquire(50).unwrap();
        assert_eq!(window.state(), CreditWindowState::Normal);
    }

    #[test]
    fn test_state_warning() {
        let config = CreditWindowConfig {
            total_credits: 100,
            max_single_alloc: 100,
            ..Default::default()
        };
        let window = CreditWindow::new(config);
        let _grant = window.try_acquire(76).unwrap();
        assert_eq!(window.state(), CreditWindowState::Warning);
    }

    #[test]
    fn test_state_throttled() {
        let config = CreditWindowConfig {
            total_credits: 100,
            max_single_alloc: 100,
            ..Default::default()
        };
        let window = CreditWindow::new(config);
        let _grant = window.try_acquire(91).unwrap();
        assert_eq!(window.state(), CreditWindowState::Throttled);
    }

    #[test]
    fn test_state_exhausted() {
        let config = CreditWindowConfig {
            total_credits: 100,
            max_single_alloc: 100,
            ..Default::default()
        };
        let window = CreditWindow::new(config);
        let _grant = window.try_acquire(100).unwrap();
        assert_eq!(window.state(), CreditWindowState::Exhausted);
    }

    #[test]
    fn test_return_credits() {
        let config = CreditWindowConfig {
            total_credits: 100,
            max_single_alloc: 100,
            ..Default::default()
        };
        let window = CreditWindow::new(config);
        let grant = window.try_acquire(100).unwrap();
        drop(grant);
        assert_eq!(window.available(), 100);
        window.return_credits(50);
        assert_eq!(window.available(), 100);
    }

    #[test]
    fn test_return_credits_no_overflow() {
        let config = CreditWindowConfig {
            total_credits: 100,
            max_single_alloc: 100,
            ..Default::default()
        };
        let window = CreditWindow::new(config);
        window.return_credits(500);
        assert_eq!(window.available(), 100);
    }

    #[test]
    fn test_utilization() {
        let config = CreditWindowConfig {
            total_credits: 100,
            ..Default::default()
        };
        let window = CreditWindow::new(config);
        let _grant = window.try_acquire(50).unwrap();
        let util = window.utilization();
        assert!((util - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_stats_counts() {
        let config = CreditWindowConfig {
            total_credits: 100,
            max_single_alloc: 100,
            ..Default::default()
        };
        let window = CreditWindow::new(config);
        let _g1 = window.try_acquire(10).unwrap();
        let _g2 = window.try_acquire(10).unwrap();
        let _g3 = window.try_acquire(10).unwrap();
        let denied = window.try_acquire(100);
        assert!(denied.is_none());
        let snap = window
            .stats()
            .snapshot(window.available(), window.total(), window.state());
        assert_eq!(snap.grants_issued, 3);
        assert_eq!(snap.grants_denied, 1);
    }

    #[test]
    fn test_multiple_acquisitions_sum() {
        let config = CreditWindowConfig {
            total_credits: 64_000_000,
            max_single_alloc: 64_000_000,
            ..Default::default()
        };
        let window = CreditWindow::new(config);
        let _g1 = window.try_acquire(1_000_000).unwrap();
        let _g2 = window.try_acquire(1_000_000).unwrap();
        let _g3 = window.try_acquire(1_000_000).unwrap();
        let _g4 = window.try_acquire(61_000_000).unwrap();
        assert_eq!(window.available(), 0);
        let grant = window.try_acquire(1);
        assert!(grant.is_none());
    }

    #[test]
    fn test_state_transitions() {
        let config = CreditWindowConfig {
            total_credits: 100,
            max_single_alloc: 100,
            ..Default::default()
        };
        let window = CreditWindow::new(config);
        assert_eq!(window.state(), CreditWindowState::Normal);

        let _g1 = window.try_acquire(75).unwrap();
        assert_eq!(window.state(), CreditWindowState::Warning);

        let _g2 = window.try_acquire(15).unwrap();
        assert_eq!(window.state(), CreditWindowState::Throttled);

        let _g3 = window.try_acquire(10).unwrap();
        assert_eq!(window.state(), CreditWindowState::Exhausted);
    }
}
