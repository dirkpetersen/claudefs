//! Drain-aware connection wrapper for graceful node shutdown.
//!
//! Integrates with the existing `drain.rs` module (DrainController/DrainGuard) to
//! make connection-level operations drain-aware. When a drain is signaled (e.g., for
//! graceful node shutdown), new requests are rejected and in-flight requests are tracked
//! until completion.
//!
//! This module provides a wrapper around connection-level state that coordinates with
//! the drain protocol used by A11 (Infrastructure) for graceful cluster node shutdown.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use thiserror::Error;

/// State of a drain-aware connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConnDrainState {
    /// Normal operation — accepting new requests.
    Active = 0,
    /// Drain signaled — no new requests, waiting for in-flight to complete.
    Draining = 1,
    /// All in-flight requests completed — ready for connection close.
    Drained = 2,
}

/// Configuration for drain-aware connections.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnDrainConfig {
    /// Maximum time to wait for drain completion in ms.
    pub drain_timeout_ms: u64,
    /// How long to wait before force-closing (after drain_timeout_ms).
    pub force_close_delay_ms: u64,
}

impl Default for ConnDrainConfig {
    fn default() -> Self {
        Self {
            drain_timeout_ms: 30000,
            force_close_delay_ms: 5000,
        }
    }
}

/// Error type for drain-aware operations.
#[derive(Debug, Error)]
pub enum ConnDrainError {
    /// Connection is draining — no new requests accepted.
    #[error("connection is draining — no new requests accepted")]
    Draining,
    /// Connection is already drained.
    #[error("connection is already drained")]
    AlreadyDrained,
    /// Drain timed out after the specified duration.
    #[error("drain timed out after {0}ms")]
    DrainTimedOut(u64),
}

/// A drain-aware connection tracker.
///
/// Wraps per-connection inflight request tracking with drain awareness.
pub struct ConnDrainTracker {
    config: ConnDrainConfig,
    #[allow(dead_code)]
    conn_id: u64,
    state: Mutex<ConnDrainState>,
    inflight_count: AtomicU64,
    drain_started_at_ms: Mutex<Option<u64>>,
    stats: Arc<ConnDrainStats>,
}

impl ConnDrainTracker {
    /// Creates a new drain-aware connection tracker.
    pub fn new(conn_id: u64, config: ConnDrainConfig) -> Self {
        Self {
            config,
            conn_id,
            state: Mutex::new(ConnDrainState::Active),
            inflight_count: AtomicU64::new(0),
            drain_started_at_ms: Mutex::new(None),
            stats: Arc::new(ConnDrainStats::new()),
        }
    }

    /// Creates a new tracker with shared stats (used by manager).
    pub fn with_stats(conn_id: u64, config: ConnDrainConfig, stats: Arc<ConnDrainStats>) -> Self {
        Self {
            config,
            conn_id,
            state: Mutex::new(ConnDrainState::Active),
            inflight_count: AtomicU64::new(0),
            drain_started_at_ms: Mutex::new(None),
            stats,
        }
    }

    /// Attempt to register a new in-flight request.
    /// Returns error if state is Draining or Drained.
    pub fn begin_request(&self) -> Result<(), ConnDrainError> {
        let state = self.state.lock().unwrap();
        match *state {
            ConnDrainState::Draining => {
                self.stats.requests_rejected.fetch_add(1, Ordering::Relaxed);
                Err(ConnDrainError::Draining)
            }
            ConnDrainState::Drained => {
                self.stats.requests_rejected.fetch_add(1, Ordering::Relaxed);
                Err(ConnDrainError::AlreadyDrained)
            }
            ConnDrainState::Active => {
                drop(state);
                self.inflight_count.fetch_add(1, Ordering::Relaxed);
                self.stats.total_requests.fetch_add(1, Ordering::Relaxed);
                Ok(())
            }
        }
    }

