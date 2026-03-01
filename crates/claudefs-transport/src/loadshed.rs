//! Adaptive load shedding module for protecting servers from overload.
//!
//! Rejects requests when system metrics exceed configured thresholds to prevent
//! cascading failures and maintain service quality.

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, AtomicUsize, Ordering};

/// Configuration for adaptive load shedding.
#[derive(Debug, Clone)]
pub struct LoadShedConfig {
    /// P99 latency (in milliseconds) above which shedding activates.
    pub latency_threshold_ms: u64,
    /// Queue depth above which shedding activates.
    pub queue_depth_threshold: usize,
    /// CPU usage percentage above which shedding activates.
    pub cpu_threshold_pct: u8,
    /// Base probability of shedding when thresholds exceeded.
    pub shed_probability_base: f64,
    /// Time to wait before reducing shed rate after recovery (ms).
    pub recovery_window_ms: u64,
    /// Whether load shedding is enabled.
    pub enabled: bool,
}

impl LoadShedConfig {
    /// Creates a new LoadShedConfig with the specified values.
    pub fn new(
        latency_threshold_ms: u64,
        queue_depth_threshold: usize,
        cpu_threshold_pct: u8,
        shed_probability_base: f64,
        recovery_window_ms: u64,
        enabled: bool,
    ) -> Self {
        Self {
            latency_threshold_ms,
            queue_depth_threshold,
            cpu_threshold_pct,
            shed_probability_base,
            recovery_window_ms,
            enabled,
        }
    }
}

impl Default for LoadShedConfig {
    fn default() -> Self {
        Self {
            latency_threshold_ms: 500,
            queue_depth_threshold: 1000,
            cpu_threshold_pct: 85,
            shed_probability_base: 0.5,
            recovery_window_ms: 5000,
            enabled: true,
        }
    }
}

/// Statistics snapshot for load shedding state.
#[derive(Debug, Clone, Default)]
pub struct LoadShedStats {
    /// Total requests evaluated.
    pub total_requests: u64,
    /// Total requests shed (rejected).
    pub total_shed: u64,
    /// Total requests admitted.
    pub total_admitted: u64,
    /// Current shedding probability (0.0 to 1.0).
    pub current_shed_probability: f64,
    /// Whether currently overloaded.
    pub is_overloaded: bool,
    /// Current tracked P99 latency estimate (in ms).
    pub p99_latency_ms: u64,
    /// Current tracked queue depth.
    pub current_queue_depth: usize,
    /// Current tracked CPU percentage.
    pub current_cpu_pct: u8,
}

/// Adaptive load shedder that protects servers from overload.
pub struct LoadShedder {
    config: LoadShedConfig,
    total_requests: AtomicU64,
    total_shed: AtomicU64,
    total_admitted: AtomicU64,
    current_shed_probability: AtomicU64,
    p99_latency_estimate: AtomicU64,
    current_queue_depth: AtomicUsize,
    current_cpu_pct: AtomicU8,
    is_overloaded: AtomicBool,
    request_counter: AtomicU64,
    last_recovery_time: AtomicU64,
}

impl LoadShedder {
    /// Creates a new LoadShedder with the given configuration.
    pub fn new(config: LoadShedConfig) -> Self {
        Self {
            config,
            total_requests: AtomicU64::new(0),
            total_shed: AtomicU64::new(0),
            total_admitted: AtomicU64::new(0),
            current_shed_probability: AtomicU64::new(0),
            p99_latency_estimate: AtomicU64::new(0),
            current_queue_depth: AtomicUsize::new(0),
            current_cpu_pct: AtomicU8::new(0),
            is_overloaded: AtomicBool::new(false),
            request_counter: AtomicU64::new(0),
            last_recovery_time: AtomicU64::new(0),
        }
    }

    /// Returns true if the current request should be shed (rejected).
    pub fn should_shed(&self) -> bool {
        if !self.config.enabled {
            return false;
        }

        self.total_requests.fetch_add(1, Ordering::Relaxed);

        if !self.is_overloaded.load(Ordering::Relaxed) {
            self.total_admitted.fetch_add(1, Ordering::Relaxed);
            return false;
        }

        let probability = self.shed_probability();
        let counter = self.request_counter.fetch_add(1, Ordering::Relaxed);

        let threshold = (probability * 100.0) as u64;
        let should_shed = (counter % 100) < threshold;

        if should_shed {
            self.total_shed.fetch_add(1, Ordering::Relaxed);
        } else {
            self.total_admitted.fetch_add(1, Ordering::Relaxed);
        }

        should_shed
    }

