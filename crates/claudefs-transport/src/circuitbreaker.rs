//! Circuit breaker pattern implementation for fault tolerance.
//!
//! This module provides a circuit breaker implementation that prevents cascading failures
//! by failing fast when a service is experiencing issues.
//!
//! # How It Works
//!
//! The circuit breaker has three states:
//!
//! 1. **Closed**: Normal operation. Requests pass through. Failures are counted.
//! 2. **Open**: Circuit is open. Requests are blocked. After `open_duration`, transitions to half-open.
//! 3. **HalfOpen**: Testing recovery. Limited requests are allowed to test if the service recovered.
//!
//! # Example
//!
//! ```
//! use claudefs_transport::circuitbreaker::{CircuitBreaker, CircuitBreakerConfig, CircuitState};
//!
//! let config = CircuitBreakerConfig::default();
//! let breaker = CircuitBreaker::new(config);
//!
//! // Check if request can proceed
//! if breaker.can_execute() {
//!     // Execute request
//!     // ...
//!     breaker.record_success();
//! } else {
//!     // Circuit is open, fail fast
//!     println!("Circuit is open, skipping request");
//! }
//! ```
//!
//! # Tuning
//!
//! - Decrease `failure_threshold` to open circuit faster
//! - Increase `open_duration` to wait longer before testing recovery
//! - Increase `success_threshold` to require more successes before closing
//!
//! # See Also
//! - [`CircuitState`] - State enumeration
//! - [`CircuitBreakerConfig`] - Configuration options

use std::time::Duration;

const STATE_CLOSED: u8 = 0;
const STATE_OPEN: u8 = 1;
const STATE_HALF_OPEN: u8 = 2;

/// Default failure threshold: number of consecutive failures required to open the circuit.
pub const DEFAULT_FAILURE_THRESHOLD: u32 = 5;
/// Default success threshold: number of consecutive successes required to close the circuit from half-open.
pub const DEFAULT_SUCCESS_THRESHOLD: u32 = 3;
/// Default open duration in milliseconds: time before transitioning from open to half-open.
pub const DEFAULT_OPEN_DURATION_MS: u64 = 30_000;
/// Default maximum requests allowed in half-open state.
pub const DEFAULT_HALF_OPEN_MAX_REQUESTS: u32 = 1;

/// Represents the state of the circuit breaker.
///
/// # State Transitions
///
/// ```
/// use claudefs_transport::circuitbreaker::{CircuitBreaker, CircuitBreakerConfig, CircuitState};
/// use std::thread;
/// use std::time::Duration;
///
/// let mut config = CircuitBreakerConfig::default();
/// config.open_duration = Duration::from_millis(50);
/// let breaker = CircuitBreaker::new(config);
///
/// // Closed -> Open: After failure_threshold failures
/// for _ in 0..5 { breaker.record_failure(); }
/// assert_eq!(breaker.state(), CircuitState::Open);
///
/// // Open -> HalfOpen: After open_duration
/// thread::sleep(Duration::from_millis(60));
/// assert!(breaker.can_execute());
/// assert_eq!(breaker.state(), CircuitState::HalfOpen);
///
/// // HalfOpen -> Closed: After success_threshold successes
/// for _ in 0..3 { breaker.record_success(); }
/// assert_eq!(breaker.state(), CircuitState::Closed);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    /// Normal operation; requests are allowed through.
    Closed,
    /// Circuit is open; requests are blocked.
    Open,
    /// Testing recovery; limited requests are allowed to test if the service recovered.
    HalfOpen,
}

impl CircuitState {
    fn from_u8(value: u8) -> Self {
        match value {
            STATE_OPEN => CircuitState::Open,
            STATE_HALF_OPEN => CircuitState::HalfOpen,
            _ => CircuitState::Closed,
        }
    }
}

/// Configuration for the circuit breaker.
///
/// # Default Values
///
/// - `failure_threshold`: 5 consecutive failures to open
/// - `success_threshold`: 3 consecutive successes to close
/// - `open_duration`: 30 seconds
/// - `half_open_max_requests`: 1 request at a time
///
/// # Examples
///
/// ```
/// use claudefs_transport::circuitbreaker::CircuitBreakerConfig;
/// use std::time::Duration;
///
/// // Aggressive configuration
/// let config = CircuitBreakerConfig {
///     failure_threshold: 3,           // Open after 3 failures
///     success_threshold: 2,           // Close after 2 successes
///     open_duration: Duration::from_secs(10),  // Wait 10s before testing
///     half_open_max_requests: 3,     // Allow 3 test requests
/// };
/// ```
///
/// # Tuning Guidelines
///
/// - **For unstable services**: Lower failure_threshold to open sooner
/// - **For stable services**: Raise failure_threshold to avoid flapping
/// - **For recovery**: Increase open_duration to allow service time to recover
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    /// Number of consecutive failures required to open the circuit.
    pub failure_threshold: u32,
    /// Number of consecutive successes required to close the circuit from half-open.
    pub success_threshold: u32,
    /// Duration the circuit remains open before transitioning to half-open.
    pub open_duration: Duration,
    /// Maximum number of requests allowed in half-open state.
    pub half_open_max_requests: u32,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: DEFAULT_FAILURE_THRESHOLD,
            success_threshold: DEFAULT_SUCCESS_THRESHOLD,
            open_duration: Duration::from_millis(DEFAULT_OPEN_DURATION_MS),
            half_open_max_requests: DEFAULT_HALF_OPEN_MAX_REQUESTS,
        }
    }
}

