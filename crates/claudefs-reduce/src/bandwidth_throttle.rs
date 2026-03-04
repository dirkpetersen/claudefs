//! Bandwidth throttling for background data reduction operations.
//!
//! Background operations (compaction, tier migration, GC) must not saturate disk I/O
//! and impact foreground user I/O. This implements a token bucket throttler.

use serde::{Deserialize, Serialize};

/// Configuration for bandwidth throttling.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThrottleConfig {
    /// Maximum bytes per second allowed.
    pub rate_bytes_per_sec: u64,
    /// Maximum burst size in bytes.
    pub burst_bytes: u64,
}

impl Default for ThrottleConfig {
    fn default() -> Self {
        Self {
            rate_bytes_per_sec: 100 * 1024 * 1024, // 100 MB/s
            burst_bytes: 4 * 1024 * 1024,          // 4 MB
        }
    }
}

/// Decision returned by the bandwidth throttle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThrottleDecision {
    /// Request is allowed to proceed.
    Allowed,
    /// Request is throttled; retry after the specified milliseconds.
    Throttled {
        /// Milliseconds to wait before retry.
        retry_after_ms: u64,
    },
}

/// Token bucket for rate limiting.
pub struct TokenBucket {
    rate_bytes_per_sec: u64,
    burst_bytes: u64,
    tokens: u64,
    last_refill_ms: u64,
}

impl TokenBucket {
    /// Creates a new token bucket with the given rate and burst size.
    pub fn new(rate_bytes_per_sec: u64, burst_bytes: u64) -> Self {
        Self {
            rate_bytes_per_sec,
            burst_bytes,
            tokens: burst_bytes, // Start with full bucket
            last_refill_ms: 0,
        }
    }

    /// Tries to consume the specified number of bytes.
    ///
    /// Returns true if the tokens were consumed, false if insufficient tokens.
    pub fn try_consume(&mut self, bytes: u64, now_ms: u64) -> bool {
        self.refill(now_ms);

        if self.tokens >= bytes {
            self.tokens -= bytes;
            true
        } else {
            false
        }
    }

    /// Returns the current number of available tokens.
    pub fn available_tokens(&self) -> u64 {
        self.tokens
    }

    /// Refills tokens based on elapsed time.
    pub fn refill(&mut self, now_ms: u64) {
        if now_ms <= self.last_refill_ms {
            return;
        }

        let elapsed_ms = now_ms - self.last_refill_ms;
        let elapsed_secs = elapsed_ms as f64 / 1000.0;
        let new_tokens = (self.rate_bytes_per_sec as f64 * elapsed_secs) as u64;

        self.tokens = (self.tokens + new_tokens).min(self.burst_bytes);
        self.last_refill_ms = now_ms;
    }
}

/// Statistics for the bandwidth throttle.
#[derive(Debug, Clone, Default)]
pub struct ThrottleStats {
    /// Number of requests allowed.
    pub requests_allowed: u64,
    /// Number of requests throttled.
    pub requests_throttled: u64,
    /// Total bytes allowed through.
    pub bytes_allowed: u64,
}

/// Bandwidth throttle using token bucket algorithm.
pub struct BandwidthThrottle {
    config: ThrottleConfig,
    bucket: TokenBucket,
    stats: ThrottleStats,
}

impl BandwidthThrottle {
    /// Creates a new bandwidth throttle.
    pub fn new(config: ThrottleConfig) -> Self {
        let bucket = TokenBucket::new(config.rate_bytes_per_sec, config.burst_bytes);
        Self {
            config,
            bucket,
            stats: ThrottleStats::default(),
        }
    }

    /// Requests bandwidth for the given number of bytes.
    pub fn request(&mut self, bytes: u64, now_ms: u64) -> ThrottleDecision {
        if self.bucket.try_consume(bytes, now_ms) {
            self.stats.requests_allowed += 1;
            self.stats.bytes_allowed += bytes;
            ThrottleDecision::Allowed
        } else {
            self.stats.requests_throttled += 1;
            let rate_bytes_per_ms = self.config.rate_bytes_per_sec as f64 / 1000.0;
            let needed = bytes.saturating_sub(self.bucket.available_tokens());
            let retry_after_ms = (needed as f64 / rate_bytes_per_ms).ceil() as u64;
            ThrottleDecision::Throttled { retry_after_ms }
        }
    }