    /// Records an observed request latency.
    pub fn record_latency(&self, latency_ms: u64) {
        let current = self.p99_latency_estimate.load(Ordering::Relaxed);
        let new_estimate = if current == 0 {
            latency_ms
        } else if latency_ms > current {
            let ratio = latency_ms as f64 / current as f64;
            if ratio > 2.0 {
                latency_ms
            } else {
                ((current as f64 * 0.9) + (latency_ms as f64 * 0.1)) as u64
            }
        } else {
            ((current as f64 * 0.99) + (latency_ms as f64 * 0.01)) as u64
        };
        self.p99_latency_estimate
            .store(new_estimate, Ordering::Relaxed);
        self.update_overloaded_state();
    }

    /// Records current queue depth.
    pub fn record_queue_depth(&self, depth: usize) {
        self.current_queue_depth.store(depth, Ordering::Relaxed);
        self.update_overloaded_state();
    }

    /// Records CPU usage percentage.
    pub fn record_cpu_usage(&self, pct: u8) {
        self.current_cpu_pct.store(pct, Ordering::Relaxed);
        self.update_overloaded_state();
    }

    /// Returns current shedding probability (0.0 to 1.0).
    pub fn shed_probability(&self) -> f64 {
        let stored = self.current_shed_probability.load(Ordering::Relaxed);
        (stored as f64) / 1000.0
    }

    /// Returns whether any threshold is currently exceeded.
    pub fn is_overloaded(&self) -> bool {
        self.is_overloaded.load(Ordering::Relaxed)
    }

    /// Returns statistics snapshot.
    pub fn stats(&self) -> LoadShedStats {
        LoadShedStats {
            total_requests: self.total_requests.load(Ordering::Relaxed),
            total_shed: self.total_shed.load(Ordering::Relaxed),
            total_admitted: self.total_admitted.load(Ordering::Relaxed),
            current_shed_probability: self.shed_probability(),
            is_overloaded: self.is_overloaded.load(Ordering::Relaxed),
            p99_latency_ms: self.p99_latency_estimate.load(Ordering::Relaxed),
            current_queue_depth: self.current_queue_depth.load(Ordering::Relaxed),
            current_cpu_pct: self.current_cpu_pct.load(Ordering::Relaxed),
        }
    }

    /// Resets all statistics and state.
    pub fn reset(&self) {
        self.total_requests.store(0, Ordering::Relaxed);
        self.total_shed.store(0, Ordering::Relaxed);
        self.total_admitted.store(0, Ordering::Relaxed);
        self.current_shed_probability.store(0, Ordering::Relaxed);
        self.p99_latency_estimate.store(0, Ordering::Relaxed);
        self.current_queue_depth.store(0, Ordering::Relaxed);
        self.current_cpu_pct.store(0, Ordering::Relaxed);
        self.is_overloaded.store(false, Ordering::Relaxed);
        self.request_counter.store(0, Ordering::Relaxed);
        self.last_recovery_time.store(0, Ordering::Relaxed);
    }

    fn update_overloaded_state(&self) {
        let latency = self.p99_latency_estimate.load(Ordering::Relaxed);
        let queue = self.current_queue_depth.load(Ordering::Relaxed);
        let cpu = self.current_cpu_pct.load(Ordering::Relaxed);

        let latency_exceeded = latency > self.config.latency_threshold_ms;
        let queue_exceeded = queue > self.config.queue_depth_threshold;
        let cpu_exceeded = cpu > self.config.cpu_threshold_pct;

        let overloaded = latency_exceeded || queue_exceeded || cpu_exceeded;
        self.is_overloaded.store(overloaded, Ordering::Relaxed);

        if overloaded {
            let probability = self.calculate_adaptive_probability(latency, queue, cpu);
            let stored_prob = ((probability.min(1.0) * 1000.0) as u64).min(1000);
            self.current_shed_probability
                .store(stored_prob, Ordering::Relaxed);
            self.last_recovery_time
                .store(Self::now_millis(), Ordering::Relaxed);
        } else {
            let last_recovery = self.last_recovery_time.load(Ordering::Relaxed);
            let now = Self::now_millis();
            if now.saturating_sub(last_recovery) > self.config.recovery_window_ms {
                let current = self.current_shed_probability.load(Ordering::Relaxed);
                let new_prob = current.saturating_sub(50);
                self.current_shed_probability
                    .store(new_prob, Ordering::Relaxed);
            }
        }
    }