/// A thread-safe circuit breaker implementation using atomic operations.
///
/// # States
///
/// - **Closed**: Normal operation. Requests pass through. Failures are counted.
/// - **Open**: Circuit is open. Requests are blocked. After `open_duration`, transitions to half-open.
/// - **HalfOpen**: Testing recovery. Limited requests (`half_open_max_requests`) are allowed.
///   - On success: success counter increments. After `success_threshold` successes, closes.
///   - On failure: immediately opens again.
///
/// # Example
///
/// ```
/// use claudefs_transport::{CircuitBreaker, CircuitBreakerConfig};
/// use std::time::Duration;
///
/// let config = CircuitBreakerConfig::default();
/// let breaker = CircuitBreaker::new(config);
///
/// // Check if request can proceed
/// if breaker.can_execute() {
///     // Execute request
///     // ...
///     breaker.record_success();
/// } else {
///     // Circuit is open, fail fast
/// }
/// ```
pub struct CircuitBreaker {
    state: std::sync::atomic::AtomicU8,
    failure_count: std::sync::atomic::AtomicU32,
    success_count: std::sync::atomic::AtomicU32,
    opened_at: std::sync::atomic::AtomicU64,
    half_open_requests: std::sync::atomic::AtomicU32,
    config: CircuitBreakerConfig,
    open_time: std::sync::atomic::AtomicU64,
}

