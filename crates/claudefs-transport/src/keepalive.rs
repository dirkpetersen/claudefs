//! Keep-alive tracking for ClaudeFS transport connections.
//!
//! This module provides keep-alive heartbeat monitoring to detect dead connections.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const STATE_ACTIVE: u8 = 0;
const STATE_WARNING: u8 = 1;
const STATE_DEAD: u8 = 2;
const STATE_DISABLED: u8 = 3;

/// Connection keep-alive state indicating health status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeepAliveState {
    /// Connection is healthy and responsive.
    Active,
    /// Connection has missed one heartbeat, may be degraded.
    Warning,
    /// Connection has missed multiple heartbeats and is considered dead.
    Dead,
    /// Keep-alive tracking is disabled for this connection.
    Disabled,
}

impl From<u8> for KeepAliveState {
    fn from(raw: u8) -> Self {
        match raw {
            STATE_ACTIVE => KeepAliveState::Active,
            STATE_WARNING => KeepAliveState::Warning,
            STATE_DEAD => KeepAliveState::Dead,
            STATE_DISABLED => KeepAliveState::Disabled,
            _ => KeepAliveState::Active,
        }
    }
}

impl From<KeepAliveState> for u8 {
    fn from(state: KeepAliveState) -> Self {
        match state {
            KeepAliveState::Active => STATE_ACTIVE,
            KeepAliveState::Warning => STATE_WARNING,
            KeepAliveState::Dead => STATE_DEAD,
            KeepAliveState::Disabled => STATE_DISABLED,
        }
    }
}

/// Configuration for keep-alive heartbeat monitoring.
#[derive(Debug, Clone)]
pub struct KeepAliveConfig {
    /// Interval between heartbeat checks.
    pub interval: Duration,
    /// Timeout waiting for a response before considering it a miss.
    pub timeout: Duration,
    /// Maximum number of missed heartbeats before marking as dead.
    pub max_missed: u32,
    /// Whether keep-alive tracking is enabled.
    pub enabled: bool,
}

impl Default for KeepAliveConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(10),
            timeout: Duration::from_secs(5),
            max_missed: 3,
            enabled: true,
        }
    }
}

/// Statistics for keep-alive tracking of a single connection.
#[derive(Debug, Clone, Default)]
pub struct KeepAliveStats {
    /// Current keep-alive state as raw value.
    pub state: u8,
    /// Number of consecutive missed heartbeats.
    pub missed_count: u32,
    /// Total number of heartbeats sent.
    pub total_sent: u64,
    /// Total number of heartbeats received.
    pub total_received: u64,
    /// Total number of timeout events.
    pub total_timeouts: u64,
    /// Average round-trip time in microseconds, if available.
    pub average_rtt_us: Option<u64>,
}

/// Tracks keep-alive state for a single connection.
pub struct KeepAliveTracker {
    config: KeepAliveConfig,
    state: AtomicU8,
    missed_count: AtomicU32,
    last_heartbeat_sent: Mutex<Option<Instant>>,
    last_heartbeat_received: Mutex<Option<Instant>>,
    total_sent: AtomicU64,
    total_received: AtomicU64,
    total_timeouts: AtomicU64,
    rtt_sum_us: AtomicU64,
    rtt_count: AtomicU64,
}

impl KeepAliveTracker {
    /// Creates a new tracker with the given configuration.
    pub fn new(config: KeepAliveConfig) -> Self {
        let initial_state = if config.enabled {
            STATE_ACTIVE
        } else {
            STATE_DISABLED
        };
        Self {
            config,
            state: AtomicU8::new(initial_state),
            missed_count: AtomicU32::new(0),
            last_heartbeat_sent: Mutex::new(None),
            last_heartbeat_received: Mutex::new(None),
            total_sent: AtomicU64::new(0),
            total_received: AtomicU64::new(0),
            total_timeouts: AtomicU64::new(0),
            rtt_sum_us: AtomicU64::new(0),
            rtt_count: AtomicU64::new(0),
        }
    }

    /// Returns the current keep-alive state.
    pub fn state(&self) -> KeepAliveState {
        KeepAliveState::from(self.state.load(Ordering::SeqCst))
    }

    /// Records that a heartbeat was sent.
    pub fn record_sent(&self) {
        self.total_sent.fetch_add(1, Ordering::Relaxed);
        let mut last_sent = self.last_heartbeat_sent.lock().unwrap();
        *last_sent = Some(Instant::now());
    }

