#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RateLimitDecision {
    Allow,
    Throttle { wait_ms: u64 },
    Reject,
}

#[derive(Debug, Clone)]
pub struct RateLimiterConfig {
    pub bytes_per_sec: u64,
    pub ops_per_sec: u64,
    pub burst_factor: f64,
    pub reject_threshold: f64,
}

impl Default for RateLimiterConfig {
    fn default() -> Self {
        Self {
            bytes_per_sec: 0,
            ops_per_sec: 0,
            burst_factor: 2.0,
            reject_threshold: 0.0,
        }
    }
}

pub struct TokenBucket {
    tokens: f64,
    capacity: f64,
    refill_rate: f64,
    last_refill_ms: u64,
}

impl TokenBucket {
    pub fn new(rate_per_sec: u64, burst_factor: f64) -> Self {
        if rate_per_sec == 0 {
            TokenBucket {
                tokens: 0.0,
                capacity: 0.0,
                refill_rate: 0.0,
                last_refill_ms: 0,
            }
        } else {
            let capacity = (rate_per_sec as f64) * burst_factor;
            TokenBucket {
                tokens: capacity,
                capacity,
                refill_rate: rate_per_sec as f64,
                last_refill_ms: u64::MAX,
            }
        }
    }

    pub fn refill(&mut self, now_ms: u64) -> f64 {
        if self.capacity == 0.0 {
            return 0.0;
        }

        if self.last_refill_ms == u64::MAX {
            self.last_refill_ms = now_ms;
            return self.tokens;
        }

        let elapsed_sec = (now_ms - self.last_refill_ms) as f64 / 1000.0;
        self.tokens = (self.tokens + elapsed_sec * self.refill_rate).min(self.capacity);
        self.last_refill_ms = now_ms;
        self.tokens
    }

    pub fn try_consume(&mut self, amount: f64, now_ms: u64) -> bool {
        if self.capacity == 0.0 {
            return true;
        }

        self.refill(now_ms);

        if self.tokens >= amount {
            self.tokens -= amount;
            true
        } else {
            false
        }
    }

    pub fn wait_ms_for(&self, amount: f64) -> u64 {
        if self.capacity == 0.0 {
            return 0;
        }

        if self.tokens >= amount {
            return 0;
        }

        let needed = amount - self.tokens;
        if self.refill_rate <= 0.0 {
            return u64::MAX;
        }
        let wait_sec = needed / self.refill_rate;
        (wait_sec * 1000.0).ceil() as u64
    }

    pub fn fill_level(&self) -> f64 {
        if self.capacity == 0.0 {
            0.0
        } else {
            self.tokens / self.capacity
        }
    }

    pub fn is_unlimited(&self) -> bool {
        self.capacity == 0.0
    }
}

pub struct IoRateLimiter {
    config: RateLimiterConfig,
    bytes_bucket: Option<TokenBucket>,
    ops_bucket: Option<TokenBucket>,
    total_allowed: u64,
    total_throttled: u64,
    total_rejected: u64,
}

impl IoRateLimiter {
    pub fn new(config: RateLimiterConfig) -> Self {
        let bytes_bucket = if config.bytes_per_sec > 0 {
            Some(TokenBucket::new(config.bytes_per_sec, config.burst_factor))
        } else {
            None
        };

        let ops_bucket = if config.ops_per_sec > 0 {
            Some(TokenBucket::new(config.ops_per_sec, config.burst_factor))
        } else {
            None
        };

        IoRateLimiter {
            config,
            bytes_bucket,
            ops_bucket,
            total_allowed: 0,
            total_throttled: 0,
            total_rejected: 0,
        }
    }

    pub fn check_io(&mut self, bytes: u64, now_ms: u64) -> RateLimitDecision {
        if let Some(ref mut bucket) = self.bytes_bucket {
            if self.config.reject_threshold > 0.0 {
                let fill = bucket.fill_level();
                if fill < self.config.reject_threshold {
                    self.total_rejected += 1;
                    return RateLimitDecision::Reject;
                }
            }

            if !bucket.try_consume(bytes as f64, now_ms) {
                self.total_throttled += 1;
                let wait = bucket.wait_ms_for(bytes as f64);
                return RateLimitDecision::Throttle { wait_ms: wait };
            }
        }

        if let Some(ref mut bucket) = self.ops_bucket {
            if self.config.reject_threshold > 0.0 {
                let fill = bucket.fill_level();
                if fill < self.config.reject_threshold {
                    self.total_rejected += 1;
                    return RateLimitDecision::Reject;
                }
            }

            if !bucket.try_consume(1.0, now_ms) {
                self.total_throttled += 1;
                let wait = bucket.wait_ms_for(1.0);
                return RateLimitDecision::Throttle { wait_ms: wait };
            }
        }

        self.total_allowed += 1;
        RateLimitDecision::Allow
    }

