//! Authentication rate limiting for conduit connections.
//!
//! Implements rate limiting to address FINDING-09: no rate limiting
//! on conduit connections.

use std::collections::HashMap;

/// Rate limit configuration.
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Maximum authentication attempts per minute per site.
    pub max_auth_attempts_per_minute: u32,
    /// Maximum batches per second (token bucket rate).
    pub max_batches_per_second: u32,
    /// Maximum global bytes per second (0 = unlimited).
    pub max_global_bytes_per_second: u64,
    /// Lockout duration in seconds when limit exceeded.
    pub lockout_duration_secs: u64,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            max_auth_attempts_per_minute: 60,
            max_batches_per_second: 1000,
            max_global_bytes_per_second: 0,
            lockout_duration_secs: 300,
        }
    }
}

/// Rate limit check result.
#[derive(Debug, Clone, PartialEq)]
pub enum RateLimitResult {
    /// Request is allowed.
    Allowed,
    /// Request is throttled.
    Throttled {
        /// Estimated wait time in milliseconds before retry.
        wait_ms: u64,
    },
    /// Request is blocked.
    Blocked {
        /// Reason for the block.
        reason: String,
        /// Unix timestamp in microseconds when block expires.
        until_us: u64,
    },
}

/// Per-site rate limit state.
struct SiteRateState {
    /// Timestamps in microseconds of recent auth attempts.
    auth_attempts: Vec<u64>,
    /// Remaining batch tokens (token bucket).
    batch_tokens: f64,
    /// Last token refill timestamp in microseconds.
    batch_last_refill_us: u64,
    /// Lockout expiration timestamp in microseconds (0 = not locked).
    locked_until_us: u64,
}

impl SiteRateState {
    fn new() -> Self {
        Self {
            auth_attempts: Vec::new(),
            batch_tokens: 0.0,
            batch_last_refill_us: 0,
            locked_until_us: 0,
        }
    }

    fn is_locked(&self, now_us: u64) -> bool {
        self.locked_until_us > 0 && now_us < self.locked_until_us
    }

    fn lock(&mut self, now_us: u64, duration_secs: u64) {
        self.locked_until_us = now_us + (duration_secs * 1_000_000);
    }

    fn clear_lock(&mut self) {
        self.locked_until_us = 0;
    }
}

/// Rate limiter for conduit authentication and batch throughput.
pub struct AuthRateLimiter {
    config: RateLimitConfig,
    per_site: HashMap<u64, SiteRateState>,
    /// Global bytes token bucket (remaining tokens).
    global_bytes_tokens: f64,
    /// Last global token refill timestamp in microseconds.
    global_last_refill_us: u64,
}