    /// Records that a heartbeat response was received.
    pub fn record_received(&self) {
        self.total_received.fetch_add(1, Ordering::Relaxed);
        self.missed_count.store(0, Ordering::SeqCst);

        let now = Instant::now();
        let last_sent = self.last_heartbeat_sent.lock().unwrap();
        if let Some(sent_time) = *last_sent {
            let rtt_us = now.duration_since(sent_time).as_micros() as u64;
            self.rtt_sum_us.fetch_add(rtt_us, Ordering::Relaxed);
            self.rtt_count.fetch_add(1, Ordering::Relaxed);
        }
        drop(last_sent);

        let mut last_received = self.last_heartbeat_received.lock().unwrap();
        *last_received = Some(now);

        let current_state = self.state.load(Ordering::SeqCst);
        if current_state != STATE_DISABLED && current_state != STATE_ACTIVE {
            self.state.store(STATE_ACTIVE, Ordering::SeqCst);
        }
    }

    /// Records a timeout (missed heartbeat).
    pub fn record_timeout(&self) {
        self.total_timeouts.fetch_add(1, Ordering::Relaxed);
        let missed = self.missed_count.fetch_add(1, Ordering::SeqCst) + 1;

        let current_state = self.state.load(Ordering::SeqCst);

        if current_state == STATE_DISABLED {
            return;
        }

        if missed == 1 && current_state == STATE_ACTIVE {
            self.state.store(STATE_WARNING, Ordering::SeqCst);
        } else if missed >= self.config.max_missed && current_state != STATE_DEAD {
            self.state.store(STATE_DEAD, Ordering::SeqCst);
        }
    }

    /// Returns the number of consecutive missed heartbeats.
    pub fn missed_count(&self) -> u32 {
        self.missed_count.load(Ordering::SeqCst)
    }

    /// Returns true if a heartbeat should be sent based on interval elapsed.
    pub fn should_send(&self) -> bool {
        let last_sent = self.last_heartbeat_sent.lock().unwrap();
        match *last_sent {
            None => true,
            Some(sent_time) => {
                let elapsed = sent_time.elapsed();
                elapsed >= self.config.interval
            }
        }
    }

    /// Returns true if the connection is considered alive (Active or Warning).
    pub fn is_alive(&self) -> bool {
        let state = self.state.load(Ordering::SeqCst);
        state == STATE_ACTIVE || state == STATE_WARNING
    }

    /// Resets the tracker state back to Active with zero missed count.
    pub fn reset(&self) {
        self.missed_count.store(0, Ordering::SeqCst);
        self.state.store(STATE_ACTIVE, Ordering::SeqCst);
    }

    /// Returns the average round-trip time, if RTT samples are available.
    pub fn average_rtt(&self) -> Option<Duration> {
        let count = self.rtt_count.load(Ordering::Relaxed);
        if count == 0 {
            return None;
        }
        let sum = self.rtt_sum_us.load(Ordering::Relaxed);
        Some(Duration::from_micros(sum / count))
    }

    /// Returns current keep-alive statistics.
    pub fn stats(&self) -> KeepAliveStats {
        let rtt_count = self.rtt_count.load(Ordering::Relaxed);
        let average_rtt_us = if rtt_count > 0 {
            Some(self.rtt_sum_us.load(Ordering::Relaxed) / rtt_count)
        } else {
            None
        };

        KeepAliveStats {
            state: self.state.load(Ordering::SeqCst),
            missed_count: self.missed_count.load(Ordering::SeqCst),
            total_sent: self.total_sent.load(Ordering::Relaxed),
            total_received: self.total_received.load(Ordering::Relaxed),
            total_timeouts: self.total_timeouts.load(Ordering::Relaxed),
            average_rtt_us,
        }
    }
}

/// Manages keep-alive tracking across multiple connections.
pub struct KeepAliveManager {
    config: KeepAliveConfig,
    trackers: Mutex<HashMap<String, KeepAliveTracker>>,
}

impl KeepAliveManager {
    /// Creates a new manager with the given configuration.
    pub fn new(config: KeepAliveConfig) -> Self {
        Self {
            config,
            trackers: Mutex::new(HashMap::new()),
        }
    }

    /// Adds a new connection to track.
    pub fn add_connection(&self, addr: &str) {
        let mut trackers = self.trackers.lock().unwrap();
        trackers.insert(addr.to_string(), KeepAliveTracker::new(self.config.clone()));
    }

    /// Removes a connection from tracking.
    pub fn remove_connection(&self, addr: &str) {
        let mut trackers = self.trackers.lock().unwrap();
        trackers.remove(addr);
    }

    /// Records a heartbeat sent event for a connection.
    pub fn record_sent(&self, addr: &str) {
        let trackers = self.trackers.lock().unwrap();
        if let Some(tracker) = trackers.get(addr) {
            tracker.record_sent();
        }
    }

    /// Records a heartbeat received event for a connection.
    pub fn record_received(&self, addr: &str) {
        let trackers = self.trackers.lock().unwrap();
        if let Some(tracker) = trackers.get(addr) {
            tracker.record_received();
        }
    }

