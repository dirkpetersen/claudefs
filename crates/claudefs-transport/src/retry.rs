//! Retry logic with exponential backoff for RPC operations.
//!
//! This module provides retry capabilities for transport operations,
//! integrating with the health module to track connection failures/successes.

use std::future::Future;
use std::time::{Duration, Instant};

use crate::error::{TransportError, Result};
use crate::health::ConnectionHealth;

/// Configuration for retry behavior.
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retry attempts (default: 3).
    pub max_retries: u32,
    /// Initial backoff duration (default: 100ms).
    pub initial_backoff: Duration,
    /// Maximum backoff duration (default: 10 seconds).
    pub max_backoff: Duration,
    /// Multiplier for exponential backoff (default: 2.0).
    pub backoff_multiplier: f64,
    /// Whether to add random jitter to backoff (default: true).
    pub jitter: bool,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_backoff: Duration::from_millis(100),
            max_backoff: Duration::from_secs(10),
            backoff_multiplier: 2.0,
            jitter: true,
        }
    }
}

/// Retry policy for operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryPolicy {
    /// No retry policy - fail immediately.
    None,
    /// Fixed delay between retries.
    Fixed(Duration),
    /// Exponential backoff using RetryConfig settings.
    ExponentialBackoff,
}

/// Outcome of a retry operation.
#[derive(Debug)]
pub enum RetryOutcome<T> {
    /// Operation succeeded.
    Success(T),
    /// All retries exhausted.
    Exhausted {
        /// The last error that occurred.
        last_error: TransportError,
        /// Total number of attempts made.
        attempts: u32,
    },
}

/// Executor for retry operations.
#[derive(Debug, Clone)]
pub struct RetryExecutor {
    config: RetryConfig,
}

impl RetryExecutor {
    /// Create a new RetryExecutor with the given configuration.
    pub fn new(config: RetryConfig) -> Self {
        Self { config }
    }

    /// Execute an operation with retry logic.
    ///
    /// Runs the operation, retrying on failure with exponential backoff.
    /// On success returns `RetryOutcome::Success(T)`.
    /// On final failure returns `RetryOutcome::Exhausted`.
    /// Only transient (retryable) errors are retried; permanent errors fail immediately.
    pub async fn execute<F, Fut, T>(&self, operation: F) -> RetryOutcome<T>
    where
        F: Fn() -> Fut,
        Fut: Future<Output = Result<T>>,
    {
        let mut attempt = 0u32;
        let mut last_error = None;
        let mut should_retry = true;

        while should_retry {
            attempt += 1;

            let result = operation().await;
            match result {
                Ok(value) => return RetryOutcome::Success(value),
                Err(e) => {
                    let retryable = is_retryable(&e);
                    let maxed_out = attempt > self.config.max_retries;

                    if !retryable || maxed_out {
                        last_error = Some(e);
                        should_retry = false;
                    } else {
                        last_error = Some(e);
                        let backoff = self.compute_backoff(attempt - 1);
                        tokio::time::sleep(backoff).await;
                    }
                }
            }
        }

        RetryOutcome::Exhausted {
            last_error: last_error.unwrap(),
            attempts: attempt,
        }
    }

    /// Execute an operation with retry logic and health monitoring.
    ///
    /// Same as `execute` but also records success/failure on the ConnectionHealth monitor.
    /// On success: calls `health.record_success(elapsed_duration)`.
    /// On failure: calls `health.record_failure()`.
    /// Only transient (retryable) errors are retried; permanent errors fail immediately.
    pub async fn execute_with_health<F, Fut, T>(
        &self,
        health: &ConnectionHealth,
        operation: F,
    ) -> RetryOutcome<T>
    where
        F: Fn() -> Fut,
        Fut: Future<Output = Result<T>>,
    {
        let mut attempt = 0u32;
        let mut last_error = None;
        let mut should_retry = true;

        while should_retry {
            attempt += 1;
            let start = Instant::now();

            let result = operation().await;
            match result {
                Ok(value) => {
                    health.record_success(start.elapsed());
                    return RetryOutcome::Success(value);
                }
                Err(e) => {
                    health.record_failure();
                    let retryable = is_retryable(&e);
                    let maxed_out = attempt > self.config.max_retries;

                    if !retryable || maxed_out {
                        last_error = Some(e);
                        should_retry = false;
                    } else {
                        last_error = Some(e);
                        let backoff = self.compute_backoff(attempt - 1);
                        tokio::time::sleep(backoff).await;
                    }
                }
            }
        }

        RetryOutcome::Exhausted {
            last_error: last_error.unwrap(),
            attempts: attempt,
        }
    }