    pub fn check_op(&mut self, now_ms: u64) -> RateLimitDecision {
        if let Some(ref mut bucket) = self.ops_bucket {
            if self.config.reject_threshold > 0.0 {
                let fill = bucket.fill_level();
                if fill < self.config.reject_threshold {
                    self.total_rejected += 1;
                    return RateLimitDecision::Reject;
                }
            }

            if !bucket.try_consume(1.0, now_ms) {
                self.total_throttled += 1;
                let wait = bucket.wait_ms_for(1.0);
                return RateLimitDecision::Throttle { wait_ms: wait };
            }
        }

        self.total_allowed += 1;
        RateLimitDecision::Allow
    }

    pub fn total_allowed(&self) -> u64 {
        self.total_allowed
    }

    pub fn total_throttled(&self) -> u64 {
        self.total_throttled
    }

    pub fn total_rejected(&self) -> u64 {
        self.total_rejected
    }

    pub fn is_limited(&self) -> bool {
        self.config.bytes_per_sec > 0 || self.config.ops_per_sec > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_bucket_new() {
        let bucket = TokenBucket::new(1000, 2.0);
        assert_eq!(bucket.capacity, 2000.0);
        assert_eq!(bucket.refill_rate, 1000.0);
    }

    #[test]
    fn test_token_bucket_unlimited() {
        let bucket = TokenBucket::new(0, 2.0);
        assert!(bucket.is_unlimited());
    }

    #[test]
    fn test_token_bucket_try_consume() {
        let mut bucket = TokenBucket::new(1000, 2.0);
        assert!(bucket.try_consume(100.0, 1000));
        assert!(bucket.try_consume(100.0, 1000));
    }

    #[test]
    fn test_token_bucket_try_consume_fails() {
        let mut bucket = TokenBucket::new(1000, 2.0);
        assert!(!bucket.try_consume(3000.0, 1000));
    }

    #[test]
    fn test_token_bucket_refill() {
        let mut bucket = TokenBucket::new(1000, 2.0);
        bucket.try_consume(1500.0, 0);

        let tokens = bucket.refill(2000);
        assert!(tokens > 1000.0);
    }

    #[test]
    fn test_token_bucket_wait_ms_for() {
        let bucket = TokenBucket::new(1000, 2.0);
        assert_eq!(bucket.wait_ms_for(100.0), 0);
    }

    #[test]
    fn test_token_bucket_wait_ms_for_blocking() {
        let mut bucket = TokenBucket::new(1000, 5.0);
        bucket.try_consume(4500.0, 0);

        let wait = bucket.wait_ms_for(1000.0);
        assert!(wait > 0);
    }

    #[test]
    fn test_token_bucket_fill_level() {
        let mut bucket = TokenBucket::new(1000, 2.0);
        assert!((bucket.fill_level() - 1.0).abs() < 0.001);

        bucket.try_consume(1000.0, 0);
        let fill = bucket.fill_level();
        assert!(fill < 1.0 && fill > 0.0);
    }

    #[test]
    fn test_unlimited_config_allows_everything() {
        let config = RateLimiterConfig {
            bytes_per_sec: 0,
            ops_per_sec: 0,
            burst_factor: 2.0,
            reject_threshold: 0.0,
        };
        let mut limiter = IoRateLimiter::new(config);

        let result = limiter.check_io(1000000, 0);
        assert!(matches!(result, RateLimitDecision::Allow));

        let result = limiter.check_op(0);
        assert!(matches!(result, RateLimitDecision::Allow));
    }

    #[test]
    fn test_byte_limiter_throttles() {
        let config = RateLimiterConfig {
            bytes_per_sec: 1000,
            ops_per_sec: 0,
            burst_factor: 1.0,
            reject_threshold: 0.0,
        };
        let mut limiter = IoRateLimiter::new(config);

        let result = limiter.check_io(2000, 0);
        let decision = match result {
            RateLimitDecision::Throttle { wait_ms } => {
                assert!(wait_ms > 0);
                true
            }
            _ => false,
        };
        assert!(decision);
    }

    #[test]
    fn test_op_limiter_counts_ops() {
        let config = RateLimiterConfig {
            bytes_per_sec: 0,
            ops_per_sec: 10,
            burst_factor: 1.0,
            reject_threshold: 0.0,
        };
        let mut limiter = IoRateLimiter::new(config);

        for _ in 0..20 {
            limiter.check_op(0);
        }

        let throttled = limiter.total_throttled();
        assert!(throttled > 0);
    }

    #[test]
    fn test_burst_allows_up_to_burst_factor() {
        let config = RateLimiterConfig {
            bytes_per_sec: 1000,
            ops_per_sec: 0,
            burst_factor: 3.0,
            reject_threshold: 0.0,
        };
        let mut limiter = IoRateLimiter::new(config);

        let result = limiter.check_io(2500, 0);
        assert!(matches!(result, RateLimitDecision::Allow));

        let result = limiter.check_io(1000, 0);
        assert!(matches!(result, RateLimitDecision::Throttle { .. }));
    }

    #[test]
    fn test_reject_threshold_works() {
        let config = RateLimiterConfig {
            bytes_per_sec: 1000,
            ops_per_sec: 0,
            burst_factor: 1.0,
            reject_threshold: 0.5,
        };
        let mut limiter = IoRateLimiter::new(config);

        limiter.check_io(600, 0);

        let result = limiter.check_io(100, 1000);
        assert!(matches!(result, RateLimitDecision::Reject));
    }

    #[test]
    fn test_wait_ms_for_returns_sensible_values() {
        let bucket = TokenBucket::new(1000, 1.0);
        let wait = bucket.wait_ms_for(100.0);
        assert_eq!(wait, 0);
    }

    #[test]
    fn test_stats_counters_increment() {
        let config = RateLimiterConfig {
            bytes_per_sec: 10000,
            ops_per_sec: 0,
            burst_factor: 1.0,
            reject_threshold: 0.0,
        };
        let mut limiter = IoRateLimiter::new(config);

        limiter.check_io(1000, 0);

        assert_eq!(limiter.total_allowed(), 1);
    }

    #[test]
    fn test_check_op_unlimited() {
        let config = RateLimiterConfig::default();
        let mut limiter = IoRateLimiter::new(config);

        let result = limiter.check_op(0);
        assert!(matches!(result, RateLimitDecision::Allow));
    }

    #[test]
    fn test_is_limited() {
        let config1 = RateLimiterConfig {
            bytes_per_sec: 1000,
            ops_per_sec: 0,
            burst_factor: 2.0,
            reject_threshold: 0.0,
        };
        let limiter1 = IoRateLimiter::new(config1);
        assert!(limiter1.is_limited());

        let config2 = RateLimiterConfig::default();
        let limiter2 = IoRateLimiter::new(config2);
        assert!(!limiter2.is_limited());
    }

    #[test]
    fn test_throttle_increments_counter() {
        let config = RateLimiterConfig {
            bytes_per_sec: 100,
            ops_per_sec: 0,
            burst_factor: 1.0,
            reject_threshold: 0.0,
        };
        let mut limiter = IoRateLimiter::new(config);

        let result = limiter.check_io(1000, 0);
        if let RateLimitDecision::Throttle { .. } = result {
            assert_eq!(limiter.total_throttled(), 1);
        } else {
            panic!("Expected Throttle");
        }
    }

    #[test]
    fn test_reject_increments_counter() {
        let config = RateLimiterConfig {
            bytes_per_sec: 1000,
            ops_per_sec: 0,
            burst_factor: 1.0,
            reject_threshold: 0.5,
        };
        let mut limiter = IoRateLimiter::new(config);

        limiter.check_io(600, 0);

        let result = limiter.check_io(100, 1000);
        if let RateLimitDecision::Reject = result {
            assert_eq!(limiter.total_rejected(), 1);
        } else {
            panic!("Expected Reject");
        }
    }
}
