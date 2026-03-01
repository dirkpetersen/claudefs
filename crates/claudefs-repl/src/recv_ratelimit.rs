//! Receive-path rate limiting for the replication conduit (addresses FINDING-09).
//!
//! Prevents a compromised peer from flooding the conduit with batches.
//! Uses a sliding-window token bucket: `Allow`, `Throttle`, or `Reject`.

/// Configuration for receive-path rate limiting.
#[derive(Debug, Clone, PartialEq)]
pub struct RateLimitConfig {
    /// Maximum batches allowed per second.
    pub max_batches_per_sec: u64,
    /// Maximum entries allowed per second.
    pub max_entries_per_sec: u64,
    /// Burst factor multiplier for burst allowance.
    pub burst_factor: f64,
    /// Window duration in milliseconds.
    pub window_ms: u64,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            max_batches_per_sec: 0,
            max_entries_per_sec: 0,
            burst_factor: 2.0,
            window_ms: 1000,
        }
    }
}

impl RateLimitConfig {
    /// Create a new config with the given limits (all other fields use defaults).
    pub fn new(max_batches_per_sec: u64, max_entries_per_sec: u64) -> Self {
        Self {
            max_batches_per_sec,
            max_entries_per_sec,
            ..Default::default()
        }
    }
}

/// Decision returned by the rate limiter.
#[derive(Debug, Clone, PartialEq)]
pub enum RateLimitDecision {
    /// The batch/entries are allowed through.
    Allow,
    /// The batch should be throttled (try again after delay_ms).
    Throttle {
        /// Recommended delay in milliseconds.
        delay_ms: u64,
    },
    /// The batch is rejected (too many entries or batches).
    Reject {
        /// Reason for rejection.
        reason: String,
    },
}

/// Statistics from the rate limiter.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RateLimiterStats {
    /// Total batches allowed.
    pub batches_allowed: u64,
    /// Total batches throttled.
    pub batches_throttled: u64,
    /// Total batches rejected.
    pub batches_rejected: u64,
    /// Total entries allowed.
    pub entries_allowed: u64,
    /// Total entries rejected.
    pub entries_rejected: u64,
    /// Number of times the window was reset.
    pub windows_reset: u64,
}

/// Rate limiter for the receive path.
pub struct RecvRateLimiter {
    config: RateLimitConfig,
    window_start_ms: u64,
    batches_in_window: u64,
    entries_in_window: u64,
    stats: RateLimiterStats,
}