    /// Compute the exponential backoff duration for a given attempt.
    ///
    /// Computes: `initial_backoff * backoff_multiplier^attempt`
    /// Caps the result at `max_backoff`.
    /// If jitter is enabled, adds random jitter (0% to 50% of computed delay).
    fn compute_backoff(&self, attempt: u32) -> Duration {
        let base_delay_ms = self.config.initial_backoff.as_millis() as f64;
        let multiplier = self.config.backoff_multiplier;
        let max_delay_ms = self.config.max_backoff.as_millis() as f64;

        let computed = base_delay_ms * multiplier.powi(attempt as i32);
        let capped = computed.min(max_delay_ms);

        if self.config.jitter {
            let jitter_ms = simple_jitter(capped as u64 / 2);
            let total = (capped as u64).saturating_add(jitter_ms);
            Duration::from_millis(total)
        } else {
            Duration::from_millis(capped as u64)
        }
    }
}

impl Default for RetryExecutor {
    fn default() -> Self {
        Self::new(RetryConfig::default())
    }
}

/// Generate simple jitter using system time entropy.
///
/// Uses modular arithmetic on system time nanos as cheap entropy.
fn simple_jitter(max_ms: u64) -> u64 {
    if max_ms == 0 {
        return 0;
    }
    let nanos = Instant::now().elapsed().subsec_nanos() as u64;
    let ts_nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(0);
    (nanos ^ ts_nanos) % max_ms
}

