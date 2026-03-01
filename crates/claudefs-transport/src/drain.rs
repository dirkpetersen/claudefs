//! Connection draining for node removal during online scaling.
//!
//! This module provides graceful drain functionality for removing nodes from
//! a distributed filesystem cluster without disrupting ongoing operations.

use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tracing::{debug, info};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
/// Represents the current state of the drain process.
pub enum DrainState {
    /// Normal operation, accepting new requests.
    Active = 0,
    /// Rejecting new requests, waiting for in-flight to complete.
    Draining = 1,
    /// All in-flight completed, ready to shut down.
    Drained = 2,
    /// Timeout exceeded, forcefully closed.
    ForceClosed = 3,
}

impl From<u8> for DrainState {
    fn from(value: u8) -> Self {
        match value {
            0 => DrainState::Active,
            1 => DrainState::Draining,
            2 => DrainState::Drained,
            3 => DrainState::ForceClosed,
            _ => DrainState::Active,
        }
    }
}

#[derive(Debug, Clone)]
/// Configuration for the drain controller.
pub struct DrainConfig {
    /// Maximum time to wait for in-flight requests to complete.
    pub drain_timeout: Duration,
    /// Interval between checking if draining is complete.
    pub check_interval: Duration,
    /// Time after which connections are forcefully closed.
    pub force_close_after: Duration,
}

impl Default for DrainConfig {
    fn default() -> Self {
        Self {
            drain_timeout: Duration::from_secs(30),
            check_interval: Duration::from_millis(100),
            force_close_after: Duration::from_secs(60),
        }
    }
}

/// Callback listener for drain events.
#[derive(Default)]
pub struct DrainListener {
    /// Callback invoked when drain starts.
    pub on_drain_start: Option<Box<dyn Fn() + Send + Sync>>,
    /// Callback invoked when drain completes (all in-flight requests finished).
    pub on_drained: Option<Box<dyn Fn() + Send + Sync>>,
    /// Callback invoked when force close is triggered.
    pub on_force_close: Option<Box<dyn Fn() + Send + Sync>>,
}

impl std::fmt::Debug for DrainListener {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DrainListener")
            .field("on_drain_start", &self.on_drain_start.is_some())
            .field("on_drained", &self.on_drained.is_some())
            .field("on_force_close", &self.on_force_close.is_some())
            .finish()
    }
}

#[derive(Debug, Clone)]
/// Snapshot of current drain state and statistics.
pub struct DrainStats {
    /// Current drain state.
    pub state: DrainState,
    /// Number of in-flight requests.
    pub inflight_count: usize,
    /// Duration since drain started, if applicable.
    pub drain_duration: Option<Duration>,
}

/// Controller for managing connection draining during node removal.
pub struct DrainController {
    #[allow(dead_code)]
    config: DrainConfig,
    state: AtomicU8,
    inflight_count: AtomicUsize,
    drain_started_at: Mutex<Option<Instant>>,
    listeners: Mutex<Vec<DrainListener>>,
}

/// RAII guard that tracks in-flight requests. Decrements the count on drop.
pub struct DrainGuard<'a> {
    controller: &'a DrainController,
}

impl Drop for DrainGuard<'_> {
    fn drop(&mut self) {
        self.controller
            .inflight_count
            .fetch_sub(1, Ordering::SeqCst);
        debug!("DrainGuard dropped, inflight decremented");
    }
}

impl DrainController {
    /// Creates a new DrainController with the given configuration.
    #[must_use]
    pub fn new(config: DrainConfig) -> Self {
        Self {
            config,
            state: AtomicU8::new(DrainState::Active as u8),
            inflight_count: AtomicUsize::new(0),
            drain_started_at: Mutex::new(None),
            listeners: Mutex::new(Vec::new()),
        }
    }

    /// Returns the current drain state.
    pub fn state(&self) -> DrainState {
        DrainState::from(self.state.load(Ordering::SeqCst))
    }

