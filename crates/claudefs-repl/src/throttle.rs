//! Cross-Site Bandwidth Throttling
//!
//! Controls the rate at which journal entries are sent to each remote site,
//! preventing replication from consuming all available WAN bandwidth.

use std::collections::HashMap;

/// Bandwidth limit configuration.
#[derive(Debug, Clone)]
pub struct ThrottleConfig {
    /// Maximum bytes per second to send to this site (0 = unlimited).
    pub max_bytes_per_sec: u64,
    /// Maximum entries per second (0 = unlimited).
    pub max_entries_per_sec: u64,
    /// Burst allowance: multiplier on max rate for short bursts (e.g., 2.0 = 2x burst).
    pub burst_factor: f64,
}

impl Default for ThrottleConfig {
    fn default() -> Self {
        Self {
            max_bytes_per_sec: 100 * 1024 * 1024,
            max_entries_per_sec: 10_000,
            burst_factor: 1.5,
        }
    }
}

/// Tracks token bucket state for one throttle dimension.
pub struct TokenBucket {
    capacity: u64,
    tokens: f64,
    refill_rate: f64,
    last_refill_us: u64,
}

impl TokenBucket {
    /// Create a new token bucket with the given capacity and refill rate (tokens/sec).
    pub fn new(capacity: u64, rate_per_sec: f64) -> Self {
        let refill_rate = rate_per_sec / 1_000_000.0;
        Self {
            capacity,
            tokens: capacity as f64,
            refill_rate,
            last_refill_us: current_time_us(),
        }
    }

    /// Try to consume `amount` tokens. Returns true if successful, false if insufficient.
    /// Updates internal state (refills first based on elapsed time).
    pub fn try_consume(&mut self, amount: u64, now_us: u64) -> bool {
        self.refill(now_us);

        let amount_f = amount as f64;
        if self.tokens >= amount_f {
            self.tokens -= amount_f;
            true
        } else {
            false
        }
    }

    /// Returns current token count (floored to u64).
    pub fn available(&self, now_us: u64) -> u64 {
        let mut bucket = Self {
            capacity: self.capacity,
            tokens: self.tokens,
            refill_rate: self.refill_rate,
            last_refill_us: self.last_refill_us,
        };
        bucket.refill(now_us);
        bucket.tokens.floor() as u64
    }

    /// Refill tokens based on elapsed time since last refill.
    pub fn refill(&mut self, now_us: u64) {
        let elapsed_us = now_us.saturating_sub(self.last_refill_us);
        if elapsed_us > 0 {
            let tokens_to_add = elapsed_us as f64 * self.refill_rate;
            self.tokens = (self.tokens + tokens_to_add).min(self.capacity as f64);
            self.last_refill_us = now_us;
        }
    }
}

/// Per-site throttle: combines byte-rate and entry-rate token buckets.
pub struct SiteThrottle {
    config: ThrottleConfig,
    byte_bucket: TokenBucket,
    entry_bucket: TokenBucket,
}

impl SiteThrottle {
    /// Create a new site throttle with the given config.
    pub fn new(config: ThrottleConfig) -> Self {
        let byte_capacity = if config.max_bytes_per_sec == 0 {
            u64::MAX
        } else {
            (config.max_bytes_per_sec as f64 * config.burst_factor)
                .max(config.max_bytes_per_sec as f64) as u64
        };
        let entry_capacity = if config.max_entries_per_sec == 0 {
            u64::MAX
        } else {
            (config.max_entries_per_sec as f64 * config.burst_factor)
                .max(config.max_entries_per_sec as f64) as u64
        };

        Self {
            config: config.clone(),
            byte_bucket: TokenBucket::new(byte_capacity, config.max_bytes_per_sec as f64),
            entry_bucket: TokenBucket::new(entry_capacity, config.max_entries_per_sec as f64),
        }
    }

    /// Check if we can send `byte_count` bytes and `entry_count` entries.
    /// Returns true if allowed and consumes tokens. Returns false if throttled.
    pub fn try_send(&mut self, byte_count: u64, entry_count: u64, now_us: u64) -> bool {
        let byte_allowed = if self.config.max_bytes_per_sec == 0 {
            true
        } else {
            self.byte_bucket.try_consume(byte_count, now_us)
        };

        let entry_allowed = if self.config.max_entries_per_sec == 0 {
            true
        } else {
            self.entry_bucket.try_consume(entry_count, now_us)
        };

        byte_allowed && entry_allowed
    }

    /// Get the current byte rate limit.
    pub fn max_bytes_per_sec(&self) -> u64 {
        self.config.max_bytes_per_sec
    }

