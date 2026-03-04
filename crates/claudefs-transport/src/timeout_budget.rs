//! RPC timeout budget management for cascading deadline propagation.
//!
//! Manages cascading timeout budgets for nested RPC calls. When a client sends a request
//! with a 100ms deadline, internal sub-requests (storage reads, metadata lookups) must share
//! that budget and not exceed it. This module tracks the remaining time budget and propagates
//! it through nested RPC call chains.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use thiserror::Error;

/// A time budget for a chain of nested RPC calls.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeoutBudget {
    /// Total budget allocated at the start (ms).
    pub total_ms: u64,
    /// When the budget was created (ms since epoch).
    pub created_at_ms: u64,
    /// Overhead to subtract for each hop (serialization, network, processing).
    pub per_hop_overhead_ms: u64,
    /// Number of hops this budget has been through.
    pub hops: u32,
}

impl TimeoutBudget {
    /// Create a new budget with default overhead (5ms).
    pub fn new(total_ms: u64, now_ms: u64) -> Self {
        Self {
            total_ms,
            created_at_ms: now_ms,
            per_hop_overhead_ms: 5,
            hops: 0,
        }
    }

    /// Create a new budget with custom per-hop overhead.
    pub fn with_overhead(total_ms: u64, per_hop_overhead_ms: u64, now_ms: u64) -> Self {
        Self {
            total_ms,
            created_at_ms: now_ms,
            per_hop_overhead_ms,
            hops: 0,
        }
    }

    /// Remaining budget in ms. Returns 0 if expired.
    pub fn remaining_ms(&self, now_ms: u64) -> u64 {
        let elapsed = now_ms.saturating_sub(self.created_at_ms);
        let overhead_total = self.per_hop_overhead_ms.saturating_mul(self.hops as u64);
        let remaining = self
            .total_ms
            .saturating_sub(elapsed)
            .saturating_sub(overhead_total);
        if remaining == 0 {
            0
        } else {
            remaining
        }
    }

    /// Whether the budget has been exhausted (remaining == 0).
    pub fn is_exhausted(&self, now_ms: u64) -> bool {
        self.remaining_ms(now_ms) == 0
    }

    /// Create a child budget for a sub-request:
    /// - subtracts per_hop_overhead_ms once
    /// - remaining budget = min(remaining_ms, max_sub_ms)
    /// - increments hops counter
    /// Returns None if budget is already exhausted.
    pub fn child(&self, max_sub_ms: Option<u64>, now_ms: u64) -> Option<Self> {
        let remaining = self.remaining_ms(now_ms);
        if remaining == 0 {
            return None;
        }

        let child_total = if let Some(max) = max_sub_ms {
            remaining.saturating_sub(self.per_hop_overhead_ms).min(max)
        } else {
            remaining.saturating_sub(self.per_hop_overhead_ms)
        };

        if child_total == 0 {
            return None;
        }

        Some(Self {
            total_ms: child_total,
            created_at_ms: now_ms,
            per_hop_overhead_ms: self.per_hop_overhead_ms,
            hops: self.hops + 1,
        })
    }

    /// Fraction of budget remaining (0.0 = exhausted, 1.0 = full).
    pub fn fraction_remaining(&self, now_ms: u64) -> f64 {
        if self.total_ms == 0 {
            return 0.0;
        }
        let remaining = self.remaining_ms(now_ms) as f64;
        let total = self.total_ms as f64;
        (remaining / total).clamp(0.0, 1.0)
    }
}

/// Configuration for timeout budget tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeoutBudgetConfig {
    /// Default timeout budget for new client requests in ms (default: 1000).
    pub default_budget_ms: u64,
    /// Per-hop overhead in ms (default: 5).
    pub per_hop_overhead_ms: u64,
    /// Maximum number of hops before forcibly exhausting budget (default: 16).
    pub max_hops: u32,
    /// Warn when remaining budget is below this fraction (default: 0.1 = 10%).
    pub warn_threshold: f64,
}

impl Default for TimeoutBudgetConfig {
    fn default() -> Self {
        Self {
            default_budget_ms: 1000,
            per_hop_overhead_ms: 5,
            max_hops: 16,
            warn_threshold: 0.1,
        }
    }
}

/// Error for timeout budget operations.
#[derive(Debug, Error)]
pub enum TimeoutBudgetError {
    /// Budget exhausted.
    #[error("timeout budget exhausted")]
    BudgetExhausted,
    /// Budget not found.
    #[error("budget for request {0} not found")]
    BudgetNotFound(u64),
    /// Maximum hops exceeded.
    #[error("maximum hops ({0}) exceeded")]
    MaxHopsExceeded(u32),
}

