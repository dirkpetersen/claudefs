//! Per-client metadata operation rate limiting.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// Configuration for per-client rate limiting.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RateLimitConfig {
    /// Default operations per second allowed per client.
    pub default_ops_per_sec: u64,
    /// Multiplier for burst capacity (tokens = ops_per_sec * burst_multiplier).
    pub burst_multiplier: f64,
    /// Time window in seconds for rate calculation.
    pub window_secs: u64,
    /// Backoff time in milliseconds when throttled.
    pub penalty_backoff_ms: u64,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            default_ops_per_sec: 10000,
            burst_multiplier: 2.0,
            window_secs: 1,
            penalty_backoff_ms: 100,
        }
    }
}

struct ClientBucket {
    tokens: f64,
    max_tokens: f64,
    refill_rate: f64,
    last_refill_ms: u64,
}

impl ClientBucket {
    fn new(ops_per_sec: u64, burst_multiplier: f64) -> Self {
        let max_tokens = ops_per_sec as f64 * burst_multiplier;
        Self {
            tokens: max_tokens,
            max_tokens,
            refill_rate: ops_per_sec as f64 / 1000.0,
            last_refill_ms: u64::MAX,
        }
    }

    fn refill(&mut self, now_ms: u64) {
        if self.last_refill_ms == u64::MAX {
            self.last_refill_ms = now_ms;
            return;
        }
        let elapsed_ms = now_ms.saturating_sub(self.last_refill_ms);
        let tokens_to_add = elapsed_ms as f64 * self.refill_rate;
        self.tokens = (self.tokens + tokens_to_add).min(self.max_tokens);
        self.last_refill_ms = now_ms;
    }

    fn try_consume(&mut self, now_ms: u64) -> bool {
        self.refill(now_ms);
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

/// Result of a rate limit check.
#[derive(Clone, Debug, PartialEq)]
pub enum RateLimitDecision {
    /// Operation is allowed.
    Allowed,
    /// Operation is throttled, client should back off.
    Throttled {
        /// Recommended backoff time in milliseconds.
        backoff_ms: u64,
    },
    /// Operation is rejected (e.g., client is banned).
    Rejected {
        /// Reason for rejection.
        reason: String,
    },
}

/// Unique identifier for a client.
pub type ClientId = u64;

/// Per-client rate limiter for metadata operations.
pub struct RateLimiter {
    config: RateLimitConfig,
    buckets: Mutex<HashMap<ClientId, ClientBucket>>,
    overrides: Mutex<HashMap<ClientId, u64>>,
    banned: Mutex<HashSet<ClientId>>,
    total_allowed: AtomicU64,
    total_throttled: AtomicU64,
    total_rejected: AtomicU64,
}

impl RateLimiter {
    /// Create a new rate limiter with the given configuration.
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            config,
            buckets: Mutex::new(HashMap::new()),
            overrides: Mutex::new(HashMap::new()),
            banned: Mutex::new(HashSet::new()),
            total_allowed: AtomicU64::new(0),
            total_throttled: AtomicU64::new(0),
            total_rejected: AtomicU64::new(0),
        }
    }

    /// Check if a client is allowed to proceed.
    ///
    /// Returns `Allowed` if the client has available tokens,
    /// `Throttled` if the client should retry after the backoff period,
    /// or `Rejected` if the client is banned.
    pub fn check(&self, client_id: ClientId, now_ms: u64) -> RateLimitDecision {
        if self.is_banned(client_id) {
            self.total_rejected.fetch_add(1, Ordering::Relaxed);
            return RateLimitDecision::Rejected {
                reason: "client is banned".to_string(),
            };
        }

        let ops_per_sec = {
            let overrides = self.overrides.lock().unwrap();
            *overrides
                .get(&client_id)
                .unwrap_or(&self.config.default_ops_per_sec)
        };

        let allowed = {
            let mut buckets = self.buckets.lock().unwrap();
            let bucket = buckets
                .entry(client_id)
                .or_insert_with(|| ClientBucket::new(ops_per_sec, self.config.burst_multiplier));
            let expected_refill_rate = ops_per_sec as f64 / 1000.0;
            if (bucket.refill_rate - expected_refill_rate).abs() > f64::EPSILON {
                *bucket = ClientBucket::new(ops_per_sec, self.config.burst_multiplier);
            }
            bucket.try_consume(now_ms)
        };

        if allowed {
            self.total_allowed.fetch_add(1, Ordering::Relaxed);
            RateLimitDecision::Allowed
        } else {
            self.total_throttled.fetch_add(1, Ordering::Relaxed);
            RateLimitDecision::Throttled {
                backoff_ms: self.config.penalty_backoff_ms,
            }
        }
    }