    /// Returns true if the controller is accepting new requests (i.e., state is Active).
    pub fn is_accepting(&self) -> bool {
        self.state() == DrainState::Active
    }

    /// Begins the drain process. Transitions from Active to Draining.
    /// Returns false if already draining or drained.
    pub fn begin_drain(&self) -> bool {
        let current_state = self.state.load(Ordering::SeqCst);
        if current_state != DrainState::Active as u8 {
            return false;
        }

        let success = self
            .state
            .compare_exchange(
                DrainState::Active as u8,
                DrainState::Draining as u8,
                Ordering::SeqCst,
                Ordering::SeqCst,
            )
            .is_ok();

        if success {
            let now = Instant::now();
            *self.drain_started_at.lock().unwrap() = Some(now);

            info!("Drain started: Active -> Draining");

            for listener in self.listeners.lock().unwrap().iter() {
                if let Some(ref callback) = listener.on_drain_start {
                    callback();
                }
            }
        }

        success
    }

    /// Tries to acquire a guard for a new request. Returns None if not in Active state.
    /// Increments the in-flight count on success.
    pub fn try_acquire(&self) -> Option<DrainGuard<'_>> {
        let current_state = self.state.load(Ordering::SeqCst);
        if current_state != DrainState::Active as u8 {
            return None;
        }

        self.inflight_count.fetch_add(1, Ordering::SeqCst);
        debug!("DrainGuard acquired");
        Some(DrainGuard { controller: self })
    }

    /// Returns the current number of in-flight requests.
    pub fn inflight_count(&self) -> usize {
        self.inflight_count.load(Ordering::SeqCst)
    }

    /// Checks if all in-flight requests have completed.
    /// If in Draining state and inflight_count is 0, transitions to Drained.
    /// Returns true if now in Drained state.
    pub fn check_drained(&self) -> bool {
        if self.state() != DrainState::Draining {
            return false;
        }

        if self.inflight_count() == 0 {
            let old_state = self.state.swap(DrainState::Drained as u8, Ordering::SeqCst);

            if old_state == DrainState::Draining as u8 {
                info!("Drain completed: Draining -> Drained");

                for listener in self.listeners.lock().unwrap().iter() {
                    if let Some(ref callback) = listener.on_drained {
                        callback();
                    }
                }
            }
            true
        } else {
            false
        }
    }

    /// Forcefully transitions to ForceClosed state, regardless of in-flight count.
    pub fn force_close(&self) {
        let old_state = self
            .state
            .swap(DrainState::ForceClosed as u8, Ordering::SeqCst);

        if old_state != DrainState::ForceClosed as u8 {
            info!(
                "Force close: {:?} -> ForceClosed",
                DrainState::from(old_state)
            );

            for listener in self.listeners.lock().unwrap().iter() {
                if let Some(ref callback) = listener.on_force_close {
                    callback();
                }
            }
        }
    }

    /// Resets the controller to Active state. Used when node rejoins the cluster.
    pub fn reset(&self) {
        let old_state = self.state.load(Ordering::SeqCst);
        self.state.store(DrainState::Active as u8, Ordering::SeqCst);
        self.inflight_count.store(0, Ordering::SeqCst);
        *self.drain_started_at.lock().unwrap() = None;

        info!("Reset from {:?} to Active", DrainState::from(old_state));
    }

    /// Adds a listener for drain events.
    pub fn add_listener(&self, listener: DrainListener) {
        self.listeners.lock().unwrap().push(listener);
    }

    /// Returns the elapsed time since drain started, if applicable.
    pub fn elapsed_since_drain(&self) -> Option<Duration> {
        let guard = self.drain_started_at.lock().unwrap();
        guard.map(|start| start.elapsed())
    }

    /// Returns a snapshot of current drain statistics.
    pub fn stats(&self) -> DrainStats {
        DrainStats {
            state: self.state(),
            inflight_count: self.inflight_count(),
            drain_duration: self.elapsed_since_drain(),
        }
    }
}