    fn calculate_adaptive_probability(&self, latency: u64, queue: usize, cpu: u8) -> f64 {
        let mut multiplier = 1.0;

        if latency > self.config.latency_threshold_ms {
            let ratio = latency as f64 / self.config.latency_threshold_ms as f64;
            let latency_mult = 1.0 + ((ratio - 1.0) * 0.1).min(0.3);
            multiplier *= latency_mult;
        }

        if queue > self.config.queue_depth_threshold {
            let ratio = queue as f64 / self.config.queue_depth_threshold as f64;
            let queue_mult = 1.0 + ((ratio - 1.0) * 0.1).min(0.3);
            multiplier *= queue_mult;
        }

        if cpu > self.config.cpu_threshold_pct {
            let ratio = cpu as f64 / self.config.cpu_threshold_pct as f64;
            let cpu_mult = 1.0 + ((ratio - 1.0) * 0.1).min(0.3);
            multiplier *= cpu_mult;
        }

        let prob = self.config.shed_probability_base * multiplier;
        prob.min(1.0)
    }

    fn now_millis() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = LoadShedConfig::default();
        assert_eq!(config.latency_threshold_ms, 500);
        assert_eq!(config.queue_depth_threshold, 1000);
        assert_eq!(config.cpu_threshold_pct, 85);
        assert_eq!(config.shed_probability_base, 0.5);
        assert_eq!(config.recovery_window_ms, 5000);
        assert!(config.enabled);
    }

    #[test]
    fn test_new_load_shedder() {
        let config = LoadShedConfig::default();
        let shedder = LoadShedder::new(config.clone());
        let stats = shedder.stats();
        assert_eq!(stats.total_requests, 0);
        assert_eq!(stats.total_shed, 0);
        assert_eq!(stats.total_admitted, 0);
    }

    #[test]
    fn test_not_shedding_when_below_thresholds() {
        let shedder = LoadShedder::new(LoadShedConfig::default());

        shedder.record_latency(100);
        shedder.record_queue_depth(100);
        shedder.record_cpu_usage(30);

        let mut shed_count = 0;
        for _ in 0..100 {
            if shedder.should_shed() {
                shed_count += 1;
            }
        }

        assert_eq!(shed_count, 0);
    }

    #[test]
    fn test_shedding_when_latency_exceeds() {
        let shedder = LoadShedder::new(LoadShedConfig::default());

        shedder.record_latency(1000);
        shedder.record_queue_depth(100);
        shedder.record_cpu_usage(30);

        let stats = shedder.stats();
        assert!(stats.is_overloaded);

        let mut shed_count = 0;
        for _ in 0..1000 {
            if shedder.should_shed() {
                shed_count += 1;
            }
        }

        assert!(shed_count > 0);
    }

    #[test]
    fn test_shedding_when_queue_depth_exceeds() {
        let shedder = LoadShedder::new(LoadShedConfig::default());

        shedder.record_latency(100);
        shedder.record_queue_depth(2000);
        shedder.record_cpu_usage(30);

        let stats = shedder.stats();
        assert!(stats.is_overloaded);

        let mut shed_count = 0;
        for _ in 0..1000 {
            if shedder.should_shed() {
                shed_count += 1;
            }
        }

        assert!(shed_count > 0);
    }

    #[test]
    fn test_shedding_when_cpu_exceeds() {
        let shedder = LoadShedder::new(LoadShedConfig::default());

        shedder.record_latency(100);
        shedder.record_queue_depth(100);
        shedder.record_cpu_usage(95);

        let stats = shedder.stats();
        assert!(stats.is_overloaded);

        let mut shed_count = 0;
        for _ in 0..1000 {
            if shedder.should_shed() {
                shed_count += 1;
            }
        }

        assert!(shed_count > 0);
    }

    #[test]
    fn test_shed_probability_increases_with_load() {
        let shedder = LoadShedder::new(LoadShedConfig::default());

        shedder.record_latency(100);
        shedder.record_queue_depth(100);
        shedder.record_cpu_usage(30);
        let prob_low = shedder.shed_probability();

        shedder.record_latency(2000);
        shedder.record_queue_depth(5000);
        shedder.record_cpu_usage(95);

        let stats = shedder.stats();
        assert!(stats.current_shed_probability > prob_low);
    }

    #[test]
    fn test_is_overloaded_false_initially() {
        let shedder = LoadShedder::new(LoadShedConfig::default());
        assert!(!shedder.is_overloaded());
    }

    #[test]
    fn test_is_overloaded_true_when_threshold_exceeded() {
        let shedder = LoadShedder::new(LoadShedConfig::default());
        shedder.record_latency(600);

        assert!(shedder.is_overloaded());
    }

    #[test]
    fn test_stats_snapshot() {
        let shedder = LoadShedder::new(LoadShedConfig::default());

        shedder.record_latency(100);
        shedder.record_queue_depth(200);
        shedder.record_cpu_usage(50);

        for _ in 0..10 {
            shedder.should_shed();
        }

        let stats = shedder.stats();
        assert_eq!(stats.total_requests, 10);
        assert_eq!(stats.total_admitted + stats.total_shed, 10);
        assert_eq!(stats.p99_latency_ms, 100);
        assert_eq!(stats.current_queue_depth, 200);
        assert_eq!(stats.current_cpu_pct, 50);
    }

    #[test]
    fn test_reset_clears_state() {
        let shedder = LoadShedder::new(LoadShedConfig::default());

        shedder.record_latency(1000);
        shedder.record_queue_depth(2000);
        shedder.record_cpu_usage(95);

        for _ in 0..100 {
            shedder.should_shed();
        }

        shedder.reset();

        let stats = shedder.stats();
        assert_eq!(stats.total_requests, 0);
        assert_eq!(stats.total_shed, 0);
        assert_eq!(stats.total_admitted, 0);
        assert!(!stats.is_overloaded);
    }

    #[test]
    fn test_disabled_never_sheds() {
        let mut config = LoadShedConfig::default();
        config.enabled = false;

        let shedder = LoadShedder::new(config);

        shedder.record_latency(10000);
        shedder.record_queue_depth(10000);
        shedder.record_cpu_usage(100);

        let mut shed_count = 0;
        for _ in 0..1000 {
            if shedder.should_shed() {
                shed_count += 1;
            }
        }

        assert_eq!(shed_count, 0);
    }

    #[test]
    fn test_shed_probability_capped_at_one() {
        let shedder = LoadShedder::new(LoadShedConfig::default());

        shedder.record_latency(100000);
        shedder.record_queue_depth(100000);
        shedder.record_cpu_usage(100);

        let prob = shedder.shed_probability();
        assert!(prob <= 1.0);
    }

    #[test]
    fn test_multiple_signals_compound() {
        let shedder = LoadShedder::new(LoadShedConfig::default());

        shedder.record_latency(100);
        shedder.record_queue_depth(100);
        shedder.record_cpu_usage(30);
        let prob_latency_only = shedder.shed_probability();

        shedder.record_latency(2000);
        shedder.record_queue_depth(100);
        shedder.record_cpu_usage(30);
        let prob_latency_high = shedder.shed_probability();

        shedder.record_latency(2000);
        shedder.record_queue_depth(5000);
        shedder.record_cpu_usage(30);
        let prob_latency_queue = shedder.shed_probability();

        shedder.record_latency(2000);
        shedder.record_queue_depth(5000);
        shedder.record_cpu_usage(95);
        let prob_all = shedder.shed_probability();

        assert!(prob_latency_high > prob_latency_only);
        assert!(prob_latency_queue > prob_latency_high);
        assert!(prob_all > prob_latency_queue);
    }

    #[test]
    fn test_latency_tracking_weighted() {
        let shedder = LoadShedder::new(LoadShedConfig::default());

        shedder.record_latency(100);
        shedder.record_latency(100);
        shedder.record_latency(100);

        let stats = shedder.stats();
        assert_eq!(stats.p99_latency_ms, 100);

        shedder.record_latency(1000);

        let stats2 = shedder.stats();
        assert!(stats2.p99_latency_ms > 100);
    }

    #[test]
    fn test_admit_count_tracking() {
        let shedder = LoadShedder::new(LoadShedConfig::default());

        shedder.record_latency(100);
        shedder.record_queue_depth(100);
        shedder.record_cpu_usage(30);

        for _ in 0..50 {
            shedder.should_shed();
        }

        let stats = shedder.stats();
        assert_eq!(stats.total_admitted, 50);
    }
}
