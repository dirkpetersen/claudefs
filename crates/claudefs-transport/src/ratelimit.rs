//! Rate limiting module for transport-level request throttling.
//!
//! Supports both per-connection and global rate limiting with configurable burst allowances.

use std::sync::atomic::{AtomicU64, Ordering};

/// Configuration for rate limiting.
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Maximum requests allowed per second.
    pub requests_per_second: u64,
    /// Maximum tokens that can accumulate (burst allowance).
    pub burst_size: u64,
}

impl RateLimitConfig {
    /// Creates a new RateLimitConfig with the specified values.
    pub fn new(requests_per_second: u64, burst_size: u64) -> Self {
        Self {
            requests_per_second,
            burst_size,
        }
    }
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            requests_per_second: 10000,
            burst_size: 1000,
        }
    }
}

/// A lock-free rate limiter using token bucket algorithm.
pub struct RateLimiter {
    config: RateLimitConfig,
    tokens: AtomicU64,
    last_refill: AtomicU64,
}

impl RateLimiter {
    /// Creates a new RateLimiter with the given configuration.
    pub fn new(config: RateLimitConfig) -> Self {
        let max_tokens = config.burst_size;
        Self {
            config,
            tokens: AtomicU64::new(max_tokens),
            last_refill: AtomicU64::new(Self::now_micros()),
        }
    }

    /// Tries to acquire 1 token. Returns true if permitted, false if rate limited.
    pub fn try_acquire(&self) -> bool {
        self.try_acquire_n(1)
    }

    /// Tries to acquire N tokens. Returns true if permitted, false if rate limited.
    pub fn try_acquire_n(&self, n: u64) -> bool {
        self.refill();

        let current = self.tokens.load(Ordering::Relaxed);
        if current >= n {
            let new_val = current.saturating_sub(n);
            self.tokens.store(new_val, Ordering::Relaxed);
            true
        } else {
            false
        }
    }

    /// Returns the current available token count.
    pub fn available_tokens(&self) -> u64 {
        self.refill();
        self.tokens.load(Ordering::Relaxed)
    }

    /// Resets the rate limiter to full burst capacity.
    pub fn reset(&self) {
        let max_tokens = self.config.burst_size;
        self.tokens.store(max_tokens, Ordering::Relaxed);
        self.last_refill.store(Self::now_micros(), Ordering::Relaxed);
    }

    fn now_micros() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_micros() as u64)
            .unwrap_or(0)
    }

    fn refill(&self) {
        let now_us = Self::now_micros();
        let last = self.last_refill.load(Ordering::Relaxed);
        let elapsed_us = now_us.saturating_sub(last);
        if elapsed_us == 0 {
            return;
        }

        if self
            .last_refill
            .compare_exchange(last, now_us, Ordering::Relaxed, Ordering::Relaxed)
            .is_err()
        {
            return;
        }

        let tokens_to_add = (self.config.requests_per_second as u128 * elapsed_us as u128
            / 1_000_000) as u64;
        if tokens_to_add == 0 {
            return;
        }

        let max_tokens = self.config.burst_size;
        let _ = self.tokens.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            Some(max_tokens.min(current.saturating_add(tokens_to_add)))
        });
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new(RateLimitConfig::default())
    }
}

/// Result of a rate limit check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RateLimitResult {
    /// Request is permitted.
    Allowed,
    /// Request denied, retry after this many milliseconds.
    Limited {
        /// Suggested retry time in milliseconds.
        retry_after_ms: u64,
    },
}

/// A composite rate limiter that checks both per-connection and global limits.
pub struct CompositeRateLimiter {
    per_connection: RateLimiter,
    global: RateLimiter,
}

impl CompositeRateLimiter {
    /// Creates a new CompositeRateLimiter with separate configs for per-connection and global limits.
    pub fn new(per_conn: RateLimitConfig, global: RateLimitConfig) -> Self {
        Self {
            per_connection: RateLimiter::new(per_conn),
            global: RateLimiter::new(global),
        }
    }