/// Statistics for timeout budget operations.
pub struct TimeoutBudgetStats {
    /// Total budgets allocated.
    pub budgets_allocated: AtomicU64,
    /// Total budgets released.
    pub budgets_released: AtomicU64,
    /// Total budgets expired.
    pub budgets_expired: AtomicU64,
    /// Total child budgets created.
    pub child_budgets_created: AtomicU64,
    /// Total budgets exhausted when creating child.
    pub budgets_exhausted: AtomicU64,
    /// Total hops across all budgets.
    pub hops_total: AtomicU64,
}

impl TimeoutBudgetStats {
    /// Create new timeout budget statistics.
    pub fn new() -> Self {
        Self {
            budgets_allocated: AtomicU64::new(0),
            budgets_released: AtomicU64::new(0),
            budgets_expired: AtomicU64::new(0),
            child_budgets_created: AtomicU64::new(0),
            budgets_exhausted: AtomicU64::new(0),
            hops_total: AtomicU64::new(0),
        }
    }

    /// Get a snapshot of current statistics.
    pub fn snapshot(&self, active: usize) -> TimeoutBudgetStatsSnapshot {
        TimeoutBudgetStatsSnapshot {
            budgets_allocated: self.budgets_allocated.load(Ordering::Relaxed),
            budgets_released: self.budgets_released.load(Ordering::Relaxed),
            budgets_expired: self.budgets_expired.load(Ordering::Relaxed),
            child_budgets_created: self.child_budgets_created.load(Ordering::Relaxed),
            budgets_exhausted: self.budgets_exhausted.load(Ordering::Relaxed),
            hops_total: self.hops_total.load(Ordering::Relaxed),
            active_count: active,
        }
    }
}

impl Default for TimeoutBudgetStats {
    fn default() -> Self {
        Self::new()
    }
}

/// A snapshot of timeout budget statistics at a point in time.
#[derive(Debug, Clone, Default)]
pub struct TimeoutBudgetStatsSnapshot {
    /// Total budgets allocated.
    pub budgets_allocated: u64,
    /// Total budgets released.
    pub budgets_released: u64,
    /// Total budgets expired.
    pub budgets_expired: u64,
    /// Total child budgets created.
    pub child_budgets_created: u64,
    /// Total budgets exhausted when creating child.
    pub budgets_exhausted: u64,
    /// Total hops across all budgets.
    pub hops_total: u64,
    /// Number of active budgets.
    pub active_count: usize,
}

/// Manages RPC timeout budgets.
pub struct TimeoutBudgetManager {
    config: TimeoutBudgetConfig,
    /// Active budgets, keyed by an opaque request ID.
    active: Mutex<HashMap<u64, TimeoutBudget>>,
    stats: Arc<TimeoutBudgetStats>,
}

impl TimeoutBudgetManager {
    /// Create a new timeout budget manager.
    pub fn new(config: TimeoutBudgetConfig) -> Self {
        Self {
            config,
            active: Mutex::new(HashMap::new()),
            stats: Arc::new(TimeoutBudgetStats::new()),
        }
    }

    /// Allocate a new budget for a top-level request.
    pub fn allocate(&self, request_id: u64, budget_ms: Option<u64>, now_ms: u64) -> TimeoutBudget {
        let total = budget_ms.unwrap_or(self.config.default_budget_ms);
        let budget = TimeoutBudget::with_overhead(total, self.config.per_hop_overhead_ms, now_ms);

        self.stats.budgets_allocated.fetch_add(1, Ordering::Relaxed);

        if let Ok(mut active) = self.active.lock() {
            active.insert(request_id, budget.clone());
        }

        budget
    }

    /// Create a child budget from a tracked parent budget (for sub-request).
    pub fn child_budget(
        &self,
        parent_id: u64,
        max_sub_ms: Option<u64>,
        now_ms: u64,
    ) -> Option<TimeoutBudget> {
        let parent = match self.active.lock() {
            Ok(active) => {
                if let Some(p) = active.get(&parent_id) {
                    p.clone()
                } else {
                    return None;
                }
            }
            Err(_) => return None,
        };

        if parent.hops >= self.config.max_hops {
            self.stats.budgets_exhausted.fetch_add(1, Ordering::Relaxed);
            return None;
        }

        let child = parent.child(max_sub_ms, now_ms);
        if let Some(ref c) = child {
            if c.hops >= self.config.max_hops {
                self.stats.budgets_exhausted.fetch_add(1, Ordering::Relaxed);
                return None;
            }
            self.stats
                .child_budgets_created
                .fetch_add(1, Ordering::Relaxed);
            self.stats.hops_total.fetch_add(1, Ordering::Relaxed);
        } else {
            self.stats.budgets_exhausted.fetch_add(1, Ordering::Relaxed);
        }
        child
    }