    /// Set a custom operations per second limit for a specific client.
    ///
    /// This overrides the default rate limit for the given client.
    pub fn set_override(&self, client_id: ClientId, ops_per_sec: u64) {
        let mut overrides = self.overrides.lock().unwrap();
        overrides.insert(client_id, ops_per_sec);
        let mut buckets = self.buckets.lock().unwrap();
        buckets.insert(
            client_id,
            ClientBucket::new(ops_per_sec, self.config.burst_multiplier),
        );
    }

    /// Remove the custom rate limit override for a client.
    ///
    /// The client will revert to using the default rate limit.
    pub fn remove_override(&self, client_id: ClientId) {
        let mut overrides = self.overrides.lock().unwrap();
        overrides.remove(&client_id);
        let mut buckets = self.buckets.lock().unwrap();
        buckets.remove(&client_id);
    }

    /// Ban a client, causing all requests to be rejected.
    pub fn ban(&self, client_id: ClientId) {
        let mut banned = self.banned.lock().unwrap();
        banned.insert(client_id);
    }

    /// Unban a previously banned client.
    pub fn unban(&self, client_id: ClientId) {
        let mut banned = self.banned.lock().unwrap();
        banned.remove(&client_id);
    }

    /// Check if a client is currently banned.
    pub fn is_banned(&self, client_id: ClientId) -> bool {
        let banned = self.banned.lock().unwrap();
        banned.contains(&client_id)
    }

    /// Returns the number of active clients with rate limit buckets.
    pub fn active_clients(&self) -> usize {
        let buckets = self.buckets.lock().unwrap();
        buckets.len()
    }

    /// Reset all rate limit buckets.
    ///
    /// This clears all client buckets but preserves overrides and bans.
    pub fn reset(&self) {
        let mut buckets = self.buckets.lock().unwrap();
        buckets.clear();
    }

    /// Get current rate limiter statistics.
    pub fn stats(&self) -> RateLimitStats {
        RateLimitStats {
            active_clients: self.active_clients(),
            total_allowed: self.total_allowed.load(Ordering::Relaxed),
            total_throttled: self.total_throttled.load(Ordering::Relaxed),
            total_rejected: self.total_rejected.load(Ordering::Relaxed),
            banned_count: {
                let banned = self.banned.lock().unwrap();
                banned.len()
            },
        }
    }
}