    /// Checks both per-connection and global rate limiters.
    /// Returns Limited if either limiter is exhausted.
    pub fn check(&self) -> RateLimitResult {
        if !self.per_connection.try_acquire() {
            return RateLimitResult::Limited { retry_after_ms: 1 };
        }

        if !self.global.try_acquire() {
            return RateLimitResult::Limited { retry_after_ms: 1 };
        }

        RateLimitResult::Allowed
    }

    /// Returns a reference to the per-connection rate limiter.
    pub fn per_connection(&self) -> &RateLimiter {
        &self.per_connection
    }

    /// Returns a reference to the global rate limiter.
    pub fn global(&self) -> &RateLimiter {
        &self.global
    }
}

impl Default for CompositeRateLimiter {
    fn default() -> Self {
        Self::new(RateLimitConfig::default(), RateLimitConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[test]
    fn test_rate_limit_config_default() {
        let config = RateLimitConfig::default();
        assert_eq!(config.requests_per_second, 10000);
        assert_eq!(config.burst_size, 1000);
    }

    #[test]
    fn test_try_acquire_within_limit() {
        let limiter = RateLimiter::new(RateLimitConfig::new(1000, 100));
        for _ in 0..100 {
            assert!(limiter.try_acquire());
        }
    }

    #[test]
    fn test_try_acquire_exceeds_limit() {
        let limiter = RateLimiter::new(RateLimitConfig::new(1000, 100));
        for _ in 0..100 {
            assert!(limiter.try_acquire());
        }
        assert!(!limiter.try_acquire());
    }

    #[test]
    fn test_try_acquire_n() {
        let limiter = RateLimiter::new(RateLimitConfig::new(1000, 50));
        assert!(limiter.try_acquire_n(30));
        assert!(limiter.try_acquire_n(20));
        assert!(!limiter.try_acquire_n(1));
    }

    #[test]
    fn test_available_tokens() {
        let limiter = RateLimiter::new(RateLimitConfig::new(1000, 100));
        assert_eq!(limiter.available_tokens(), 100);
        limiter.try_acquire_n(30);
        assert_eq!(limiter.available_tokens(), 70);
    }

    #[test]
    fn test_reset() {
        let limiter = RateLimiter::new(RateLimitConfig::new(1000, 100));
        for _ in 0..100 {
            assert!(limiter.try_acquire());
        }
        assert!(!limiter.try_acquire());
        limiter.reset();
        assert_eq!(limiter.available_tokens(), 100);
        assert!(limiter.try_acquire());
    }

    #[test]
    fn test_rate_limit_result() {
        assert_eq!(
            RateLimitResult::Allowed,
            RateLimitResult::Allowed
        );
        assert_ne!(
            RateLimitResult::Allowed,
            RateLimitResult::Limited { retry_after_ms: 1 }
        );
        assert_eq!(
            RateLimitResult::Limited { retry_after_ms: 5 },
            RateLimitResult::Limited { retry_after_ms: 5 }
        );
    }

    #[test]
    fn test_composite_rate_limiter() {
        let composite = CompositeRateLimiter::new(
            RateLimitConfig::new(10000, 1000),
            RateLimitConfig::new(1000, 100),
        );

        for _ in 0..100 {
            assert!(matches!(composite.check(), RateLimitResult::Allowed));
        }

        assert!(matches!(
            composite.check(),
            RateLimitResult::Limited { .. }
        ));
    }

    #[tokio::test]
    async fn test_concurrent_acquire() {
        let limiter = Arc::new(RateLimiter::new(RateLimitConfig::new(1000, 100)));
        let acquired = Arc::new(AtomicU64::new(0));
        let mut handles = Vec::new();

        for _ in 0..10 {
            let limiter = Arc::clone(&limiter);
            let acquired = Arc::clone(&acquired);
            let handle = tokio::spawn(async move {
                for _ in 0..20 {
                    if limiter.try_acquire() {
                        acquired.fetch_add(1, Ordering::Relaxed);
                    }
                }
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.await.unwrap();
        }

        let total = acquired.load(Ordering::Relaxed);
        assert!(total <= 100, "Total acquired {} should not exceed burst size 100", total);
    }
}