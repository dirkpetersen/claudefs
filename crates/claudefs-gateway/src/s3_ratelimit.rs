//! Per-token S3 API rate limiting using token bucket algorithm

use std::collections::HashMap;
use std::sync::Mutex;

/// Configuration for token bucket rate limiting.
#[derive(Debug, Clone, Copy)]
pub struct RateLimitConfig {
    /// Maximum sustained requests per second allowed.
    pub requests_per_second: u32,
    /// Maximum burst capacity (initial tokens available).
    pub burst_capacity: u32,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            requests_per_second: 1000,
            burst_capacity: 5000,
        }
    }
}

impl RateLimitConfig {
    /// Creates a new rate limit config with specified parameters.
    pub fn new(requests_per_second: u32, burst_capacity: u32) -> Self {
        Self {
            requests_per_second,
            burst_capacity,
        }
    }
    /// Conservative settings for low-throughput scenarios.
    pub fn conservative() -> Self {
        Self {
            requests_per_second: 100,
            burst_capacity: 500,
        }
    }
    /// Generous settings for high-throughput scenarios.
    pub fn generous() -> Self {
        Self {
            requests_per_second: 10000,
            burst_capacity: 50000,
        }
    }
}

#[derive(Debug)]
struct TokenBucket {
    tokens: f64,
    last_refill: f64,
    total_requests: u64,
    rejected_requests: u64,
}

impl TokenBucket {
    fn new(initial_tokens: f64) -> Self {
        Self {
            tokens: initial_tokens,
            last_refill: 0.0,
            total_requests: 0,
            rejected_requests: 0,
        }
    }

    fn try_consume(&mut self, now: f64, config: &RateLimitConfig) -> bool {
        let elapsed = (now - self.last_refill).max(0.0);
        let new_tokens = elapsed * config.requests_per_second as f64;
        self.tokens = (self.tokens + new_tokens).min(config.burst_capacity as f64);
        self.last_refill = now;

        self.total_requests += 1;
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            self.rejected_requests += 1;
            false
        }
    }
}

/// Statistics for a token's rate limiting behavior.
#[derive(Debug, Clone)]
pub struct RateLimiterStats {
    /// Total requests attempted.
    pub total_requests: u64,
    /// Requests rejected due to rate limiting.
    pub rejected_requests: u64,
    /// Current available tokens in the bucket.
    pub current_tokens: f64,
    /// Ratio of rejected to total requests (0.0 to 1.0).
    pub rejection_rate: f64,
}

/// Per-token S3 API rate limiter using token bucket algorithm.
pub struct S3RateLimiter {
    buckets: Mutex<HashMap<String, TokenBucket>>,
    config: RateLimitConfig,
}

impl S3RateLimiter {
    /// Creates a new rate limiter with the given config.
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            buckets: Mutex::new(HashMap::new()),
            config,
        }
    }

    /// Attempts to consume a token for the given token hash. Returns true if allowed.
    pub fn try_request(&self, token_hash: &str, now: f64) -> bool {
        let mut buckets = self.buckets.lock().unwrap_or_else(|e| e.into_inner());
        let bucket = buckets
            .entry(token_hash.to_string())
            .or_insert_with(|| TokenBucket::new(self.config.burst_capacity as f64));
        bucket.try_consume(now, &self.config)
    }

    /// Returns statistics for the given token, or None if not tracked.
    pub fn stats(&self, token_hash: &str) -> Option<RateLimiterStats> {
        let buckets = self.buckets.lock().unwrap_or_else(|e| e.into_inner());
        buckets.get(token_hash).map(|b| RateLimiterStats {
            total_requests: b.total_requests,
            rejected_requests: b.rejected_requests,
            current_tokens: b.tokens,
            rejection_rate: if b.total_requests > 0 {
                b.rejected_requests as f64 / b.total_requests as f64
            } else {
                0.0
            },
        })
    }

    /// Removes stale token buckets idle for longer than max_idle_seconds. Returns count removed.
    pub fn evict_stale(&self, now: f64, max_idle_seconds: f64) -> usize {
        let mut buckets = self.buckets.lock().unwrap_or_else(|e| e.into_inner());
        let before = buckets.len();
        buckets.retain(|_, b| (now - b.last_refill) < max_idle_seconds);
        before - buckets.len()
    }

    /// Returns the number of currently tracked token buckets.
    pub fn tracked_count(&self) -> usize {
        self.buckets.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    /// Returns a reference to the rate limit configuration.
    pub fn config(&self) -> &RateLimitConfig {
        &self.config
    }
}