impl Default for DrainController {
    fn default() -> Self {
        Self::new(DrainConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn test_drain_config_default() {
        let config = DrainConfig::default();
        assert_eq!(config.drain_timeout, Duration::from_secs(30));
        assert_eq!(config.check_interval, Duration::from_millis(100));
        assert_eq!(config.force_close_after, Duration::from_secs(60));
    }

    #[test]
    fn test_initial_state_active() {
        let controller = DrainController::new(DrainConfig::default());
        assert_eq!(controller.state(), DrainState::Active);
    }

    #[test]
    fn test_is_accepting_when_active() {
        let controller = DrainController::new(DrainConfig::default());
        assert!(controller.is_accepting());
    }

    #[test]
    fn test_is_not_accepting_when_draining() {
        let controller = DrainController::new(DrainConfig::default());
        controller.begin_drain();
        assert!(!controller.is_accepting());
    }

    #[test]
    fn test_begin_drain_transitions() {
        let controller = DrainController::new(DrainConfig::default());
        assert_eq!(controller.state(), DrainState::Active);
        let result = controller.begin_drain();
        assert!(result);
        assert_eq!(controller.state(), DrainState::Draining);
    }

    #[test]
    fn test_begin_drain_idempotent() {
        let controller = DrainController::new(DrainConfig::default());
        let first = controller.begin_drain();
        let second = controller.begin_drain();
        assert!(first);
        assert!(!second);
    }

    #[test]
    fn test_try_acquire_when_active() {
        let controller = DrainController::new(DrainConfig::default());
        let guard = controller.try_acquire();
        assert!(guard.is_some());
        drop(guard);
        assert_eq!(controller.inflight_count(), 0);
    }

    #[test]
    fn test_try_acquire_when_draining() {
        let controller = DrainController::new(DrainConfig::default());
        controller.begin_drain();
        let guard = controller.try_acquire();
        assert!(guard.is_none());
    }

    #[test]
    fn test_drain_guard_decrements() {
        let controller = DrainController::new(DrainConfig::default());
        assert_eq!(controller.inflight_count(), 0);

        {
            let _guard = controller.try_acquire().unwrap();
            assert_eq!(controller.inflight_count(), 1);
        }

        assert_eq!(controller.inflight_count(), 0);
    }

    #[test]
    fn test_inflight_count_tracking() {
        let controller = DrainController::new(DrainConfig::default());

        let guard1 = controller.try_acquire();
        let guard2 = controller.try_acquire();
        let guard3 = controller.try_acquire();

        assert!(guard1.is_some());
        assert!(guard2.is_some());
        assert!(guard3.is_some());

        assert_eq!(controller.inflight_count(), 3);

        drop(guard2);
        assert_eq!(controller.inflight_count(), 2);

        drop(guard1);
        assert_eq!(controller.inflight_count(), 1);

        drop(guard3);
        assert_eq!(controller.inflight_count(), 0);
    }

    #[test]
    fn test_check_drained_with_zero_inflight() {
        let controller = DrainController::new(DrainConfig::default());
        controller.begin_drain();
        assert_eq!(controller.state(), DrainState::Draining);

        let result = controller.check_drained();
        assert!(result);
        assert_eq!(controller.state(), DrainState::Drained);
    }

    #[test]
    fn test_check_drained_with_inflight() {
        let controller = DrainController::new(DrainConfig::default());

        let _guard = controller.try_acquire().unwrap();
        controller.begin_drain();

        let result = controller.check_drained();
        assert!(!result);
        assert_eq!(controller.state(), DrainState::Draining);
    }

    #[test]
    fn test_force_close() {
        let controller = DrainController::new(DrainConfig::default());

        let _guard = controller.try_acquire().unwrap();
        controller.begin_drain();

        controller.force_close();
        assert_eq!(controller.state(), DrainState::ForceClosed);
    }

    #[test]
    fn test_reset_to_active() {
        let controller = DrainController::new(DrainConfig::default());

        controller.begin_drain();
        assert_eq!(controller.state(), DrainState::Draining);

        controller.reset();
        assert_eq!(controller.state(), DrainState::Active);

        let guard = controller.try_acquire();
        assert!(guard.is_some());
    }

    #[test]
    fn test_drain_stats() {
        let controller = DrainController::new(DrainConfig::default());

        let stats = controller.stats();
        assert_eq!(stats.state, DrainState::Active);
        assert_eq!(stats.inflight_count, 0);
        assert!(stats.drain_duration.is_none());

        let _guard = controller.try_acquire().unwrap();
        controller.begin_drain();

        let stats = controller.stats();
        assert_eq!(stats.state, DrainState::Draining);
        assert_eq!(stats.inflight_count, 1);
        assert!(stats.drain_duration.is_some());
    }

    #[test]
    fn test_elapsed_since_drain() {
        let controller = DrainController::new(DrainConfig::default());

        let elapsed = controller.elapsed_since_drain();
        assert!(elapsed.is_none());

        controller.begin_drain();

        std::thread::sleep(Duration::from_millis(10));

        let elapsed = controller.elapsed_since_drain();
        assert!(elapsed.is_some());
        assert!(elapsed.unwrap() >= Duration::from_millis(10));
    }

    #[test]
    fn test_concurrent_acquire_release() {
        let controller = DrainController::new(DrainConfig::default());
        let num_tasks = 20;

        let mut guards = Vec::new();
        for _ in 0..num_tasks {
            if let Some(guard) = controller.try_acquire() {
                guards.push(guard);
            }
        }

        assert_eq!(controller.inflight_count(), num_tasks);

        drop(guards.pop());
        drop(guards.pop());

        assert_eq!(controller.inflight_count(), num_tasks - 2);
    }

    #[test]
    fn test_listener_callbacks() {
        let controller = DrainController::new(DrainConfig::default());

        let drain_start_called = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let drained_called = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let force_close_called = Arc::new(std::sync::atomic::AtomicBool::new(false));

        let drain_start_called_clone = Arc::clone(&drain_start_called);
        let drained_called_clone = Arc::clone(&drained_called);
        let force_close_called_clone = Arc::clone(&force_close_called);

        controller.add_listener(DrainListener {
            on_drain_start: Some(Box::new(move || {
                drain_start_called_clone.store(true, std::sync::atomic::Ordering::SeqCst);
            })),
            on_drained: Some(Box::new(move || {
                drained_called_clone.store(true, std::sync::atomic::Ordering::SeqCst);
            })),
            on_force_close: Some(Box::new(move || {
                force_close_called_clone.store(true, std::sync::atomic::Ordering::SeqCst);
            })),
        });

        controller.begin_drain();
        assert!(drain_start_called.load(std::sync::atomic::Ordering::SeqCst));

        controller.check_drained();
        assert!(drained_called.load(std::sync::atomic::Ordering::SeqCst));

        controller.reset();
        controller.begin_drain();
        controller.force_close();
        assert!(force_close_called.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[test]
    fn test_drain_from_drained_state() {
        let controller = DrainController::new(DrainConfig::default());

        controller.begin_drain();
        controller.check_drained();
        assert_eq!(controller.state(), DrainState::Drained);

        let result = controller.begin_drain();
        assert!(!result);
    }

    #[test]
    fn test_drain_from_force_closed_state() {
        let controller = DrainController::new(DrainConfig::default());

        controller.force_close();
        assert_eq!(controller.state(), DrainState::ForceClosed);

        let result = controller.begin_drain();
        assert!(!result);

        let guard = controller.try_acquire();
        assert!(guard.is_none());
    }

    #[test]
    fn test_stats_after_force_close() {
        let controller = DrainController::new(DrainConfig::default());

        let _guard = controller.try_acquire().unwrap();
        controller.begin_drain();

        controller.force_close();

        let stats = controller.stats();
        assert_eq!(stats.state, DrainState::ForceClosed);
        assert_eq!(stats.inflight_count, 1);
    }
}