    /// Get the current entry rate limit.
    pub fn max_entries_per_sec(&self) -> u64 {
        self.config.max_entries_per_sec
    }

    /// Update the throttle config (e.g., admin changed bandwidth limit).
    pub fn update_config(&mut self, config: ThrottleConfig) {
        self.config = config;
    }

    /// Returns how many bytes are available (capped at max_bytes_per_sec).
    pub fn available_bytes(&self, now_us: u64) -> u64 {
        if self.config.max_bytes_per_sec == 0 {
            return u64::MAX;
        }
        self.byte_bucket
            .available(now_us)
            .min(self.config.max_bytes_per_sec)
    }
}

/// Manages throttles for multiple remote sites.
pub struct ThrottleManager {
    per_site: HashMap<u64, SiteThrottle>,
    default_config: ThrottleConfig,
}

impl ThrottleManager {
    /// Create a new throttle manager with the given default config.
    pub fn new(default_config: ThrottleConfig) -> Self {
        Self {
            per_site: HashMap::new(),
            default_config,
        }
    }

    /// Register a site with a specific throttle config.
    pub fn register_site(&mut self, site_id: u64, config: ThrottleConfig) {
        self.per_site.insert(site_id, SiteThrottle::new(config));
    }

    /// Register a site with the default throttle config.
    pub fn register_site_default(&mut self, site_id: u64) {
        self.per_site
            .insert(site_id, SiteThrottle::new(self.default_config.clone()));
    }

    /// Try to send for a specific site.
    pub fn try_send(
        &mut self,
        site_id: u64,
        byte_count: u64,
        entry_count: u64,
        now_us: u64,
    ) -> bool {
        if let Some(throttle) = self.per_site.get_mut(&site_id) {
            throttle.try_send(byte_count, entry_count, now_us)
        } else {
            true
        }
    }

    /// Remove a site's throttle.
    pub fn remove_site(&mut self, site_id: u64) {
        self.per_site.remove(&site_id);
    }

    /// Update throttle config for a site.
    pub fn update_site_config(&mut self, site_id: u64, config: ThrottleConfig) {
        if let Some(throttle) = self.per_site.get_mut(&site_id) {
            throttle.update_config(config);
        }
    }

    /// Get available bytes for a site.
    pub fn available_bytes(&self, site_id: u64, now_us: u64) -> u64 {
        if let Some(throttle) = self.per_site.get(&site_id) {
            throttle.available_bytes(now_us)
        } else {
            u64::MAX
        }
    }
}