impl CircuitBreaker {
    /// Creates a new circuit breaker with the given configuration.
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            state: std::sync::atomic::AtomicU8::new(STATE_CLOSED),
            failure_count: std::sync::atomic::AtomicU32::new(0),
            success_count: std::sync::atomic::AtomicU32::new(0),
            opened_at: std::sync::atomic::AtomicU64::new(0),
            half_open_requests: std::sync::atomic::AtomicU32::new(0),
            config,
            open_time: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// Checks if a request can proceed based on the current circuit state.
    ///
    /// Returns `true` if the request is allowed, `false` if the circuit is open.
    pub fn can_execute(&self) -> bool {
        let current_state = self.state.load(std::sync::atomic::Ordering::Acquire);

        match current_state {
            STATE_CLOSED => true,
            STATE_OPEN => {
                let open_time = self.open_time.load(std::sync::atomic::Ordering::Acquire);
                if open_time == 0 {
                    return true;
                }
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as u64;
                let elapsed = now.saturating_sub(open_time);

                if elapsed >= self.config.open_duration.as_millis() as u64 {
                    let half_open_count = self
                        .half_open_requests
                        .load(std::sync::atomic::Ordering::Acquire);
                    if half_open_count < self.config.half_open_max_requests {
                        self.half_open_requests
                            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
                        self.state
                            .store(STATE_HALF_OPEN, std::sync::atomic::Ordering::Release);
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            STATE_HALF_OPEN => {
                let half_open_count = self
                    .half_open_requests
                    .load(std::sync::atomic::Ordering::Acquire);
                half_open_count < self.config.half_open_max_requests
            }
            _ => false,
        }
    }

    /// Records a successful request.
    ///
    /// - In closed state: resets the failure count.
    /// - In half-open state: increments the success count. After `success_threshold`
    ///   consecutive successes, transitions to closed state.
    pub fn record_success(&self) {
        let current_state = self.state.load(std::sync::atomic::Ordering::Acquire);

        if current_state == STATE_HALF_OPEN {
            let prev = self
                .success_count
                .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
            if prev + 1 >= self.config.success_threshold {
                self.state
                    .store(STATE_CLOSED, std::sync::atomic::Ordering::Release);
                self.failure_count
                    .store(0, std::sync::atomic::Ordering::Release);
                self.success_count
                    .store(0, std::sync::atomic::Ordering::Release);
                self.half_open_requests
                    .store(0, std::sync::atomic::Ordering::Release);
                self.opened_at
                    .store(0, std::sync::atomic::Ordering::Release);
                self.open_time
                    .store(0, std::sync::atomic::Ordering::Release);
            }
        } else if current_state == STATE_CLOSED {
            self.failure_count
                .store(0, std::sync::atomic::Ordering::Release);
        }
    }

    /// Records a failed request.
    ///
    /// - In closed state: increments the failure count. After `failure_threshold`
    ///   consecutive failures, transitions to open state.
    /// - In half-open state: immediately transitions to open state.
    pub fn record_failure(&self) {
        let current_state = self.state.load(std::sync::atomic::Ordering::Acquire);

        if current_state == STATE_CLOSED {
            let prev = self
                .failure_count
                .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
            if prev + 1 >= self.config.failure_threshold {
                self.state
                    .store(STATE_OPEN, std::sync::atomic::Ordering::Release);
                self.opened_at.store(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_millis() as u64,
                    std::sync::atomic::Ordering::Release,
                );
                self.open_time.store(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_millis() as u64,
                    std::sync::atomic::Ordering::Release,
                );
            }
        } else if current_state == STATE_HALF_OPEN {
            self.state
                .store(STATE_OPEN, std::sync::atomic::Ordering::Release);
            self.half_open_requests
                .store(0, std::sync::atomic::Ordering::Release);
        }
    }

    /// Returns the current state of the circuit breaker.
    pub fn state(&self) -> CircuitState {
        CircuitState::from_u8(self.state.load(std::sync::atomic::Ordering::Acquire))
    }

    /// Returns the current failure count.
    pub fn failure_count(&self) -> u32 {
        self.failure_count
            .load(std::sync::atomic::Ordering::Acquire)
    }

    /// Returns the current success count.
    pub fn success_count(&self) -> u32 {
        self.success_count
            .load(std::sync::atomic::Ordering::Acquire)
    }

    /// Manually resets the circuit breaker to closed state.
    ///
    /// This clears all counters and allows requests through again.
    pub fn reset(&self) {
        self.state
            .store(STATE_CLOSED, std::sync::atomic::Ordering::Release);
        self.failure_count
            .store(0, std::sync::atomic::Ordering::Release);
        self.success_count
            .store(0, std::sync::atomic::Ordering::Release);
        self.opened_at
            .store(0, std::sync::atomic::Ordering::Release);
        self.half_open_requests
            .store(0, std::sync::atomic::Ordering::Release);
        self.open_time
            .store(0, std::sync::atomic::Ordering::Release);
    }
}

impl Default for CircuitBreaker {
    fn default() -> Self {
        Self::new(CircuitBreakerConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn config_default() {
        let config = CircuitBreakerConfig::default();
        assert_eq!(config.failure_threshold, 5);
        assert_eq!(config.success_threshold, 3);
        assert_eq!(config.open_duration.as_secs(), 30);
        assert_eq!(config.half_open_max_requests, 1);
    }

    #[test]
    fn initial_closed() {
        let breaker = CircuitBreaker::default();
        assert_eq!(breaker.state(), CircuitState::Closed);
        assert!(breaker.can_execute());
    }

    #[test]
    fn trip_open() {
        let breaker = CircuitBreaker::default();
        for _ in 0..5 {
            breaker.record_failure();
        }
        assert_eq!(breaker.state(), CircuitState::Open);
        assert!(!breaker.can_execute());
    }

    #[test]
    fn open_to_halfopen() {
        let mut config = CircuitBreakerConfig::default();
        config.open_duration = Duration::from_millis(50);
        let breaker = CircuitBreaker::new(config);

        for _ in 0..5 {
            breaker.record_failure();
        }
        assert_eq!(breaker.state(), CircuitState::Open);

        thread::sleep(Duration::from_millis(60));

        assert!(breaker.can_execute());
        assert_eq!(breaker.state(), CircuitState::HalfOpen);
    }

    #[test]
    fn halfopen_to_closed() {
        let mut config = CircuitBreakerConfig::default();
        config.open_duration = Duration::from_millis(50);
        let breaker = CircuitBreaker::new(config);

        for _ in 0..5 {
            breaker.record_failure();
        }

        thread::sleep(Duration::from_millis(60));
        breaker.can_execute();

        for _ in 0..3 {
            breaker.record_success();
        }

        assert_eq!(breaker.state(), CircuitState::Closed);
    }

    #[test]
    fn halfopen_to_open() {
        let mut config = CircuitBreakerConfig::default();
        config.open_duration = Duration::from_millis(50);
        let breaker = CircuitBreaker::new(config);

        for _ in 0..5 {
            breaker.record_failure();
        }

        thread::sleep(Duration::from_millis(60));
        breaker.can_execute();

        breaker.record_failure();

        assert_eq!(breaker.state(), CircuitState::Open);
    }

    #[test]
    fn halfopen_max_requests() {
        let mut config = CircuitBreakerConfig::default();
        config.open_duration = Duration::from_millis(50);
        config.half_open_max_requests = 1;
        let breaker = CircuitBreaker::new(config);

        for _ in 0..5 {
            breaker.record_failure();
        }

        thread::sleep(Duration::from_millis(60));

        assert!(breaker.can_execute());
        assert!(!breaker.can_execute());
    }

    #[test]
    fn success_resets_failures() {
        let breaker = CircuitBreaker::default();

        breaker.record_failure();
        breaker.record_failure();
        assert_eq!(breaker.failure_count(), 2);

        breaker.record_success();
        assert_eq!(breaker.failure_count(), 0);
    }

    #[test]
    fn reset() {
        let breaker = CircuitBreaker::default();

        for _ in 0..5 {
            breaker.record_failure();
        }
        assert_eq!(breaker.state(), CircuitState::Open);

        breaker.reset();
        assert_eq!(breaker.state(), CircuitState::Closed);
        assert!(breaker.can_execute());
    }
}