    /// Records a timeout event for a connection.
    pub fn record_timeout(&self, addr: &str) {
        let trackers = self.trackers.lock().unwrap();
        if let Some(tracker) = trackers.get(addr) {
            tracker.record_timeout();
        }
    }

    /// Returns the keep-alive state for a connection, if tracked.
    pub fn connection_state(&self, addr: &str) -> Option<KeepAliveState> {
        let trackers = self.trackers.lock().unwrap();
        trackers.get(addr).map(|t| t.state())
    }

    /// Returns a list of addresses for dead connections.
    pub fn dead_connections(&self) -> Vec<String> {
        let trackers = self.trackers.lock().unwrap();
        trackers
            .iter()
            .filter(|(_, t)| t.state() == KeepAliveState::Dead)
            .map(|(addr, _)| addr.clone())
            .collect()
    }

    /// Returns a list of addresses for connections that need a heartbeat sent.
    pub fn connections_needing_heartbeat(&self) -> Vec<String> {
        let trackers = self.trackers.lock().unwrap();
        trackers
            .iter()
            .filter(|(_, t)| t.is_alive() && t.should_send())
            .map(|(addr, _)| addr.clone())
            .collect()
    }

    /// Returns keep-alive statistics for a connection, if tracked.
    pub fn stats(&self, addr: &str) -> Option<KeepAliveStats> {
        let trackers = self.trackers.lock().unwrap();
        trackers.get(addr).map(|t| t.stats())
    }