/// Check if an error is retryable.
///
/// Returns true for transient errors that should be retried:
/// - ConnectionReset, ConnectionTimeout, RequestTimeout, IoError
///
/// Returns false for permanent errors:
/// - InvalidFrame, InvalidMagic, VersionMismatch, ChecksumMismatch,
///   PayloadTooLarge, UnknownOpcode, SerializationError, NotConnected,
///   RdmaNotAvailable, TlsError, ConnectionRefused
pub fn is_retryable(error: &TransportError) -> bool {
    match error {
        TransportError::ConnectionReset => true,
        TransportError::ConnectionTimeout { .. } => true,
        TransportError::RequestTimeout { .. } => true,
        TransportError::IoError(_) => true,
        TransportError::ConnectionRefused { .. } => false,
        TransportError::InvalidFrame { .. } => false,
        TransportError::InvalidMagic { .. } => false,
        TransportError::VersionMismatch { .. } => false,
        TransportError::ChecksumMismatch { .. } => false,
        TransportError::PayloadTooLarge { .. } => false,
        TransportError::UnknownOpcode(_) => false,
        TransportError::NotConnected => false,
        TransportError::RdmaNotAvailable { .. } => false,
        TransportError::TlsError { .. } => false,
        TransportError::SerializationError(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    #[test]
    fn test_retry_config_default() {
        let config = RetryConfig::default();
        assert_eq!(config.max_retries, 3);
        assert_eq!(config.initial_backoff, Duration::from_millis(100));
        assert_eq!(config.max_backoff, Duration::from_secs(10));
        assert_eq!(config.backoff_multiplier, 2.0);
        assert!(config.jitter);
    }

    #[test]
    fn test_is_retryable() {
        assert!(is_retryable(&TransportError::ConnectionReset));
        assert!(is_retryable(&TransportError::ConnectionTimeout {
            addr: "127.0.0.1:8080".to_string(),
            timeout_ms: 1000
        }));
        assert!(is_retryable(&TransportError::RequestTimeout {
            request_id: 1,
            timeout_ms: 5000
        }));
        assert!(is_retryable(&TransportError::IoError(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "test"
        ))));

        assert!(!is_retryable(&TransportError::InvalidFrame {
            reason: "test".to_string()
        }));
        assert!(!is_retryable(&TransportError::InvalidMagic {
            expected: 0x12345678,
            got: 0x87654321
        }));
        assert!(!is_retryable(&TransportError::VersionMismatch {
            expected: 1,
            got: 2
        }));
        assert!(!is_retryable(&TransportError::ChecksumMismatch {
            expected: 100,
            computed: 200
        }));
        assert!(!is_retryable(&TransportError::PayloadTooLarge {
            size: 1000,
            max_size: 500
        }));
        assert!(!is_retryable(&TransportError::UnknownOpcode(0x1234)));
        assert!(!is_retryable(&TransportError::NotConnected));
        assert!(!is_retryable(&TransportError::RdmaNotAvailable {
            reason: "no hardware".to_string()
        }));
        assert!(!is_retryable(&TransportError::TlsError {
            reason: "handshake failed".to_string()
        }));
        assert!(!is_retryable(&TransportError::SerializationError(
            "invalid payload".to_string()
        )));
        assert!(!is_retryable(&TransportError::ConnectionRefused {
            addr: "127.0.0.1:8080".to_string()
        }));
    }

    #[tokio::test]
    async fn test_retry_success_first_attempt() {
        let executor = RetryExecutor::new(RetryConfig::default());
        let counter = Arc::new(AtomicU32::new(0));

        let counter_clone = Arc::clone(&counter);
        let outcome = executor
            .execute(move || {
                let counter = Arc::clone(&counter_clone);
                async move {
                    counter.fetch_add(1, Ordering::Relaxed);
                    Ok::<_, TransportError>("success")
                }
            })
            .await;

        assert!(matches!(outcome, RetryOutcome::Success("success")));
        assert_eq!(counter.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn test_retry_success_after_failures() {
        let config = RetryConfig {
            max_retries: 3,
            ..Default::default()
        };
        let executor = RetryExecutor::new(config);
        let counter = Arc::new(AtomicU32::new(0));

        let counter_clone = Arc::clone(&counter);
        let outcome = executor
            .execute(move || {
                let counter = Arc::clone(&counter_clone);
                async move {
                    let count = counter.fetch_add(1, Ordering::Relaxed) + 1;
                    if count < 3 {
                        Err(TransportError::ConnectionReset)
                    } else {
                        Ok("success")
                    }
                }
            })
            .await;

        assert!(matches!(outcome, RetryOutcome::Success("success")));
        assert_eq!(counter.load(Ordering::Relaxed), 3);
    }

    #[tokio::test]
    async fn test_retry_exhausted() {
        let config = RetryConfig {
            max_retries: 3,
            initial_backoff: Duration::from_millis(1),
            ..Default::default()
        };
        let executor = RetryExecutor::new(config);
        let counter = Arc::new(AtomicU32::new(0));

        let counter_clone = Arc::clone(&counter);
        let outcome: RetryOutcome<&str> = executor
            .execute(move || {
                let counter = Arc::clone(&counter_clone);
                async move {
                    counter.fetch_add(1, Ordering::Relaxed);
                    Err::<&str, _>(TransportError::ConnectionReset)
                }
            })
            .await;

        assert!(matches!(
            outcome,
            RetryOutcome::Exhausted {
                last_error: TransportError::ConnectionReset,
                attempts: 4
            }
        ));
        assert_eq!(counter.load(Ordering::Relaxed), 4);
    }

    #[test]
    fn test_compute_backoff() {
        let config = RetryConfig {
            max_retries: 3,
            initial_backoff: Duration::from_millis(100),
            max_backoff: Duration::from_secs(10),
            backoff_multiplier: 2.0,
            jitter: false,
        };
        let executor = RetryExecutor::new(config);

        let backoff0 = executor.compute_backoff(0);
        let backoff1 = executor.compute_backoff(1);
        let backoff2 = executor.compute_backoff(2);

        assert_eq!(backoff0, Duration::from_millis(100));
        assert_eq!(backoff1, Duration::from_millis(200));
        assert_eq!(backoff2, Duration::from_millis(400));

        let config_large = RetryConfig {
            max_retries: 10,
            initial_backoff: Duration::from_millis(100),
            max_backoff: Duration::from_millis(500),
            backoff_multiplier: 2.0,
            jitter: false,
        };
        let executor_large = RetryExecutor::new(config_large);

        let backoff_capped = executor_large.compute_backoff(10);
        assert_eq!(backoff_capped, Duration::from_millis(500));
    }

    #[tokio::test]
    async fn test_retry_with_health() {
        let config = RetryConfig {
            max_retries: 2,
            initial_backoff: Duration::from_millis(1),
            ..Default::default()
        };
        let executor = RetryExecutor::new(config);
        let health = ConnectionHealth::new();

        let outcome = executor
            .execute_with_health(&health, || async {
                Err::<String, _>(TransportError::ConnectionReset)
            })
            .await;

        assert!(matches!(
            outcome,
            RetryOutcome::Exhausted {
                last_error: TransportError::ConnectionReset,
                attempts: 3
            }
        ));
        assert_eq!(health.failure_count(), 3);

        let health2 = ConnectionHealth::new();
        let outcome2 = executor
            .execute_with_health(&health2, || async { Ok::<_, TransportError>("ok") })
            .await;

        assert!(matches!(outcome2, RetryOutcome::Success("ok")));
        assert_eq!(health2.success_count(), 1);
    }

    #[tokio::test]
    async fn test_retry_no_retry_on_permanent_error() {
        let config = RetryConfig {
            max_retries: 3,
            ..Default::default()
        };
        let executor = RetryExecutor::new(config);
        let counter = Arc::new(AtomicU32::new(0));

        let counter_clone = Arc::clone(&counter);
        let outcome: RetryOutcome<&str> = executor
            .execute(move || {
                let counter = Arc::clone(&counter_clone);
                async move {
                    counter.fetch_add(1, Ordering::Relaxed);
                    Err::<&str, _>(TransportError::InvalidFrame {
                        reason: "bad frame".to_string(),
                    })
                }
            })
            .await;

        assert!(matches!(
            outcome,
            RetryOutcome::Exhausted {
                last_error: TransportError::InvalidFrame { .. },
                attempts: 1
            }
        ));
        assert_eq!(counter.load(Ordering::Relaxed), 1);
    }
}