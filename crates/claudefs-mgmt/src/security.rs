use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub fn constant_time_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.as_bytes()
        .iter()
        .zip(b.as_bytes())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

struct RateLimitEntry {
    failures: u32,
    first_failure: Instant,
    last_failure: Instant,
    locked_until: Option<Instant>,
}

pub struct AuthRateLimiter {
    inner: Mutex<HashMap<String, RateLimitEntry>>,
}

impl AuthRateLimiter {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }

    pub fn record_failure(&self, ip: &str) -> bool {
        let now = Instant::now();
        let mut inner = self.inner.lock().unwrap();
        let entry = inner.entry(ip.to_string()).or_insert(RateLimitEntry {
            failures: 0,
            first_failure: now,
            last_failure: now,
            locked_until: None,
        });
        entry.failures += 1;
        entry.last_failure = now;
        if entry.first_failure + Duration::from_secs(60) < now {
            entry.failures = 1;
            entry.first_failure = now;
        }
        if entry.failures >= 5 {
            entry.locked_until = Some(now + Duration::from_secs(60));
        }
        entry.locked_until.is_some()
    }

    pub fn is_rate_limited(&self, ip: &str) -> bool {
        let inner = self.inner.lock().unwrap();
        if let Some(entry) = inner.get(ip) {
            if let Some(locked_until) = entry.locked_until {
                return locked_until > Instant::now();
            }
        }
        false
    }

    pub fn prune(&self) {
        let now = Instant::now();
        let mut inner = self.inner.lock().unwrap();
        inner.retain(|_, entry| entry.first_failure + Duration::from_secs(120) > now);
    }
}

impl Default for AuthRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

pub async fn security_headers_middleware(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let mut response = next.run(request).await;
    response.headers_mut().insert(
        axum::http::header::HeaderName::from_static("x-content-type-options"),
        axum::http::HeaderValue::from_static("nosniff"),
    );
    response.headers_mut().insert(
        axum::http::header::HeaderName::from_static("x-frame-options"),
        axum::http::HeaderValue::from_static("DENY"),
    );
    response.headers_mut().insert(
        axum::http::header::HeaderName::from_static("x-xss-protection"),
        axum::http::HeaderValue::from_static("1; mode=block"),
    );
    response.headers_mut().insert(
        axum::http::header::HeaderName::from_static("strict-transport-security"),
        axum::http::HeaderValue::from_static("max-age=31536000; includeSubDomains"),
    );
    response.headers_mut().insert(
        axum::http::header::HeaderName::from_static("cache-control"),
        axum::http::HeaderValue::from_static("no-store"),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_time_eq_equal_strings() {
        assert!(constant_time_eq("hello", "hello"));
    }

    #[test]
    fn constant_time_eq_different_strings() {
        assert!(!constant_time_eq("hello", "world"));
    }

    #[test]
    fn constant_time_eq_different_lengths() {
        assert!(!constant_time_eq("hello", "hello world"));
        assert!(!constant_time_eq("hello world", "hello"));
    }

    #[test]
    fn constant_time_eq_empty_strings() {
        assert!(constant_time_eq("", ""));
    }

    #[test]
    fn constant_time_eq_same_length_different_content() {
        assert!(!constant_time_eq("abcd", "abce"));
    }

    #[test]
    fn constant_time_eq_single_char_different() {
        assert!(!constant_time_eq("a", "b"));
    }

    #[test]
    fn constant_time_eq_single_char_same() {
        assert!(constant_time_eq("a", "a"));
    }

    #[test]
    fn rate_limiter_new() {
        let limiter = AuthRateLimiter::new();
        assert!(!limiter.is_rate_limited("192.168.1.1"));
    }

    #[test]
    fn rate_limiter_single_failure_not_limited() {
        let limiter = AuthRateLimiter::new();
        let result = limiter.record_failure("192.168.1.1");
        assert!(!result);
        assert!(!limiter.is_rate_limited("192.168.1.1"));
    }

    #[test]
    fn rate_limiter_five_failures_causes_limit() {
        let limiter = AuthRateLimiter::new();
        for _ in 0..4 {
            assert!(!limiter.record_failure("192.168.1.1"));
        }
        let result = limiter.record_failure("192.168.1.1");
        assert!(result);
        assert!(limiter.is_rate_limited("192.168.1.1"));
    }

    #[test]
    fn rate_limiter_different_ips_independent() {
        let limiter = AuthRateLimiter::new();
        for _ in 0..5 {
            limiter.record_failure("192.168.1.1");
        }
        assert!(limiter.is_rate_limited("192.168.1.1"));
        assert!(!limiter.is_rate_limited("192.168.1.2"));
    }

    #[test]
    fn rate_limiter_prune_removes_old_entries() {
        let limiter = AuthRateLimiter::new();
        limiter.record_failure("192.168.1.1");
        limiter.prune();
        assert!(!limiter.is_rate_limited("192.168.1.1"));
    }

    #[test]
    fn rate_limiter_window_reset() {
        let limiter = AuthRateLimiter::new();
        for _ in 0..4 {
            limiter.record_failure("192.168.1.1");
        }
        std::thread::sleep(Duration::from_millis(10));
        for _ in 0..5 {
            limiter.record_failure("192.168.1.1");
        }
        assert!(limiter.is_rate_limited("192.168.1.1"));
    }

    #[test]
    fn rate_limiter_after_lock_expires() {
        let limiter = AuthRateLimiter::new();
        for _ in 0..5 {
            limiter.record_failure("192.168.1.1");
        }
        assert!(limiter.is_rate_limited("192.168.1.1"));
    }

    #[test]
    fn rate_limiter_record_failure_returns_true_when_locked() {
        let limiter = AuthRateLimiter::new();
        for _ in 0..5 {
            limiter.record_failure("10.0.0.1");
        }
        let is_limited = limiter.is_rate_limited("10.0.0.1");
        assert!(is_limited);
    }
}