fn current_time_us() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_micros() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    mod token_bucket {
        use super::*;

        #[test]
        fn test_new() {
            let bucket = TokenBucket::new(1000, 100.0);
            assert_eq!(bucket.capacity, 1000);
        }

        #[test]
        fn test_try_consume_succeeds() {
            let mut bucket = TokenBucket::new(100, 1000.0);
            let now = current_time_us();
            assert!(bucket.try_consume(50, now));
        }

        #[test]
        fn test_try_consume_fails_not_enough() {
            let mut bucket = TokenBucket::new(100, 1000.0);
            let now = current_time_us();
            assert!(bucket.try_consume(50, now));
            assert!(!bucket.try_consume(60, now));
        }

        #[test]
        fn test_refill_over_time() {
            let mut bucket = TokenBucket::new(100, 1_000_000.0);
            let now = current_time_us();
            bucket.try_consume(100, now);
            assert_eq!(bucket.available(now), 0);
            let later = now + 1_000_000;
            assert!(bucket.available(later) >= 1);
        }

        #[test]
        fn test_available() {
            let bucket = TokenBucket::new(100, 1000.0);
            let now = current_time_us();
            assert_eq!(bucket.available(now), 100);
        }
    }

    mod site_throttle {
        use super::*;

        #[test]
        fn test_new() {
            let config = ThrottleConfig::default();
            let throttle = SiteThrottle::new(config);
            assert_eq!(throttle.max_bytes_per_sec(), 100 * 1024 * 1024);
        }

        #[test]
        fn test_try_send_success() {
            let mut throttle = SiteThrottle::new(ThrottleConfig::default());
            let now = current_time_us();
            assert!(throttle.try_send(1000, 10, now));
        }

        #[test]
        fn test_try_send_fails_on_bytes() {
            let mut config = ThrottleConfig::default();
            config.max_bytes_per_sec = 3;
            config.burst_factor = 1.0;
            let mut throttle = SiteThrottle::new(config);
            let now = current_time_us();
            assert!(throttle.try_send(2, 1, now));
            assert!(!throttle.try_send(2, 1, now));
        }

        #[test]
        fn test_try_send_fails_on_entries() {
            let mut config = ThrottleConfig::default();
            config.max_entries_per_sec = 3;
            config.burst_factor = 1.0;
            let mut throttle = SiteThrottle::new(config);
            let now = current_time_us();
            assert!(throttle.try_send(1, 2, now));
            assert!(!throttle.try_send(1, 2, now));
        }

        #[test]
        fn test_update_config() {
            let mut throttle = SiteThrottle::new(ThrottleConfig::default());
            let mut config = ThrottleConfig::default();
            config.max_bytes_per_sec = 50 * 1024 * 1024;
            throttle.update_config(config.clone());
            assert_eq!(throttle.max_bytes_per_sec(), 50 * 1024 * 1024);
        }
    }

    mod throttle_manager {
        use super::*;

        #[test]
        fn test_register() {
            let mut manager = ThrottleManager::new(ThrottleConfig::default());
            manager.register_site(1, ThrottleConfig::default());
            let now = current_time_us();
            assert!(manager.try_send(1, 1000, 10, now));
        }

        #[test]
        fn test_try_send() {
            let mut manager = ThrottleManager::new(ThrottleConfig::default());
            manager.register_site_default(1);
            let now = current_time_us();
            assert!(manager.try_send(1, 1000, 10, now));
        }

        #[test]
        fn test_remove_site() {
            let mut manager = ThrottleManager::new(ThrottleConfig::default());
            manager.register_site_default(1);
            manager.remove_site(1);
            let now = current_time_us();
            assert!(manager.try_send(1, 1000, 10, now));
        }

        #[test]
        fn test_update_site_config() {
            let mut manager = ThrottleManager::new(ThrottleConfig::default());
            manager.register_site_default(1);
            let mut config = ThrottleConfig::default();
            config.max_bytes_per_sec = 200 * 1024 * 1024;
            manager.update_site_config(1, config);
            let throttle = manager.per_site.get(&1).unwrap();
            assert_eq!(throttle.max_bytes_per_sec(), 200 * 1024 * 1024);
        }

        #[test]
        fn test_available_bytes() {
            let mut manager = ThrottleManager::new(ThrottleConfig::default());
            manager.register_site_default(1);
            let now = current_time_us();
            let bytes = manager.available_bytes(1, now);
            assert!(bytes > 0);
        }
    }

    mod unlimited_throttle {
        use super::*;

        #[test]
        fn test_zero_bytes_per_sec_unlimited() {
            let mut config = ThrottleConfig::default();
            config.max_bytes_per_sec = 0;
            let mut throttle = SiteThrottle::new(config);
            let now = current_time_us();
            assert!(throttle.try_send(u64::MAX, 1, now));
        }

        #[test]
        fn test_zero_entries_per_sec_unlimited() {
            let mut config = ThrottleConfig::default();
            config.max_entries_per_sec = 0;
            let mut throttle = SiteThrottle::new(config);
            let now = current_time_us();
            assert!(throttle.try_send(1, u64::MAX, now));
        }
    }

    mod burst_capacity {
        use super::*;

        #[test]
        fn test_burst_allows_short_burst() {
            let mut config = ThrottleConfig::default();
            config.max_bytes_per_sec = 100;
            config.burst_factor = 2.0;
            let mut throttle = SiteThrottle::new(config);
            let now = current_time_us();
            assert!(throttle.try_send(200, 1, now));
        }
    }

    mod zero_requests {
        use super::*;

        #[test]
        fn test_zero_byte_request_always_succeeds() {
            let mut config = ThrottleConfig::default();
            config.max_bytes_per_sec = 1;
            let mut throttle = SiteThrottle::new(config);
            let now = current_time_us();
            assert!(throttle.try_send(0, 1, now));
        }

        #[test]
        fn test_zero_entry_request_always_succeeds() {
            let mut config = ThrottleConfig::default();
            config.max_entries_per_sec = 1;
            let mut throttle = SiteThrottle::new(config);
            let now = current_time_us();
            assert!(throttle.try_send(1, 0, now));
        }
    }

    mod available_bytes_after_consumption {
        use super::*;

        #[test]
        fn test_available_bytes_decreases() {
            let mut config = ThrottleConfig::default();
            config.max_bytes_per_sec = 1000;
            config.burst_factor = 1.0;
            let mut throttle = SiteThrottle::new(config);
            let now = current_time_us();
            throttle.try_send(500, 1, now);
            let available = throttle.available_bytes(now);
            assert!(available <= 500);
        }
    }
}