    /// Complete an in-flight request. Decrements inflight count.
    /// If state is Draining and inflight reaches 0, transitions to Drained.
    pub fn end_request(&self) {
        let prev = self.inflight_count.fetch_sub(1, Ordering::Relaxed);
        if prev == 1 {
            let mut state = self.state.lock().unwrap();
            if *state == ConnDrainState::Draining {
                *state = ConnDrainState::Drained;
                self.stats.drains_completed.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// Signal that this connection should drain (no new requests).
    /// Returns error if already draining/drained.
    pub fn begin_drain(&self, now_ms: u64) -> Result<(), ConnDrainError> {
        let mut state = self.state.lock().unwrap();
        match *state {
            ConnDrainState::Draining | ConnDrainState::Drained => {
                Err(ConnDrainError::AlreadyDrained)
            }
            ConnDrainState::Active => {
                *state = ConnDrainState::Draining;
                *self.drain_started_at_ms.lock().unwrap() = Some(now_ms);
                self.stats.drains_initiated.fetch_add(1, Ordering::Relaxed);

                if self.inflight_count.load(Ordering::Relaxed) == 0 {
                    *state = ConnDrainState::Drained;
                    self.stats.drains_completed.fetch_add(1, Ordering::Relaxed);
                }
                Ok(())
            }
        }
    }

    /// Check drain timeout. Returns DrainTimedOut if drain_timeout_ms has elapsed.
    pub fn check_drain_timeout(&self, now_ms: u64) -> Result<(), ConnDrainError> {
        let state = self.state.lock().unwrap();
        if *state != ConnDrainState::Draining {
            return Ok(());
        }

        let started_at = self.drain_started_at_ms.lock().unwrap();
        if let Some(started) = *started_at {
            if now_ms.saturating_sub(started) > self.config.drain_timeout_ms {
                self.stats.drains_timed_out.fetch_add(1, Ordering::Relaxed);
                return Err(ConnDrainError::DrainTimedOut(self.config.drain_timeout_ms));
            }
        }
        Ok(())
    }

    /// Current state.
    pub fn state(&self) -> ConnDrainState {
        *self.state.lock().unwrap()
    }

    /// Current in-flight request count.
    pub fn inflight_count(&self) -> u64 {
        self.inflight_count.load(Ordering::Relaxed)
    }

    /// Whether drain is complete (state == Drained).
    pub fn is_drained(&self) -> bool {
        self.state() == ConnDrainState::Drained
    }

    /// Returns the statistics for this tracker.
    pub fn stats(&self) -> Arc<ConnDrainStats> {
        Arc::clone(&self.stats)
    }
}

/// Manager for multiple drain-aware connections.
pub struct ConnDrainManager {
    config: ConnDrainConfig,
    connections: RwLock<HashMap<u64, ConnDrainTracker>>,
    stats: Arc<ConnDrainStats>,
}

impl ConnDrainManager {
    /// Creates a new manager with the given configuration.
    pub fn new(config: ConnDrainConfig) -> Self {
        Self {
            config,
            connections: RwLock::new(HashMap::new()),
            stats: Arc::new(ConnDrainStats::new()),
        }
    }

    /// Register a new connection.
    pub fn register(&self, conn_id: u64) {
        let tracker =
            ConnDrainTracker::with_stats(conn_id, self.config.clone(), Arc::clone(&self.stats));
        self.connections.write().unwrap().insert(conn_id, tracker);
        self.stats
            .connections_registered
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Remove a connection.
    pub fn remove(&self, conn_id: u64) {
        self.connections.write().unwrap().remove(&conn_id);
        self.stats
            .connections_removed
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Begin request on a connection. Returns error if draining/drained/not found.
    pub fn begin_request(&self, conn_id: u64) -> Result<(), ConnDrainError> {
        let connections = self.connections.read().unwrap();
        let tracker = connections
            .get(&conn_id)
            .ok_or(ConnDrainError::AlreadyDrained)?;
        tracker.begin_request()
    }

    /// End request on a connection.
    pub fn end_request(&self, conn_id: u64) {
        let connections = self.connections.read().unwrap();
        if let Some(tracker) = connections.get(&conn_id) {
            tracker.end_request();
        }
    }

    /// Signal drain on a specific connection.
    pub fn drain_connection(&self, conn_id: u64, now_ms: u64) -> Result<(), ConnDrainError> {
        let connections = self.connections.read().unwrap();
        let tracker = connections
            .get(&conn_id)
            .ok_or(ConnDrainError::AlreadyDrained)?;
        tracker.begin_drain(now_ms)
    }

    /// Signal drain on ALL connections.
    pub fn drain_all(&self, now_ms: u64) {
        let connections = self.connections.read().unwrap();
        for tracker in connections.values() {
            let _ = tracker.begin_drain(now_ms);
        }
    }

    /// Check drain timeouts across all connections. Returns conn_ids that timed out.
    pub fn check_timeouts(&self, now_ms: u64) -> Vec<u64> {
        let connections = self.connections.read().unwrap();
        let mut timed_out = Vec::new();

        for (id, tracker) in connections.iter() {
            if tracker.check_drain_timeout(now_ms).is_err() {
                timed_out.push(*id);
            }
        }

        timed_out
    }

    /// Count of connections in each state. Returns (active, draining, drained).
    pub fn state_counts(&self) -> (usize, usize, usize) {
        let connections = self.connections.read().unwrap();
        let mut active = 0;
        let mut draining = 0;
        let mut drained = 0;

        for tracker in connections.values() {
            match tracker.state() {
                ConnDrainState::Active => active += 1,
                ConnDrainState::Draining => draining += 1,
                ConnDrainState::Drained => drained += 1,
            }
        }

        (active, draining, drained)
    }

    /// Returns the statistics for this manager.
    pub fn stats(&self) -> Arc<ConnDrainStats> {
        Arc::clone(&self.stats)
    }
}

impl Default for ConnDrainManager {
    fn default() -> Self {
        Self::new(ConnDrainConfig::default())
    }
}

/// Statistics for drain-aware connections.
pub struct ConnDrainStats {
    /// Total connections registered.
    pub connections_registered: AtomicU64,
    /// Total connections removed.
    pub connections_removed: AtomicU64,
    /// Total drains initiated.
    pub drains_initiated: AtomicU64,
    /// Total drains completed successfully.
    pub drains_completed: AtomicU64,
    /// Total drains that timed out.
    pub drains_timed_out: AtomicU64,
    /// Total requests rejected due to draining.
    pub requests_rejected: AtomicU64,
    /// Total requests processed.
    pub total_requests: AtomicU64,
}

impl ConnDrainStats {
    /// Creates new empty statistics.
    pub fn new() -> Self {
        Self {
            connections_registered: AtomicU64::new(0),
            connections_removed: AtomicU64::new(0),
            drains_initiated: AtomicU64::new(0),
            drains_completed: AtomicU64::new(0),
            drains_timed_out: AtomicU64::new(0),
            requests_rejected: AtomicU64::new(0),
            total_requests: AtomicU64::new(0),
        }
    }

    /// Returns a snapshot of current statistics.
    pub fn snapshot(
        &self,
        active: usize,
        draining: usize,
        drained: usize,
    ) -> ConnDrainStatsSnapshot {
        ConnDrainStatsSnapshot {
            connections_registered: self.connections_registered.load(Ordering::Relaxed),
            connections_removed: self.connections_removed.load(Ordering::Relaxed),
            drains_initiated: self.drains_initiated.load(Ordering::Relaxed),
            drains_completed: self.drains_completed.load(Ordering::Relaxed),
            drains_timed_out: self.drains_timed_out.load(Ordering::Relaxed),
            requests_rejected: self.requests_rejected.load(Ordering::Relaxed),
            total_requests: self.total_requests.load(Ordering::Relaxed),
            active_connections: active,
            draining_connections: draining,
            drained_connections: drained,
        }
    }
}

impl Default for ConnDrainStats {
    fn default() -> Self {
        Self::new()
    }
}

/// Snapshot of drain-aware connection statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnDrainStatsSnapshot {
    /// Total connections registered.
    pub connections_registered: u64,
    /// Total connections removed.
    pub connections_removed: u64,
    /// Total drains initiated.
    pub drains_initiated: u64,
    /// Total drains completed successfully.
    pub drains_completed: u64,
    /// Total drains that timed out.
    pub drains_timed_out: u64,
    /// Total requests rejected due to draining.
    pub requests_rejected: u64,
    /// Total requests processed.
    pub total_requests: u64,
    /// Currently active connections.
    pub active_connections: usize,
    /// Currently draining connections.
    pub draining_connections: usize,
    /// Currently drained connections.
    pub drained_connections: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_tracker_is_active() {
        let tracker = ConnDrainTracker::new(1, ConnDrainConfig::default());
        assert_eq!(tracker.state(), ConnDrainState::Active);
        assert_eq!(tracker.inflight_count(), 0);
    }

    #[test]
    fn test_begin_request_success() {
        let tracker = ConnDrainTracker::new(1, ConnDrainConfig::default());
        assert!(tracker.begin_request().is_ok());
        assert_eq!(tracker.inflight_count(), 1);
    }

    #[test]
    fn test_begin_request_while_draining() {
        let tracker = ConnDrainTracker::new(1, ConnDrainConfig::default());
        tracker.begin_request().unwrap();
        tracker.begin_drain(1000).unwrap();
        let result = tracker.begin_request();
        assert!(matches!(result, Err(ConnDrainError::Draining)));
    }

    #[test]
    fn test_begin_request_while_drained() {
        let tracker = ConnDrainTracker::new(1, ConnDrainConfig::default());
        tracker.begin_drain(1000).unwrap();
        let result = tracker.begin_request();
        assert!(
            matches!(result, Err(ConnDrainError::Draining))
                || matches!(result, Err(ConnDrainError::AlreadyDrained))
        );
    }

    #[test]
    fn test_end_request_decrements_count() {
        let tracker = ConnDrainTracker::new(1, ConnDrainConfig::default());
        tracker.begin_request().unwrap();
        assert_eq!(tracker.inflight_count(), 1);
        tracker.end_request();
        assert_eq!(tracker.inflight_count(), 0);
    }

    #[test]
    fn test_drain_transitions_to_draining() {
        let tracker = ConnDrainTracker::new(1, ConnDrainConfig::default());
        tracker.begin_request().unwrap();
        tracker.begin_drain(1000).unwrap();
        assert_eq!(tracker.state(), ConnDrainState::Draining);
    }

    #[test]
    fn test_drain_with_no_inflight_transitions_to_drained() {
        let tracker = ConnDrainTracker::new(1, ConnDrainConfig::default());
        tracker.begin_drain(1000).unwrap();
        assert_eq!(tracker.state(), ConnDrainState::Drained);
    }

    #[test]
    fn test_drain_completes_when_last_request_ends() {
        let tracker = ConnDrainTracker::new(1, ConnDrainConfig::default());
        tracker.begin_request().unwrap();
        tracker.begin_drain(1000).unwrap();
        assert_eq!(tracker.state(), ConnDrainState::Draining);
        tracker.end_request();
        assert_eq!(tracker.state(), ConnDrainState::Drained);
    }

    #[test]
    fn test_drain_already_draining() {
        let tracker = ConnDrainTracker::new(1, ConnDrainConfig::default());
        tracker.begin_request().unwrap();
        tracker.begin_drain(1000).unwrap();
        let result = tracker.begin_drain(2000);
        assert!(matches!(result, Err(ConnDrainError::AlreadyDrained)));
    }

    #[test]
    fn test_drain_timeout() {
        let config = ConnDrainConfig {
            drain_timeout_ms: 1000,
            force_close_delay_ms: 500,
        };
        let tracker = ConnDrainTracker::new(1, config);
        tracker.begin_request().unwrap();
        tracker.begin_drain(1000).unwrap();

        let result = tracker.check_drain_timeout(3000);
        assert!(matches!(result, Err(ConnDrainError::DrainTimedOut(1000))));
    }

    #[test]
    fn test_drain_timeout_not_expired() {
        let config = ConnDrainConfig {
            drain_timeout_ms: 5000,
            force_close_delay_ms: 500,
        };
        let tracker = ConnDrainTracker::new(1, config);
        tracker.begin_request().unwrap();
        tracker.begin_drain(1000).unwrap();

        let result = tracker.check_drain_timeout(2000);
        assert!(result.is_ok());
    }

    #[test]
    fn test_manager_register_and_remove() {
        let manager = ConnDrainManager::new(ConnDrainConfig::default());
        manager.register(1);
        manager.register(2);

        let (active, _, _) = manager.state_counts();
        assert_eq!(active, 2);

        manager.remove(1);
        let (active, _, _) = manager.state_counts();
        assert_eq!(active, 1);
    }

    #[test]
    fn test_manager_drain_all() {
        let manager = ConnDrainManager::new(ConnDrainConfig::default());
        manager.register(1);
        manager.register(2);

        manager.drain_all(1000);

        let (active, draining, drained) = manager.state_counts();
        assert_eq!(active, 0);
        assert_eq!(draining, 0);
        assert_eq!(drained, 2);
    }

    #[test]
    fn test_manager_state_counts() {
        let manager = ConnDrainManager::new(ConnDrainConfig::default());
        manager.register(1);
        manager.register(2);
        manager.register(3);

        manager.begin_request(1).unwrap();
        manager.begin_request(2).unwrap();

        manager.drain_connection(1, 1000).unwrap();
        manager.drain_connection(2, 1000).unwrap();
        manager.drain_connection(3, 1000).unwrap();

        let (active, draining, drained) = manager.state_counts();
        assert_eq!(active, 0);
        assert_eq!(draining, 2);
        assert_eq!(drained, 1);
    }

    #[test]
    fn test_stats_counts() {
        let manager = ConnDrainManager::new(ConnDrainConfig::default());
        let stats = manager.stats();

        manager.register(1);
        assert_eq!(stats.connections_registered.load(Ordering::Relaxed), 1);

        manager.begin_request(1).unwrap();
        assert_eq!(stats.total_requests.load(Ordering::Relaxed), 1);

        manager.drain_connection(1, 1000).unwrap();
        assert_eq!(stats.drains_initiated.load(Ordering::Relaxed), 1);

        manager.end_request(1);
        assert_eq!(stats.drains_completed.load(Ordering::Relaxed), 1);

        manager.remove(1);
        assert_eq!(stats.connections_removed.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_config_default() {
        let config = ConnDrainConfig::default();
        assert_eq!(config.drain_timeout_ms, 30000);
        assert_eq!(config.force_close_delay_ms, 5000);
    }

    #[test]
    fn test_stats_snapshot() {
        let stats = ConnDrainStats::new();
        stats.connections_registered.store(10, Ordering::Relaxed);
        stats.drains_completed.store(3, Ordering::Relaxed);

        let snapshot = stats.snapshot(5, 2, 3);
        assert_eq!(snapshot.connections_registered, 10);
        assert_eq!(snapshot.drains_completed, 3);
        assert_eq!(snapshot.active_connections, 5);
        assert_eq!(snapshot.draining_connections, 2);
        assert_eq!(snapshot.drained_connections, 3);
    }

    #[test]
    fn test_error_display() {
        let err = ConnDrainError::Draining;
        assert!(err.to_string().contains("draining"));

        let err = ConnDrainError::AlreadyDrained;
        assert!(err.to_string().contains("already drained"));

        let err = ConnDrainError::DrainTimedOut(5000);
        assert!(err.to_string().contains("timed out"));
    }

    #[test]
    fn test_manager_begin_request_unknown_connection() {
        let manager = ConnDrainManager::new(ConnDrainConfig::default());
        let result = manager.begin_request(999);
        assert!(matches!(result, Err(ConnDrainError::AlreadyDrained)));
    }

    #[test]
    fn test_multiple_requests_tracking() {
        let tracker = ConnDrainTracker::new(1, ConnDrainConfig::default());
        tracker.begin_request().unwrap();
        tracker.begin_request().unwrap();
        tracker.begin_request().unwrap();
        assert_eq!(tracker.inflight_count(), 3);

        tracker.end_request();
        tracker.end_request();
        assert_eq!(tracker.inflight_count(), 1);
    }

    #[test]
    fn test_drain_rejects_new_requests() {
        let tracker = ConnDrainTracker::new(1, ConnDrainConfig::default());
        tracker.begin_request().unwrap();
        tracker.begin_drain(1000).unwrap();

        let result = tracker.begin_request();
        assert!(result.is_err());

        let stats = tracker.stats();
        assert_eq!(stats.requests_rejected.load(Ordering::Relaxed), 1);
    }
}