    /// Returns the number of tracked connections.
    pub fn connection_count(&self) -> usize {
        let trackers = self.trackers.lock().unwrap();
        trackers.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keepalive_config_default() {
        let config = KeepAliveConfig::default();
        assert_eq!(config.interval, Duration::from_secs(10));
        assert_eq!(config.timeout, Duration::from_secs(5));
        assert_eq!(config.max_missed, 3);
        assert!(config.enabled);
    }

    #[test]
    fn test_initial_state_active() {
        let config = KeepAliveConfig::default();
        let tracker = KeepAliveTracker::new(config);
        assert_eq!(tracker.state(), KeepAliveState::Active);
    }

    #[test]
    fn test_initial_state_disabled() {
        let config = KeepAliveConfig {
            interval: Duration::from_secs(10),
            timeout: Duration::from_secs(5),
            max_missed: 3,
            enabled: false,
        };
        let tracker = KeepAliveTracker::new(config);
        assert_eq!(tracker.state(), KeepAliveState::Disabled);
    }

    #[test]
    fn test_record_sent() {
        let config = KeepAliveConfig::default();
        let tracker = KeepAliveTracker::new(config);
        assert_eq!(tracker.stats().total_sent, 0);
        tracker.record_sent();
        assert_eq!(tracker.stats().total_sent, 1);
        tracker.record_sent();
        assert_eq!(tracker.stats().total_sent, 2);
    }

    #[test]
    fn test_record_received_resets_missed() {
        let config = KeepAliveConfig::default();
        let tracker = KeepAliveTracker::new(config);
        tracker.record_timeout();
        tracker.record_timeout();
        assert_eq!(tracker.missed_count(), 2);
        tracker.record_received();
        assert_eq!(tracker.missed_count(), 0);
    }

    #[test]
    fn test_record_timeout_increments_missed() {
        let config = KeepAliveConfig::default();
        let tracker = KeepAliveTracker::new(config);
        assert_eq!(tracker.missed_count(), 0);
        tracker.record_timeout();
        assert_eq!(tracker.missed_count(), 1);
        tracker.record_timeout();
        assert_eq!(tracker.missed_count(), 2);
    }

    #[test]
    fn test_warning_state() {
        let config = KeepAliveConfig::default();
        let tracker = KeepAliveTracker::new(config);
        assert_eq!(tracker.state(), KeepAliveState::Active);
        tracker.record_timeout();
        assert_eq!(tracker.state(), KeepAliveState::Warning);
    }

    #[test]
    fn test_dead_state() {
        let config = KeepAliveConfig {
            interval: Duration::from_secs(10),
            timeout: Duration::from_secs(5),
            max_missed: 3,
            enabled: true,
        };
        let tracker = KeepAliveTracker::new(config);
        assert_eq!(tracker.state(), KeepAliveState::Active);

        tracker.record_timeout();
        assert_eq!(tracker.state(), KeepAliveState::Warning);

        tracker.record_timeout();
        assert_eq!(tracker.state(), KeepAliveState::Warning);

        tracker.record_timeout();
        assert_eq!(tracker.state(), KeepAliveState::Dead);
    }

    #[test]
    fn test_is_alive_active() {
        let config = KeepAliveConfig::default();
        let tracker = KeepAliveTracker::new(config);
        assert!(tracker.is_alive());
    }

    #[test]
    fn test_is_alive_warning() {
        let config = KeepAliveConfig::default();
        let tracker = KeepAliveTracker::new(config);
        tracker.record_timeout();
        assert!(tracker.is_alive());
    }

    #[test]
    fn test_is_not_alive_dead() {
        let config = KeepAliveConfig {
            interval: Duration::from_secs(10),
            timeout: Duration::from_secs(5),
            max_missed: 3,
            enabled: true,
        };
        let tracker = KeepAliveTracker::new(config);
        tracker.record_timeout();
        tracker.record_timeout();
        tracker.record_timeout();
        assert!(!tracker.is_alive());
    }

    #[test]
    fn test_reset() {
        let config = KeepAliveConfig {
            interval: Duration::from_secs(10),
            timeout: Duration::from_secs(5),
            max_missed: 3,
            enabled: true,
        };
        let tracker = KeepAliveTracker::new(config);
        tracker.record_timeout();
        tracker.record_timeout();
        tracker.record_timeout();
        assert_eq!(tracker.state(), KeepAliveState::Dead);
        assert_eq!(tracker.missed_count(), 3);

        tracker.reset();

        assert_eq!(tracker.state(), KeepAliveState::Active);
        assert_eq!(tracker.missed_count(), 0);
    }

    #[test]
    fn test_should_send_initially() {
        let config = KeepAliveConfig::default();
        let tracker = KeepAliveTracker::new(config);
        assert!(tracker.should_send());
    }

    #[test]
    fn test_manager_add_remove() {
        let config = KeepAliveConfig::default();
        let manager = KeepAliveManager::new(config);

        assert_eq!(manager.connection_count(), 0);

        manager.add_connection("192.168.1.1:8080");
        assert_eq!(manager.connection_count(), 1);

        manager.add_connection("192.168.1.2:8080");
        assert_eq!(manager.connection_count(), 2);

        manager.remove_connection("192.168.1.1:8080");
        assert_eq!(manager.connection_count(), 1);
    }

    #[test]
    fn test_manager_dead_connections() {
        let config = KeepAliveConfig {
            interval: Duration::from_secs(10),
            timeout: Duration::from_secs(5),
            max_missed: 3,
            enabled: true,
        };
        let manager = KeepAliveManager::new(config);

        manager.add_connection("192.168.1.1:8080");
        manager.add_connection("192.168.1.2:8080");

        manager.record_timeout("192.168.1.1:8080");
        manager.record_timeout("192.168.1.1:8080");
        manager.record_timeout("192.168.1.1:8080");

        let dead = manager.dead_connections();
        assert_eq!(dead.len(), 1);
        assert!(dead.contains(&"192.168.1.1:8080".to_string()));
    }

    #[test]
    fn test_manager_connection_count() {
        let config = KeepAliveConfig::default();
        let manager = KeepAliveManager::new(config);

        manager.add_connection("192.168.1.1:8080");
        manager.add_connection("192.168.1.2:8080");
        manager.add_connection("192.168.1.3:8080");

        assert_eq!(manager.connection_count(), 3);

        manager.remove_connection("192.168.1.2:8080");
        assert_eq!(manager.connection_count(), 2);
    }

    #[test]
    fn test_average_rtt() {
        let config = KeepAliveConfig::default();
        let tracker = KeepAliveTracker::new(config);

        assert!(tracker.average_rtt().is_none());

        tracker.record_sent();
        std::thread::sleep(Duration::from_micros(100));
        tracker.record_received();

        let rtt = tracker.average_rtt();
        assert!(rtt.is_some());
        assert!(rtt.unwrap().as_micros() >= 50);

        tracker.record_sent();
        std::thread::sleep(Duration::from_micros(200));
        tracker.record_received();

        let rtt = tracker.average_rtt().unwrap();
        assert!(rtt.as_micros() >= 100);
    }

    #[test]
    fn test_stats() {
        let config = KeepAliveConfig::default();
        let tracker = KeepAliveTracker::new(config);

        let stats = tracker.stats();
        assert_eq!(stats.state, 0);
        assert_eq!(stats.missed_count, 0);
        assert_eq!(stats.total_sent, 0);
        assert_eq!(stats.total_received, 0);
        assert_eq!(stats.total_timeouts, 0);
        assert!(stats.average_rtt_us.is_none());

        tracker.record_sent();
        tracker.record_timeout();

        let stats = tracker.stats();
        assert_eq!(stats.total_sent, 1);
        assert_eq!(stats.total_timeouts, 1);
        assert_eq!(stats.missed_count, 1);
    }
}