    /// Returns statistics about the throttle.
    pub fn stats(&self) -> &ThrottleStats {
        &self.stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn throttle_config_default() {
        let config = ThrottleConfig::default();
        assert_eq!(config.rate_bytes_per_sec, 100 * 1024 * 1024);
        assert_eq!(config.burst_bytes, 4 * 1024 * 1024);
    }

    #[test]
    fn token_bucket_initial_tokens_equals_burst() {
        let bucket = TokenBucket::new(1000, 5000);
        assert_eq!(bucket.available_tokens(), 5000);
    }

    #[test]
    fn token_bucket_consume_within_burst() {
        let mut bucket = TokenBucket::new(1000, 5000);
        let result = bucket.try_consume(3000, 0);
        assert!(result);
        assert_eq!(bucket.available_tokens(), 2000);
    }

    #[test]
    fn token_bucket_consume_exceeds_available() {
        let mut bucket = TokenBucket::new(1000, 5000);
        bucket.try_consume(3000, 0);
        let result = bucket.try_consume(3000, 0);
        assert!(!result);
        assert_eq!(bucket.available_tokens(), 2000);
    }

    #[test]
    fn token_bucket_refill_over_time() {
        let mut bucket = TokenBucket::new(1000, 5000);
        bucket.try_consume(5000, 0);
        assert_eq!(bucket.available_tokens(), 0);

        bucket.refill(1000);
        let expected = (1000.0 / 1000.0 * 1000.0) as u64;
        assert_eq!(bucket.available_tokens(), expected);
    }

    #[test]
    fn token_bucket_capped_at_burst() {
        let mut bucket = TokenBucket::new(1000, 5000);
        bucket.refill(10000);
        assert_eq!(bucket.available_tokens(), 5000);
    }

    #[test]
    fn bandwidth_throttle_allows_small_request() {
        let config = ThrottleConfig {
            rate_bytes_per_sec: 1000,
            burst_bytes: 5000,
        };
        let mut throttle = BandwidthThrottle::new(config);

        let decision = throttle.request(1000, 0);
        assert_eq!(decision, ThrottleDecision::Allowed);
    }

    #[test]
    fn bandwidth_throttle_throttles_large_request() {
        let config = ThrottleConfig {
            rate_bytes_per_sec: 1000,
            burst_bytes: 5000,
        };
        let mut throttle = BandwidthThrottle::new(config);

        throttle.request(4500, 0);
        let decision = throttle.request(1000, 0);
        assert!(matches!(decision, ThrottleDecision::Throttled { .. }));
    }

    #[test]
    fn bandwidth_throttle_allows_after_refill() {
        let config = ThrottleConfig {
            rate_bytes_per_sec: 1000,
            burst_bytes: 5000,
        };
        let mut throttle = BandwidthThrottle::new(config);

        throttle.request(5000, 0);
        let decision = throttle.request(1000, 1000);
        assert_eq!(decision, ThrottleDecision::Allowed);
    }

    #[test]
    fn throttle_decision_allowed_variant() {
        let decision = ThrottleDecision::Allowed;
        assert_eq!(decision, ThrottleDecision::Allowed);
    }

    #[test]
    fn throttle_decision_throttled_has_retry_time() {
        let decision = ThrottleDecision::Throttled {
            retry_after_ms: 500,
        };
        if let ThrottleDecision::Throttled { retry_after_ms } = decision {
            assert_eq!(retry_after_ms, 500);
        } else {
            panic!("Expected Throttled variant");
        }
    }

    #[test]
    fn throttle_stats_counts_allowed() {
        let config = ThrottleConfig {
            rate_bytes_per_sec: 1000,
            burst_bytes: 5000,
        };
        let mut throttle = BandwidthThrottle::new(config);

        throttle.request(1000, 0);
        throttle.request(1000, 0);

        assert_eq!(throttle.stats().requests_allowed, 2);
    }

    #[test]
    fn throttle_stats_counts_throttled() {
        let config = ThrottleConfig {
            rate_bytes_per_sec: 1000,
            burst_bytes: 5000,
        };
        let mut throttle = BandwidthThrottle::new(config);

        throttle.request(4500, 0);
        throttle.request(1000, 0);

        assert_eq!(throttle.stats().requests_throttled, 1);
    }

    #[test]
    fn throttle_stats_bytes_allowed() {
        let config = ThrottleConfig {
            rate_bytes_per_sec: 1000,
            burst_bytes: 5000,
        };
        let mut throttle = BandwidthThrottle::new(config);

        throttle.request(1000, 0);
        throttle.request(2000, 0);

        assert_eq!(throttle.stats().bytes_allowed, 3000);
    }

    #[test]
    fn zero_elapsed_time_no_refill() {
        let mut bucket = TokenBucket::new(1000, 5000);
        bucket.try_consume(5000, 0);

        bucket.refill(0);
        assert_eq!(bucket.available_tokens(), 0);
    }

    #[test]
    fn token_bucket_exact_rate_calculation() {
        let mut bucket = TokenBucket::new(1000, 10000);
        bucket.try_consume(10000, 0);

        bucket.refill(500);
        assert_eq!(bucket.available_tokens(), 500);
    }

    #[test]
    fn throttle_decision_equality() {
        assert_eq!(ThrottleDecision::Allowed, ThrottleDecision::Allowed);
        assert_eq!(
            ThrottleDecision::Throttled {
                retry_after_ms: 100
            },
            ThrottleDecision::Throttled {
                retry_after_ms: 100
            }
        );
        assert_ne!(
            ThrottleDecision::Throttled {
                retry_after_ms: 100
            },
            ThrottleDecision::Throttled {
                retry_after_ms: 200
            }
        );
    }

    #[test]
    fn throttle_config_clone() {
        let config = ThrottleConfig {
            rate_bytes_per_sec: 500,
            burst_bytes: 1000,
        };
        let cloned = config.clone();
        assert_eq!(config.rate_bytes_per_sec, cloned.rate_bytes_per_sec);
        assert_eq!(config.burst_bytes, cloned.burst_bytes);
    }

    #[test]
    fn throttle_stats_default() {
        let stats = ThrottleStats::default();
        assert_eq!(stats.requests_allowed, 0);
        assert_eq!(stats.requests_throttled, 0);
        assert_eq!(stats.bytes_allowed, 0);
    }

    #[test]
    fn throttle_stats_clone() {
        let stats = ThrottleStats {
            requests_allowed: 10,
            requests_throttled: 5,
            bytes_allowed: 1000,
        };
        let cloned = stats.clone();
        assert_eq!(stats.requests_allowed, cloned.requests_allowed);
    }

    #[test]
    fn multiple_requests_tracking() {
        let config = ThrottleConfig {
            rate_bytes_per_sec: 100,
            burst_bytes: 1000,
        };
        let mut throttle = BandwidthThrottle::new(config);

        for _ in 0..10 {
            throttle.request(100, 0);
        }

        assert_eq!(throttle.stats().requests_allowed, 10);
        assert_eq!(throttle.stats().bytes_allowed, 1000);
    }

    #[test]
    fn retry_after_ms_calculation() {
        let config = ThrottleConfig {
            rate_bytes_per_sec: 1000,
            burst_bytes: 100,
        };
        let mut throttle = BandwidthThrottle::new(config);

        throttle.request(100, 0);
        let decision = throttle.request(1000, 0);

        if let ThrottleDecision::Throttled { retry_after_ms } = decision {
            assert!(retry_after_ms > 0);
        } else {
            panic!("Expected Throttled");
        }
    }

    #[test]
    fn bucket_consume_exact_burst() {
        let mut bucket = TokenBucket::new(1000, 5000);
        let result = bucket.try_consume(5000, 0);
        assert!(result);
        assert_eq!(bucket.available_tokens(), 0);
    }

    #[test]
    fn bucket_consume_more_than_burst() {
        let mut bucket = TokenBucket::new(1000, 5000);
        let result = bucket.try_consume(6000, 0);
        assert!(!result);
        assert_eq!(bucket.available_tokens(), 5000);
    }

    #[test]
    fn bandwidth_throttle_multiple_refills() {
        let config = ThrottleConfig {
            rate_bytes_per_sec: 1000,
            burst_bytes: 5000,
        };
        let mut throttle = BandwidthThrottle::new(config);

        throttle.request(5000, 0);
        throttle.request(500, 500);
        throttle.request(500, 1000);

        assert_eq!(throttle.stats().requests_allowed, 3);
    }

    #[test]
    fn throttle_stats_update_correctly() {
        let config = ThrottleConfig {
            rate_bytes_per_sec: 100,
            burst_bytes: 200,
        };
        let mut throttle = BandwidthThrottle::new(config);

        throttle.request(200, 0);
        throttle.request(100, 0);
        throttle.request(100, 1000);

        assert_eq!(throttle.stats().requests_allowed, 2);
        assert_eq!(throttle.stats().requests_throttled, 1);
    }
}