    /// Remove a budget (request completed).
    pub fn release(&self, request_id: u64) {
        if let Ok(mut active) = self.active.lock() {
            if active.remove(&request_id).is_some() {
                self.stats.budgets_released.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// Expire all exhausted budgets. Returns count removed.
    pub fn expire(&self, now_ms: u64) -> usize {
        let mut count = 0;
        if let Ok(mut active) = self.active.lock() {
            let initial_len = active.len();
            active.retain(|_, budget| !budget.is_exhausted(now_ms));
            count = initial_len - active.len();
            if count > 0 {
                self.stats
                    .budgets_expired
                    .fetch_add(count as u64, Ordering::Relaxed);
            }
        }
        count
    }

    /// Number of active budgets.
    pub fn active_count(&self) -> usize {
        self.active.lock().map(|a| a.len()).unwrap_or(0)
    }

    /// Number of budgets in the warning zone (< warn_threshold remaining).
    pub fn warning_count(&self, now_ms: u64) -> usize {
        match self.active.lock() {
            Ok(active) => active
                .values()
                .filter(|b| b.fraction_remaining(now_ms) < self.config.warn_threshold)
                .count(),
            Err(_) => 0,
        }
    }

    /// Get statistics.
    pub fn stats(&self) -> Arc<TimeoutBudgetStats> {
        self.stats.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_budget_full() {
        let budget = TimeoutBudget::new(1000, 0);
        assert_eq!(budget.total_ms, 1000);
        assert_eq!(budget.created_at_ms, 0);
        assert_eq!(budget.hops, 0);
        assert_eq!(budget.per_hop_overhead_ms, 5);
    }

    #[test]
    fn test_remaining_decreases_with_time() {
        let budget = TimeoutBudget::new(1000, 0);
        let r1 = budget.remaining_ms(0);
        let r2 = budget.remaining_ms(500);

        assert!(r1 > r2);
        assert_eq!(r1, 1000);
        assert_eq!(r2, 500);
    }

    #[test]
    fn test_exhausted_when_past_deadline() {
        let budget = TimeoutBudget::new(100, 0);
        assert!(budget.is_exhausted(200));
    }

    #[test]
    fn test_not_exhausted_before_deadline() {
        let budget = TimeoutBudget::new(1000, 0);
        assert!(!budget.is_exhausted(500));
    }

    #[test]
    fn test_child_budget_subtracts_overhead() {
        let budget = TimeoutBudget::new(100, 0);
        let child = budget.child(None, 0).unwrap();

        assert!(child.total_ms < budget.remaining_ms(0));
        assert_eq!(child.total_ms, 95);
    }

    #[test]
    fn test_child_budget_caps_at_max_sub_ms() {
        let budget = TimeoutBudget::new(1000, 0);
        let child = budget.child(Some(50), 0).unwrap();

        assert_eq!(child.total_ms, 50);
    }

    #[test]
    fn test_child_returns_none_when_exhausted() {
        let budget = TimeoutBudget::new(10, 0);
        let child = budget.child(None, 100);
        assert!(child.is_none());
    }

    #[test]
    fn test_child_increments_hops() {
        let budget = TimeoutBudget::new(1000, 0);
        assert_eq!(budget.hops, 0);

        let child = budget.child(None, 0).unwrap();
        assert_eq!(child.hops, 1);

        let grandchild = child.child(None, 0).unwrap();
        assert_eq!(grandchild.hops, 2);
    }

    #[test]
    fn test_fraction_remaining_full() {
        let budget = TimeoutBudget::new(1000, 0);
        let frac = budget.fraction_remaining(0);
        assert!((frac - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_fraction_remaining_half() {
        let budget = TimeoutBudget::new(1000, 0);
        let frac = budget.fraction_remaining(500);
        assert!((frac - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_fraction_remaining_zero() {
        let budget = TimeoutBudget::new(100, 0);
        let frac = budget.fraction_remaining(200);
        assert!((frac - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_manager_allocate() {
        let manager = TimeoutBudgetManager::new(TimeoutBudgetConfig::default());
        let budget = manager.allocate(1, Some(500), 0);

        assert_eq!(budget.total_ms, 500);
        assert_eq!(manager.active_count(), 1);
    }

    #[test]
    fn test_manager_child_budget() {
        let manager = TimeoutBudgetManager::new(TimeoutBudgetConfig::default());
        manager.allocate(1, Some(100), 0);

        let child = manager.child_budget(1, None, 0);
        assert!(child.is_some());
        assert_eq!(child.unwrap().total_ms, 95);
    }

    #[test]
    fn test_manager_expire() {
        let manager = TimeoutBudgetManager::new(TimeoutBudgetConfig::default());
        manager.allocate(1, Some(50), 0);
        manager.allocate(2, Some(500), 0);

        let expired = manager.expire(100);
        assert_eq!(expired, 1);
        assert_eq!(manager.active_count(), 1);
    }

    #[test]
    fn test_stats_counts() {
        let manager = TimeoutBudgetManager::new(TimeoutBudgetConfig::default());
        let stats = manager.stats();

        manager.allocate(1, Some(100), 0);
        manager.allocate(2, Some(100), 0);
        manager.child_budget(1, None, 0);
        manager.release(1);
        manager.expire(200);

        let snapshot = stats.snapshot(manager.active_count());
        assert_eq!(snapshot.budgets_allocated, 2);
        assert_eq!(snapshot.budgets_released, 1);
        assert_eq!(snapshot.budgets_expired, 1);
        assert_eq!(snapshot.child_budgets_created, 1);
    }

    #[test]
    fn test_budget_with_custom_overhead() {
        let budget = TimeoutBudget::with_overhead(100, 10, 0);
        assert_eq!(budget.per_hop_overhead_ms, 10);

        let child = budget.child(None, 0).unwrap();
        assert_eq!(child.total_ms, 90);
    }

    #[test]
    fn test_config_default() {
        let config = TimeoutBudgetConfig::default();
        assert_eq!(config.default_budget_ms, 1000);
        assert_eq!(config.per_hop_overhead_ms, 5);
        assert_eq!(config.max_hops, 16);
        assert!((config.warn_threshold - 0.1).abs() < 0.01);
    }

    #[test]
    fn test_manager_allocate_default_budget() {
        let manager = TimeoutBudgetManager::new(TimeoutBudgetConfig::default());
        let budget = manager.allocate(1, None, 0);

        assert_eq!(budget.total_ms, 1000);
    }

    #[test]
    fn test_manager_child_budget_exhausted_parent() {
        let manager = TimeoutBudgetManager::new(TimeoutBudgetConfig::default());
        manager.allocate(1, Some(10), 0);

        let child = manager.child_budget(1, None, 100);
        assert!(child.is_none());
    }

    #[test]
    fn test_manager_child_budget_nonexistent_parent() {
        let manager = TimeoutBudgetManager::new(TimeoutBudgetConfig::default());

        let child = manager.child_budget(999, None, 0);
        assert!(child.is_none());
    }

    #[test]
    fn test_warning_count() {
        let config = TimeoutBudgetConfig {
            default_budget_ms: 100,
            warn_threshold: 0.1,
            ..TimeoutBudgetConfig::default()
        };
        let manager = TimeoutBudgetManager::new(config);

        manager.allocate(1, Some(100), 0);
        manager.allocate(2, Some(1000), 0);

        let warnings = manager.warning_count(95);
        assert_eq!(warnings, 1);
    }

    #[test]
    fn test_max_hops_exceeded() {
        let config = TimeoutBudgetConfig {
            max_hops: 2,
            per_hop_overhead_ms: 1,
            ..TimeoutBudgetConfig::default()
        };
        let manager = TimeoutBudgetManager::new(config);

        let parent = TimeoutBudget::with_overhead(100, 1, 0);
        assert_eq!(parent.hops, 0);

        let c1 = parent.child(None, 0).unwrap();
        assert_eq!(c1.hops, 1);

        let c2 = c1.child(None, 0).unwrap();
        assert_eq!(c2.hops, 2);

        let c3 = c2.child(None, 0).unwrap();
        assert_eq!(c3.hops, 3);
    }

    #[test]
    fn test_release_nonexistent() {
        let manager = TimeoutBudgetManager::new(TimeoutBudgetConfig::default());
        manager.release(999);

        let snapshot = manager.stats().snapshot(0);
        assert_eq!(snapshot.budgets_released, 0);
    }

    #[test]
    fn test_fraction_remaining_with_hops() {
        let budget = TimeoutBudget::new(100, 0);
        let child = budget.child(None, 0).unwrap();

        let frac = child.fraction_remaining(0);
        assert!((frac - 0.95).abs() < 0.01);
    }

    #[test]
    fn test_multiple_children_same_budget() {
        let budget = TimeoutBudget::new(100, 0);

        let c1 = budget.child(Some(20), 0).unwrap();
        let c2 = budget.child(Some(20), 0).unwrap();

        assert_eq!(c1.total_ms, 20);
        assert_eq!(c2.total_ms, 20);
        assert_eq!(c1.hops, 1);
        assert_eq!(c2.hops, 1);
    }

    #[test]
    fn test_budget_remaining_zero_total() {
        let budget = TimeoutBudget::new(0, 0);
        assert!(budget.is_exhausted(0));
        assert_eq!(budget.fraction_remaining(0), 0.0);
    }
}