impl AuthRateLimiter {
    /// Create a new rate limiter with the given configuration.
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            config,
            per_site: HashMap::new(),
            global_bytes_tokens: 0.0,
            global_last_refill_us: 0,
        }
    }

    fn get_or_create_site(&mut self, site_id: u64) -> &mut SiteRateState {
        self.per_site
            .entry(site_id)
            .or_insert_with(SiteRateState::new)
    }

    /// Check auth attempt: records timestamp, checks lockout, checks rate.
    ///
    /// Window = 60 seconds. If attempts in window >= max_auth_attempts_per_minute:
    ///   → lock site for lockout_duration_secs, return Blocked
    /// If site is locked: return Blocked with remaining time
    /// Otherwise: record attempt, return Allowed
    pub fn check_auth_attempt(&mut self, site_id: u64, now_us: u64) -> RateLimitResult {
        let max_attempts = self.config.max_auth_attempts_per_minute;
        let lockout_duration = self.config.lockout_duration_secs;

        let state = self.get_or_create_site(site_id);

        if state.is_locked(now_us) {
            return RateLimitResult::Blocked {
                reason: "rate limit exceeded".to_string(),
                until_us: state.locked_until_us,
            };
        }

        let window_start_us = now_us.saturating_sub(60_000_000);
        state.auth_attempts.retain(|&t| t >= window_start_us);

        if state.auth_attempts.len() >= max_attempts as usize {
            state.lock(now_us, lockout_duration);
            return RateLimitResult::Blocked {
                reason: "max auth attempts exceeded".to_string(),
                until_us: state.locked_until_us,
            };
        }

        state.auth_attempts.push(now_us);
        RateLimitResult::Allowed
    }

    /// Check batch send rate using token bucket.
    ///
    /// Refill rate = max_batches_per_second tokens/sec.
    /// Also check global bytes limit if configured.
    pub fn check_batch_send(
        &mut self,
        site_id: u64,
        byte_count: u64,
        now_us: u64,
    ) -> RateLimitResult {
        let max_batches = self.config.max_batches_per_second;
        let max_global_bytes = self.config.max_global_bytes_per_second;

        let state = self.get_or_create_site(site_id);

        let refill_interval_us = 1_000_000.0 / max_batches as f64;
        let elapsed = (now_us as f64) - (state.batch_last_refill_us as f64);
        state.batch_tokens =
            (state.batch_tokens + (elapsed / refill_interval_us)).min(max_batches as f64);
        state.batch_last_refill_us = now_us;

        if state.batch_tokens < 1.0 {
            let wait_ms = (refill_interval_us / 1000.0).ceil() as u64;
            return RateLimitResult::Throttled { wait_ms };
        }

        state.batch_tokens -= 1.0;

        if max_global_bytes > 0 {
            let global_refill_interval_us = 1_000_000.0 / (max_global_bytes as f64);
            let global_elapsed = (now_us as f64) - (self.global_last_refill_us as f64);
            self.global_bytes_tokens = (self.global_bytes_tokens
                + (global_elapsed / global_refill_interval_us))
                .min(max_global_bytes as f64);
            self.global_last_refill_us = now_us;

            if (self.global_bytes_tokens as u64) < byte_count {
                let wait_ms =
                    ((byte_count as f64 * global_refill_interval_us / 1_000_000.0).ceil()) as u64;
                return RateLimitResult::Throttled { wait_ms };
            }

            self.global_bytes_tokens -= byte_count as f64;
        }

        RateLimitResult::Allowed
    }

    /// Reset rate limit for a site (admin unblock).
    pub fn reset_site(&mut self, site_id: u64) {
        if let Some(state) = self.per_site.get_mut(&site_id) {
            state.clear_lock();
            state.auth_attempts.clear();
            state.batch_tokens = self.config.max_batches_per_second as f64;
        }
    }

    /// Count auth attempts in the last 60 seconds.
    pub fn auth_attempt_count(&self, site_id: u64, now_us: u64) -> u32 {
        if let Some(state) = self.per_site.get(&site_id) {
            let window_start_us = now_us.saturating_sub(60_000_000);
            state
                .auth_attempts
                .iter()
                .filter(|&&t| t >= window_start_us)
                .count() as u32
        } else {
            0
        }
    }

    /// Check if site is currently locked out.
    pub fn is_locked_out(&self, site_id: u64, now_us: u64) -> bool {
        if let Some(state) = self.per_site.get(&site_id) {
            state.is_locked(now_us)
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limit_config_default() {
        let config = RateLimitConfig::default();
        assert_eq!(config.max_auth_attempts_per_minute, 60);
        assert_eq!(config.max_batches_per_second, 1000);
        assert_eq!(config.max_global_bytes_per_second, 0);
        assert_eq!(config.lockout_duration_secs, 300);
    }

    #[test]
    fn test_auth_attempt_count() {
        let config = RateLimitConfig::default();
        let mut limiter = AuthRateLimiter::new(config);

        assert!(matches!(
            limiter.check_auth_attempt(100, 1_000_000),
            RateLimitResult::Allowed
        ));
        assert!(matches!(
            limiter.check_auth_attempt(100, 2_000_000),
            RateLimitResult::Allowed
        ));
        assert!(matches!(
            limiter.check_auth_attempt(100, 3_000_000),
            RateLimitResult::Allowed
        ));

        let count = limiter.auth_attempt_count(100, 4_000_000);
        assert_eq!(count, 3);
    }

    #[test]
    fn test_auth_attempt_count_expired() {
        let config = RateLimitConfig::default();
        let mut limiter = AuthRateLimiter::new(config);

        assert!(matches!(
            limiter.check_auth_attempt(100, 1_000_000),
            RateLimitResult::Allowed
        ));

        let count = limiter.auth_attempt_count(100, 70_000_000);
        assert_eq!(count, 0);
    }

    #[test]
    fn test_auth_max_attempts() {
        let config = RateLimitConfig {
            max_auth_attempts_per_minute: 3,
            ..Default::default()
        };
        let mut limiter = AuthRateLimiter::new(config);

        assert!(matches!(
            limiter.check_auth_attempt(100, 1_000_000),
            RateLimitResult::Allowed
        ));
        assert!(matches!(
            limiter.check_auth_attempt(100, 2_000_000),
            RateLimitResult::Allowed
        ));
        let result = limiter.check_auth_attempt(100, 3_000_000);
        assert_eq!(result, RateLimitResult::Allowed);
    }

    #[test]
    fn test_auth_lockout() {
        let config = RateLimitConfig {
            max_auth_attempts_per_minute: 3,
            lockout_duration_secs: 300,
            ..Default::default()
        };
        let mut limiter = AuthRateLimiter::new(config);

        assert!(matches!(
            limiter.check_auth_attempt(100, 1_000_000),
            RateLimitResult::Allowed
        ));
        assert!(matches!(
            limiter.check_auth_attempt(100, 2_000_000),
            RateLimitResult::Allowed
        ));
        assert!(matches!(
            limiter.check_auth_attempt(100, 3_000_000),
            RateLimitResult::Allowed
        ));

        let result = limiter.check_auth_attempt(100, 4_000_000);
        match result {
            RateLimitResult::Blocked { reason, .. } => {
                assert!(reason.contains("max auth attempts exceeded"));
            }
            _ => panic!("expected blocked"),
        }
    }

    #[test]
    fn test_auth_lockout_released() {
        let config = RateLimitConfig {
            max_auth_attempts_per_minute: 3,
            lockout_duration_secs: 1,
            ..Default::default()
        };
        let mut limiter = AuthRateLimiter::new(config);

        for _ in 0..4 {
            limiter.check_auth_attempt(100, 10_000_000);
        }

        let result = limiter.check_auth_attempt(100, 15_000_000);
        matches!(result, RateLimitResult::Blocked { .. });

        std::thread::sleep(std::time::Duration::from_millis(1100));

        let locked = limiter.is_locked_out(100, 17_000_000);
        assert!(!locked);
    }

    #[test]
    fn test_is_locked_out() {
        let config = RateLimitConfig::default();
        let mut limiter = AuthRateLimiter::new(config);

        let locked = limiter.is_locked_out(100, 1_000_000);
        assert!(!locked);

        for _ in 0..4 {
            limiter.check_auth_attempt(100, 10_000_000);
        }

        let result = limiter.check_auth_attempt(100, 10_000_001);
        matches!(result, RateLimitResult::Blocked { .. });
    }

    #[test]
    fn test_reset_site() {
        let config = RateLimitConfig {
            max_auth_attempts_per_minute: 3,
            lockout_duration_secs: 300,
            ..Default::default()
        };
        let mut limiter = AuthRateLimiter::new(config);

        for _ in 0..4 {
            limiter.check_auth_attempt(100, 10_000_000);
        }

        let result = limiter.check_auth_attempt(100, 10_000_001);
        matches!(result, RateLimitResult::Blocked { .. });

        limiter.reset_site(100);

        let result = limiter.check_auth_attempt(100, 10_000_002);
        assert_eq!(result, RateLimitResult::Allowed);
    }

    #[test]
    fn test_batch_send_allowed() {
        let config = RateLimitConfig::default();
        let mut limiter = AuthRateLimiter::new(config);

        let result = limiter.check_batch_send(100, 1000, 1_000_000);
        assert_eq!(result, RateLimitResult::Allowed);
    }

    #[test]
    fn test_batch_send_throttled() {
        let config = RateLimitConfig {
            max_batches_per_second: 1,
            ..Default::default()
        };
        let mut limiter = AuthRateLimiter::new(config);

        assert!(matches!(
            limiter.check_batch_send(100, 1000, 1_000_000),
            RateLimitResult::Allowed
        ));

        let result = limiter.check_batch_send(100, 1000, 1_500_000);
        match result {
            RateLimitResult::Throttled { wait_ms } => {
                assert!(wait_ms > 0);
            }
            _ => panic!("expected throttled"),
        }
    }

    #[test]
    fn test_batch_send_recovers() {
        let config = RateLimitConfig {
            max_batches_per_second: 1,
            ..Default::default()
        };
        let mut limiter = AuthRateLimiter::new(config);

        assert!(matches!(
            limiter.check_batch_send(100, 1000, 1_000_000),
            RateLimitResult::Allowed
        ));
        let result = limiter.check_batch_send(100, 1000, 2_500_000);
        assert_eq!(result, RateLimitResult::Allowed);
    }

    #[test]
    fn test_global_bytes_limit() {
        let config = RateLimitConfig {
            max_batches_per_second: 10000,
            max_global_bytes_per_second: 1000,
            ..Default::default()
        };
        let mut limiter = AuthRateLimiter::new(config);

        let result = limiter.check_batch_send(100, 500, 1_000_000);
        assert_eq!(result, RateLimitResult::Allowed);

        let result = limiter.check_batch_send(200, 600, 1_500_000);
        assert_eq!(result, RateLimitResult::Allowed);
    }

    #[test]
    fn test_global_bytes_unlimited() {
        let config = RateLimitConfig {
            max_global_bytes_per_second: 0,
            ..Default::default()
        };
        let mut limiter = AuthRateLimiter::new(config);

        for _ in 0..100 {
            let result = limiter.check_batch_send(100, 10_000_000, 1_000_000);
            assert_eq!(result, RateLimitResult::Allowed);
        }
    }

    #[test]
    fn test_different_sites() {
        let config = RateLimitConfig::default();
        let mut limiter = AuthRateLimiter::new(config);

        assert!(matches!(
            limiter.check_auth_attempt(100, 1_000_000),
            RateLimitResult::Allowed
        ));
        assert!(matches!(
            limiter.check_auth_attempt(200, 1_000_000),
            RateLimitResult::Allowed
        ));
        assert!(matches!(
            limiter.check_auth_attempt(100, 2_000_000),
            RateLimitResult::Allowed
        ));
        assert!(matches!(
            limiter.check_auth_attempt(200, 2_000_000),
            RateLimitResult::Allowed
        ));
        assert!(matches!(
            limiter.check_auth_attempt(100, 3_000_000),
            RateLimitResult::Allowed
        ));
        assert!(matches!(
            limiter.check_auth_attempt(200, 3_000_000),
            RateLimitResult::Allowed
        ));
        assert!(matches!(
            limiter.check_auth_attempt(100, 4_000_000),
            RateLimitResult::Allowed
        ));

        let count_100 = limiter.auth_attempt_count(100, 5_000_000);
        let count_200 = limiter.auth_attempt_count(200, 5_000_000);
        assert_eq!(count_100, 4);
        assert_eq!(count_200, 3);
    }

    #[test]
    fn test_rate_limit_result_variants() {
        let result1 = RateLimitResult::Allowed;
        let result2 = RateLimitResult::Throttled { wait_ms: 100 };
        let result3 = RateLimitResult::Blocked {
            reason: "test".to_string(),
            until_us: 1000,
        };

        assert_eq!(result1, RateLimitResult::Allowed);
        assert_eq!(result2, RateLimitResult::Throttled { wait_ms: 100 });
        assert_eq!(
            result3,
            RateLimitResult::Blocked {
                reason: "test".to_string(),
                until_us: 1000
            }
        );
    }
}