/// Statistics about the rate limiter's current state.
#[derive(Clone, Debug)]
pub struct RateLimitStats {
    /// Number of active clients with rate limit buckets.
    pub active_clients: usize,
    /// Total number of requests allowed.
    pub total_allowed: u64,
    /// Total number of requests throttled.
    pub total_throttled: u64,
    /// Total number of requests rejected (banned clients).
    pub total_rejected: u64,
    /// Number of currently banned clients.
    pub banned_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = RateLimitConfig::default();
        assert_eq!(config.default_ops_per_sec, 10000);
        assert_eq!(config.burst_multiplier, 2.0);
        assert_eq!(config.window_secs, 1);
        assert_eq!(config.penalty_backoff_ms, 100);
    }

    #[test]
    fn test_allowed_under_limit() {
        let config = RateLimitConfig::default();
        let limiter = RateLimiter::new(config);
        let result = limiter.check(1, 1000);
        assert_eq!(result, RateLimitDecision::Allowed);
    }

    #[test]
    fn test_throttled_over_limit() {
        let config = RateLimitConfig {
            default_ops_per_sec: 1,
            burst_multiplier: 1.0,
            window_secs: 1,
            penalty_backoff_ms: 100,
        };
        let limiter = RateLimiter::new(config);
        let _ = limiter.check(1, 1000);
        let result = limiter.check(1, 1000);
        assert_eq!(result, RateLimitDecision::Throttled { backoff_ms: 100 });
    }

    #[test]
    fn test_token_refill() {
        let config = RateLimitConfig {
            default_ops_per_sec: 1000,
            burst_multiplier: 1.0,
            window_secs: 1,
            penalty_backoff_ms: 100,
        };
        let limiter = RateLimiter::new(config);
        let _ = limiter.check(1, 0);
        let result = limiter.check(1, 1000);
        assert_eq!(result, RateLimitDecision::Allowed);
    }

    #[test]
    fn test_burst_allows_spike() {
        let config = RateLimitConfig {
            default_ops_per_sec: 1,
            burst_multiplier: 5.0,
            window_secs: 1,
            penalty_backoff_ms: 100,
        };
        let limiter = RateLimiter::new(config);
        for _ in 0..5 {
            let result = limiter.check(1, 0);
            assert_eq!(result, RateLimitDecision::Allowed);
        }
        let result = limiter.check(1, 0);
        assert_eq!(result, RateLimitDecision::Throttled { backoff_ms: 100 });
    }

    #[test]
    fn test_banned_client_rejected() {
        let config = RateLimitConfig::default();
        let limiter = RateLimiter::new(config);
        limiter.ban(42);
        let result = limiter.check(42, 1000);
        assert_eq!(
            result,
            RateLimitDecision::Rejected {
                reason: "client is banned".to_string()
            }
        );
    }

    #[test]
    fn test_ban_unban() {
        let config = RateLimitConfig::default();
        let limiter = RateLimiter::new(config);
        limiter.ban(42);
        assert!(limiter.is_banned(42));
        limiter.unban(42);
        assert!(!limiter.is_banned(42));
    }

    #[test]
    fn test_override_higher_limit() {
        let config = RateLimitConfig {
            default_ops_per_sec: 1,
            burst_multiplier: 1.0,
            window_secs: 1,
            penalty_backoff_ms: 100,
        };
        let limiter = RateLimiter::new(config);
        limiter.set_override(1, 10000);
        for _ in 0..10 {
            let result = limiter.check(1, 0);
            assert_eq!(result, RateLimitDecision::Allowed);
        }
    }

    #[test]
    fn test_override_lower_limit() {
        let config = RateLimitConfig {
            default_ops_per_sec: 10000,
            burst_multiplier: 1.0,
            window_secs: 1,
            penalty_backoff_ms: 100,
        };
        let limiter = RateLimiter::new(config);
        limiter.set_override(1, 1);
        let _ = limiter.check(1, 0);
        let result = limiter.check(1, 0);
        assert_eq!(result, RateLimitDecision::Throttled { backoff_ms: 100 });
    }

    #[test]
    fn test_remove_override() {
        let config = RateLimitConfig {
            default_ops_per_sec: 10000,
            burst_multiplier: 2.0,
            window_secs: 1,
            penalty_backoff_ms: 100,
        };
        let limiter = RateLimiter::new(config);
        limiter.set_override(1, 1);
        limiter.remove_override(1);
        for _ in 0..100 {
            let result = limiter.check(1, 0);
            assert_eq!(result, RateLimitDecision::Allowed);
        }
    }

    #[test]
    fn test_reset_clears_buckets() {
        let config = RateLimitConfig::default();
        let limiter = RateLimiter::new(config);
        let _ = limiter.check(1, 0);
        assert_eq!(limiter.active_clients(), 1);
        limiter.reset();
        assert_eq!(limiter.active_clients(), 0);
    }

    #[test]
    fn test_reset_preserves_bans() {
        let config = RateLimitConfig::default();
        let limiter = RateLimiter::new(config);
        limiter.ban(42);
        limiter.reset();
        assert!(limiter.is_banned(42));
    }

    #[test]
    fn test_reset_preserves_overrides() {
        let config = RateLimitConfig::default();
        let limiter = RateLimiter::new(config);
        limiter.set_override(1, 5000);
        limiter.reset();
        let overrides = limiter.overrides.lock().unwrap();
        assert!(overrides.contains_key(&1));
    }

    #[test]
    fn test_active_clients_count() {
        let config = RateLimitConfig::default();
        let limiter = RateLimiter::new(config);
        let _ = limiter.check(1, 0);
        let _ = limiter.check(2, 0);
        let _ = limiter.check(3, 0);
        assert_eq!(limiter.active_clients(), 3);
    }

    #[test]
    fn test_stats_allowed_counter() {
        let config = RateLimitConfig::default();
        let limiter = RateLimiter::new(config);
        let _ = limiter.check(1, 0);
        let _ = limiter.check(1, 0);
        let stats = limiter.stats();
        assert_eq!(stats.total_allowed, 2);
    }

    #[test]
    fn test_stats_throttled_counter() {
        let config = RateLimitConfig {
            default_ops_per_sec: 1,
            burst_multiplier: 1.0,
            window_secs: 1,
            penalty_backoff_ms: 100,
        };
        let limiter = RateLimiter::new(config);
        let _ = limiter.check(1, 0);
        let _ = limiter.check(1, 0);
        let _ = limiter.check(1, 0);
        let stats = limiter.stats();
        assert_eq!(stats.total_throttled, 2);
    }

    #[test]
    fn test_stats_rejected_counter() {
        let config = RateLimitConfig::default();
        let limiter = RateLimiter::new(config);
        limiter.ban(1);
        let _ = limiter.check(1, 0);
        let stats = limiter.stats();
        assert_eq!(stats.total_rejected, 1);
    }

    #[test]
    fn test_stats_banned_count() {
        let config = RateLimitConfig::default();
        let limiter = RateLimiter::new(config);
        limiter.ban(1);
        limiter.ban(2);
        limiter.ban(3);
        let stats = limiter.stats();
        assert_eq!(stats.banned_count, 3);
    }

    #[test]
    fn test_multiple_clients_independent() {
        let config = RateLimitConfig {
            default_ops_per_sec: 1,
            burst_multiplier: 1.0,
            window_secs: 1,
            penalty_backoff_ms: 100,
        };
        let limiter = RateLimiter::new(config);
        let _ = limiter.check(1, 0);
        let result2 = limiter.check(2, 0);
        assert_eq!(result2, RateLimitDecision::Allowed);
    }

    #[test]
    fn test_is_banned() {
        let config = RateLimitConfig::default();
        let limiter = RateLimiter::new(config);
        assert!(!limiter.is_banned(1));
        limiter.ban(1);
        assert!(limiter.is_banned(1));
    }

    #[test]
    fn test_throttle_backoff_ms() {
        let config = RateLimitConfig {
            default_ops_per_sec: 1,
            burst_multiplier: 1.0,
            window_secs: 1,
            penalty_backoff_ms: 500,
        };
        let limiter = RateLimiter::new(config);
        let _ = limiter.check(1, 0);
        let result = limiter.check(1, 0);
        assert_eq!(result, RateLimitDecision::Throttled { backoff_ms: 500 });
    }

    #[test]
    fn test_gradual_consumption() {
        let config = RateLimitConfig {
            default_ops_per_sec: 5,
            burst_multiplier: 1.0,
            window_secs: 1,
            penalty_backoff_ms: 100,
        };
        let limiter = RateLimiter::new(config);
        for i in 0..6 {
            let result = limiter.check(1, 0);
            if i < 5 {
                assert_eq!(result, RateLimitDecision::Allowed);
            } else {
                assert_eq!(result, RateLimitDecision::Throttled { backoff_ms: 100 });
            }
        }
        let result = limiter.check(1, 1000);
        assert_eq!(result, RateLimitDecision::Allowed);
    }
}