impl RecvRateLimiter {
    /// Create a new rate limiter with the given config.
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            config,
            window_start_ms: 0,
            batches_in_window: 0,
            entries_in_window: 0,
            stats: RateLimiterStats::default(),
        }
    }

    /// Check if a batch with the given entry count can be processed.
    pub fn check_batch(&mut self, entry_count: usize, now_ms: u64) -> RateLimitDecision {
        let now = now_ms;

        if self.config.window_ms > 0 && now >= self.window_start_ms + self.config.window_ms {
            self.window_start_ms = now;
            self.batches_in_window = 0;
            self.entries_in_window = 0;
            self.stats.windows_reset += 1;
        }

        self.batches_in_window += 1;
        self.entries_in_window += entry_count as u64;

        let entry_count = entry_count as u64;

        if self.config.max_entries_per_sec > 0 {
            let normal_cap = self.config.max_entries_per_sec * self.config.window_ms / 1000;
            let burst_cap = (normal_cap as f64 * self.config.burst_factor) as u64;

            if self.entries_in_window > burst_cap {
                self.stats.entries_rejected += entry_count;
                self.stats.batches_rejected += 1;
                return RateLimitDecision::Reject {
                    reason: "entries exceeded burst limit".to_string(),
                };
            }
        }

        if self.config.max_batches_per_sec > 0 {
            let normal_cap = self.config.max_batches_per_sec * self.config.window_ms / 1000;
            let burst_cap = (normal_cap as f64 * self.config.burst_factor) as u64;

            if self.batches_in_window > burst_cap {
                self.stats.batches_rejected += 1;
                return RateLimitDecision::Reject {
                    reason: "batches exceeded burst limit".to_string(),
                };
            }

            if self.batches_in_window > normal_cap {
                self.stats.batches_throttled += 1;
                return RateLimitDecision::Throttle { delay_ms: 50 };
            }
        }

        self.stats.batches_allowed += 1;
        self.stats.entries_allowed += entry_count;
        RateLimitDecision::Allow
    }

    /// Reset the rate limiter state.
    pub fn reset(&mut self) {
        self.window_start_ms = 0;
        self.batches_in_window = 0;
        self.entries_in_window = 0;
    }

    /// Get the current statistics.
    pub fn stats(&self) -> &RateLimiterStats {
        &self.stats
    }

    /// Get the configuration.
    pub fn config(&self) -> &RateLimitConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_config(batches: u64, entries: u64) -> RateLimitConfig {
        RateLimitConfig {
            max_batches_per_sec: batches,
            max_entries_per_sec: entries,
            burst_factor: 2.0,
            window_ms: 1000,
        }
    }

    #[test]
    fn test_unlimited_config_allows() {
        let config = RateLimitConfig::default();
        let mut limiter = RecvRateLimiter::new(config);

        let decision = limiter.check_batch(100, 1000);
        assert!(matches!(decision, RateLimitDecision::Allow));
    }

    #[test]
    fn test_within_limit_allows() {
        let config = make_config(10, 1000);
        let mut limiter = RecvRateLimiter::new(config);

        for _ in 0..5 {
            let decision = limiter.check_batch(100, 1000);
            assert!(matches!(decision, RateLimitDecision::Allow));
        }
    }

    #[test]
    fn test_throttle_after_normal_limit() {
        let config = make_config(5, 10000);
        let mut limiter = RecvRateLimiter::new(config);

        for _ in 0..5 {
            limiter.check_batch(100, 1000);
        }

        let decision = limiter.check_batch(100, 1000);
        assert!(matches!(
            decision,
            RateLimitDecision::Throttle { delay_ms: 50 }
        ));
    }

    #[test]
    fn test_reject_after_burst_limit() {
        let config = make_config(5, 10000);
        let mut limiter = RecvRateLimiter::new(config);

        for _ in 0..15 {
            limiter.check_batch(100, 1000);
        }

        let decision = limiter.check_batch(100, 1000);
        assert!(matches!(decision, RateLimitDecision::Reject { .. }));
    }

    #[test]
    fn test_window_reset() {
        let mut config = make_config(10, 10000);
        config.window_ms = 100;
        let mut limiter = RecvRateLimiter::new(config);

        limiter.check_batch(100, 0);
        limiter.check_batch(100, 50);
        limiter.check_batch(100, 100);

        assert_eq!(limiter.stats().windows_reset, 1);
    }

    #[test]
    fn test_burst_factor() {
        let mut config = make_config(2, 1000);
        config.burst_factor = 3.0;
        let mut limiter = RecvRateLimiter::new(config);

        for i in 0..6 {
            let decision = limiter.check_batch(100, 1000);
            if i < 2 {
                assert!(matches!(decision, RateLimitDecision::Allow));
            } else {
                assert!(matches!(decision, RateLimitDecision::Throttle { .. }));
            }
        }

        let decision = limiter.check_batch(100, 1000);
        assert!(matches!(decision, RateLimitDecision::Reject { .. }));
    }

    #[test]
    fn test_stats_tracking_batches_allowed() {
        let config = make_config(10, 1000);
        let mut limiter = RecvRateLimiter::new(config);

        for _ in 0..3 {
            limiter.check_batch(50, 1000);
        }

        assert_eq!(limiter.stats().batches_allowed, 3);
    }

    #[test]
    fn test_stats_tracking_entries_allowed() {
        let config = make_config(10, 1000);
        let mut limiter = RecvRateLimiter::new(config);

        limiter.check_batch(50, 1000);
        limiter.check_batch(75, 1000);
        limiter.check_batch(100, 1000);

        assert_eq!(limiter.stats().entries_allowed, 225);
    }

    #[test]
    fn test_stats_tracking_throttled() {
        let config = make_config(2, 10000);
        let mut limiter = RecvRateLimiter::new(config);

        limiter.check_batch(100, 1000);
        limiter.check_batch(100, 1000);

        limiter.check_batch(100, 1000);
        limiter.check_batch(100, 1000);

        assert_eq!(limiter.stats().batches_throttled, 2);
    }

    #[test]
    fn test_stats_tracking_rejected() {
        let config = make_config(2, 10000);
        let mut limiter = RecvRateLimiter::new(config);

        for _ in 0..10 {
            limiter.check_batch(100, 1000);
        }

        assert!(limiter.stats().batches_rejected > 0);
    }

    #[test]
    fn test_reset_clears_state() {
        let config = make_config(5, 1000);
        let mut limiter = RecvRateLimiter::new(config);

        for _ in 0..5 {
            limiter.check_batch(100, 1000);
        }

        limiter.reset();

        let decision = limiter.check_batch(100, 2000);
        assert!(matches!(decision, RateLimitDecision::Allow));
    }

    #[test]
    fn test_large_batch_entries() {
        let config = make_config(100, 2_000_000);
        let mut limiter = RecvRateLimiter::new(config);

        let decision = limiter.check_batch(1_000_000, 1000);
        assert!(matches!(decision, RateLimitDecision::Allow));
        assert_eq!(limiter.stats().entries_allowed, 1_000_000);
    }

    #[test]
    fn test_multiple_windows() {
        let mut config = make_config(5, 1000);
        config.window_ms = 100;
        let mut limiter = RecvRateLimiter::new(config);

        for _ in 0..5 {
            limiter.check_batch(10, 0);
        }
        for _ in 0..5 {
            limiter.check_batch(10, 100);
        }
        for _ in 0..5 {
            limiter.check_batch(10, 200);
        }

        assert_eq!(limiter.stats().windows_reset, 2);
    }

    #[test]
    fn test_zero_entries_throttle() {
        let mut config = make_config(2, 100);
        config.burst_factor = 3.0;
        let mut limiter = RecvRateLimiter::new(config);

        for i in 0..4 {
            let decision = limiter.check_batch(0, 1000);
            if i < 2 {
                assert!(matches!(decision, RateLimitDecision::Allow));
            } else {
                assert!(matches!(decision, RateLimitDecision::Throttle { .. }));
            }
        }
    }

    #[test]
    fn test_config_accessors() {
        let config = make_config(5, 100);
        let limiter = RecvRateLimiter::new(config);

        assert_eq!(limiter.config().max_batches_per_sec, 5);
        assert_eq!(limiter.config().max_entries_per_sec, 100);
    }

    #[test]
    fn test_default_config() {
        let config = RateLimitConfig::default();
        assert_eq!(config.max_batches_per_sec, 0);
        assert_eq!(config.max_entries_per_sec, 0);
        assert_eq!(config.burst_factor, 2.0);
        assert_eq!(config.window_ms, 1000);
    }

    #[test]
    fn test_config_new() {
        let config = RateLimitConfig::new(10, 100);
        assert_eq!(config.max_batches_per_sec, 10);
        assert_eq!(config.max_entries_per_sec, 100);
    }

    #[test]
    fn test_reject_reason_contains_info() {
        let config = make_config(1, 10000);
        let mut limiter = RecvRateLimiter::new(config);

        limiter.check_batch(100, 1000);
        limiter.check_batch(100, 1000);
        limiter.check_batch(100, 1000);

        let decision = limiter.check_batch(100, 1000);
        if let RateLimitDecision::Reject { reason } = decision {
            assert!(!reason.is_empty());
        } else {
            panic!("expected Reject");
        }
    }
}