impl Default for S3RateLimiter {
    fn default() -> Self {
        Self::new(RateLimitConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limit_config_default_values() {
        let config = RateLimitConfig::default();
        assert_eq!(config.requests_per_second, 1000);
        assert_eq!(config.burst_capacity, 5000);
    }

    #[test]
    fn test_rate_limit_config_conservative() {
        let config = RateLimitConfig::conservative();
        assert_eq!(config.requests_per_second, 100);
        assert_eq!(config.burst_capacity, 500);
    }

    #[test]
    fn test_rate_limit_config_generous() {
        let config = RateLimitConfig::generous();
        assert_eq!(config.requests_per_second, 10000);
        assert_eq!(config.burst_capacity, 50000);
    }

    #[test]
    fn test_first_request_always_allowed() {
        let limiter = S3RateLimiter::new(RateLimitConfig::default());
        let result = limiter.try_request("token1", 0.0);
        assert!(result);
    }

    #[test]
    fn test_multiple_requests_within_burst_allowed() {
        let config = RateLimitConfig::new(100, 10);
        let limiter = S3RateLimiter::new(config);
        for _ in 0..10 {
            assert!(limiter.try_request("token1", 0.0));
        }
    }

    #[test]
    fn test_requests_exceeding_burst_rejected() {
        let config = RateLimitConfig::new(100, 5);
        let limiter = S3RateLimiter::new(config);
        for _ in 0..5 {
            assert!(limiter.try_request("token1", 0.0));
        }
        let result = limiter.try_request("token1", 0.0);
        assert!(!result);
    }

    #[test]
    fn test_try_request_returns_true_for_first_n_requests() {
        let config = RateLimitConfig::new(1000, 100);
        let limiter = S3RateLimiter::new(config);
        let mut allowed = 0;
        for _ in 0..100 {
            if limiter.try_request("token1", 0.0) {
                allowed += 1;
            }
        }
        assert_eq!(allowed, 100);
    }

    #[test]
    fn test_after_burst_exhausted_next_request_rejected() {
        let limiter = S3RateLimiter::new(RateLimitConfig::conservative());
        for _ in 0..500 {
            limiter.try_request("tok", 0.0);
        }
        let rejected = !limiter.try_request("tok", 0.0);
        assert!(rejected);
    }

    #[test]
    fn test_stats_returns_none_for_unknown_token() {
        let limiter = S3RateLimiter::default();
        let stats = limiter.stats("unknown");
        assert!(stats.is_none());
    }

    #[test]
    fn test_stats_total_requests_tracks_correctly() {
        let limiter = S3RateLimiter::default();
        limiter.try_request("token1", 0.0);
        limiter.try_request("token1", 0.0);
        limiter.try_request("token1", 0.0);
        let stats = limiter.stats("token1").unwrap();
        assert_eq!(stats.total_requests, 3);
    }

    #[test]
    fn test_stats_rejected_requests_tracks_correctly() {
        let config = RateLimitConfig::new(100, 2);
        let limiter = S3RateLimiter::new(config);
        limiter.try_request("tok", 0.0);
        limiter.try_request("tok", 0.0);
        limiter.try_request("tok", 0.0);
        limiter.try_request("tok", 0.0);
        let stats = limiter.stats("tok").unwrap();
        assert_eq!(stats.rejected_requests, 2);
    }

    #[test]
    fn test_stats_rejection_rate_zero_when_no_rejections() {
        let limiter = S3RateLimiter::default();
        limiter.try_request("token1", 0.0);
        let stats = limiter.stats("token1").unwrap();
        assert_eq!(stats.rejection_rate, 0.0);
    }

    #[test]
    fn test_stats_current_tokens_decreases_per_request() {
        let config = RateLimitConfig::new(1000, 10);
        let limiter = S3RateLimiter::new(config);
        limiter.try_request("token1", 0.0);
        limiter.try_request("token1", 0.0);
        let stats = limiter.stats("token1").unwrap();
        assert!((stats.current_tokens - 8.0).abs() < 0.001);
    }

    #[test]
    fn test_tracked_count_increases_with_new_tokens() {
        let limiter = S3RateLimiter::default();
        assert_eq!(limiter.tracked_count(), 0);
        limiter.try_request("token1", 0.0);
        assert_eq!(limiter.tracked_count(), 1);
        limiter.try_request("token2", 0.0);
        assert_eq!(limiter.tracked_count(), 2);
    }

    #[test]
    fn test_evict_stale_removes_old_entries() {
        let limiter = S3RateLimiter::default();
        limiter.try_request("old_token", 0.0);
        let removed = limiter.evict_stale(100.0, 60.0);
        assert_eq!(removed, 1);
    }

    #[test]
    fn test_evict_stale_keeps_recent_entries() {
        let limiter = S3RateLimiter::default();
        limiter.try_request("recent_token", 50.0);
        let removed = limiter.evict_stale(100.0, 60.0);
        assert_eq!(removed, 0);
    }

    #[test]
    fn test_two_different_tokens_have_independent_buckets() {
        let config = RateLimitConfig::new(1000, 3);
        let limiter = S3RateLimiter::new(config);
        for _ in 0..3 {
            assert!(limiter.try_request("tokenA", 0.0));
        }
        for _ in 0..3 {
            assert!(limiter.try_request("tokenB", 0.0));
        }
        let stats_a = limiter.stats("tokenA").unwrap();
        let stats_b = limiter.stats("tokenB").unwrap();
        assert_eq!(stats_a.rejected_requests, 0);
        assert_eq!(stats_b.rejected_requests, 0);
    }

    #[test]
    fn test_after_time_passes_tokens_refill() {
        let config = RateLimitConfig::conservative();
        let limiter = S3RateLimiter::new(config);
        for _ in 0..500 {
            limiter.try_request("token1", 0.0);
        }
        let result = limiter.try_request("token1", 10.0);
        assert!(result);
    }

    #[test]
    fn test_s3_rate_limiter_default_creates_with_default_config() {
        let limiter = S3RateLimiter::default();
        assert_eq!(limiter.config().requests_per_second, 1000);
        assert_eq!(limiter.config().burst_capacity, 5000);
    }

    #[test]
    fn test_evict_stale_returns_count_of_removed_entries() {
        let limiter = S3RateLimiter::default();
        limiter.try_request("tok1", 0.0);
        limiter.try_request("tok2", 50.0);
        let removed = limiter.evict_stale(100.0, 30.0);
        assert_eq!(removed, 2);
    }
}
