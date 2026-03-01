//! Backpressure module for coordinating overload signals between servers and clients.
//!
//! This module provides a unified backpressure mechanism that integrates queue depth,
//! memory pressure, and throughput signals into a single score that clients can use
//! to throttle requests and prevent system overload.

use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Instant;

/// Backpressure signal level indicating system load.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum PressureLevel {
    /// System is healthy, no throttling needed.
    #[default]
    None,
    /// System is under moderate load, clients should slow down.
    Low,
    /// System is under heavy load, clients must throttle significantly.
    Medium,
    /// System is critically overloaded, clients should stop sending.
    High,
    /// System is refusing all new work.
    Critical,
}

impl PressureLevel {
    fn from_score(score: f64) -> Self {
        if score >= 0.8 {
            PressureLevel::Critical
        } else if score >= 0.6 {
            PressureLevel::High
        } else if score >= 0.4 {
            PressureLevel::Medium
        } else if score >= 0.2 {
            PressureLevel::Low
        } else {
            PressureLevel::None
        }
    }
}

/// A backpressure signal with score and metadata.
#[derive(Debug, Clone)]
pub struct BackpressureSignal {
    /// The pressure level derived from the score.
    pub level: PressureLevel,
    /// The backpressure score from 0.0 (none) to 1.0 (critical).
    pub score: f64,
    /// Current queue depth.
    pub queue_depth: usize,
    /// Memory usage as percentage (0-100).
    pub memory_used_pct: u8,
    /// Current throughput as percentage of capacity (0-100).
    pub throughput_pct: u8,
    /// Timestamp of signal generation in milliseconds.
    pub timestamp_ms: u64,
}

impl Default for BackpressureSignal {
    fn default() -> Self {
        Self {
            level: PressureLevel::None,
            score: 0.0,
            queue_depth: 0,
            memory_used_pct: 0,
            throughput_pct: 0,
            timestamp_ms: 0,
        }
    }
}

impl BackpressureSignal {
    /// Creates a new backpressure signal with the given values.
    pub fn new(
        level: PressureLevel,
        score: f64,
        queue_depth: usize,
        memory_used_pct: u8,
        throughput_pct: u8,
        timestamp_ms: u64,
    ) -> Self {
        Self {
            level,
            score,
            queue_depth,
            memory_used_pct,
            throughput_pct,
            timestamp_ms,
        }
    }
}

/// Configuration for backpressure monitoring.
#[derive(Debug, Clone)]
pub struct BackpressureConfig {
    /// Low watermark for queue depth (default 100).
    pub queue_low_watermark: usize,
    /// High watermark for queue depth (default 1000).
    pub queue_high_watermark: usize,
    /// Low watermark for memory usage percentage (default 60).
    pub memory_low_watermark_pct: u8,
    /// High watermark for memory usage percentage (default 85).
    pub memory_high_watermark_pct: u8,
    /// Low watermark for throughput percentage (default 70).
    pub throughput_low_watermark_pct: u8,
    /// High watermark for throughput percentage (default 90).
    pub throughput_high_watermark_pct: u8,
    /// How fast pressure decays over time (default 0.1).
    pub decay_rate: f64,
    /// Sample window size in milliseconds (default 1000).
    pub sample_window_ms: u64,
    /// Whether backpressure is enabled (default true).
    pub enabled: bool,
}

impl Default for BackpressureConfig {
    fn default() -> Self {
        Self {
            queue_low_watermark: 100,
            queue_high_watermark: 1000,
            memory_low_watermark_pct: 60,
            memory_high_watermark_pct: 85,
            throughput_low_watermark_pct: 70,
            throughput_high_watermark_pct: 90,
            decay_rate: 0.1,
            sample_window_ms: 1000,
            enabled: true,
        }
    }
}

/// Inner state for BackpressureMonitor that can be shared.
struct BackpressureMonitorInner {
    queue_depth: AtomicU64,
    memory_pct: AtomicU8,
    throughput_pct: AtomicU8,
    signals_generated: AtomicU64,
    throttle_events: AtomicU64,
    max_score_seen: AtomicU64,
    current_level: AtomicU8,
    last_update: AtomicU64,
}

/// Tracks current system pressure and generates signals.
pub struct BackpressureMonitor {
    config: BackpressureConfig,
    inner: Arc<BackpressureMonitorInner>,
    start_time: Instant,
}

impl Default for BackpressureMonitor {
    fn default() -> Self {
        Self::new(BackpressureConfig::default())
    }
}

impl std::fmt::Debug for BackpressureMonitor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BackpressureMonitor")
            .field("config", &self.config)
            .field("stats", &self.stats())
            .finish()
    }
}

impl BackpressureMonitor {
    /// Creates a new backpressure monitor with the given configuration.
    pub fn new(config: BackpressureConfig) -> Self {
        Self {
            config: config.clone(),
            inner: Arc::new(BackpressureMonitorInner {
                queue_depth: AtomicU64::new(0),
                memory_pct: AtomicU8::new(0),
                throughput_pct: AtomicU8::new(0),
                signals_generated: AtomicU64::new(0),
                throttle_events: AtomicU64::new(0),
                max_score_seen: AtomicU64::new(0),
                current_level: AtomicU8::new(0),
                last_update: AtomicU64::new(0),
            }),
            start_time: Instant::now(),
        }
    }

    /// Updates the current queue depth.
    pub fn update_queue_depth(&self, depth: usize) {
        self.inner
            .queue_depth
            .store(depth as u64, Ordering::Relaxed);
        self.inner.last_update.store(
            self.start_time.elapsed().as_millis() as u64,
            Ordering::Relaxed,
        );
    }

    /// Updates the current memory usage percentage.
    pub fn update_memory_usage(&self, pct: u8) {
        self.inner.memory_pct.store(pct, Ordering::Relaxed);
        self.inner.last_update.store(
            self.start_time.elapsed().as_millis() as u64,
            Ordering::Relaxed,
        );
    }

    /// Updates the current throughput as percentage of capacity.
    pub fn update_throughput(&self, pct: u8) {
        self.inner.throughput_pct.store(pct, Ordering::Relaxed);
        self.inner.last_update.store(
            self.start_time.elapsed().as_millis() as u64,
            Ordering::Relaxed,
        );
    }

    fn calculate_score(&self) -> f64 {
        let queue_depth = self.inner.queue_depth.load(Ordering::Acquire) as usize;
        let memory_pct = self.inner.memory_pct.load(Ordering::Acquire);
        let throughput_pct = self.inner.throughput_pct.load(Ordering::Acquire);

        let queue_component = self.normalize(
            queue_depth,
            self.config.queue_low_watermark,
            self.config.queue_high_watermark,
        ) * 0.4;

        let memory_component = self.normalize(
            memory_pct as usize,
            self.config.memory_low_watermark_pct as usize,
            self.config.memory_high_watermark_pct as usize,
        ) * 0.35;

        let throughput_component = self.normalize(
            throughput_pct as usize,
            self.config.throughput_low_watermark_pct as usize,
            self.config.throughput_high_watermark_pct as usize,
        ) * 0.25;

        let score = queue_component + memory_component + throughput_component;
        self.clamp(score, 0.0, 1.0)
    }

    fn normalize(&self, value: usize, low: usize, high: usize) -> f64 {
        if value <= low {
            0.0
        } else if value >= high {
            1.0
        } else if high == low {
            0.0
        } else {
            (value - low) as f64 / (high - low) as f64
        }
    }

    fn clamp(&self, value: f64, min: f64, max: f64) -> f64 {
        if value < min {
            min
        } else if value > max {
            max
        } else {
            value
        }
    }

    /// Returns the current backpressure signal.
    pub fn current_signal(&self) -> BackpressureSignal {
        if !self.config.enabled {
            return BackpressureSignal::default();
        }

        let score = self.calculate_score();
        let level = PressureLevel::from_score(score);

        self.inner.signals_generated.fetch_add(1, Ordering::Relaxed);

        let prev_level = self.inner.current_level.load(Ordering::Acquire);
        if level as u8 > prev_level {
            self.inner.throttle_events.fetch_add(1, Ordering::Relaxed);
        }
        self.inner
            .current_level
            .store(level as u8, Ordering::Release);

        let current_max = self.inner.max_score_seen.load(Ordering::Acquire) as f64;
        if score > current_max {
            self.inner
                .max_score_seen
                .store(score as u64, Ordering::Release);
        }

        BackpressureSignal::new(
            level,
            score,
            self.inner.queue_depth.load(Ordering::Acquire) as usize,
            self.inner.memory_pct.load(Ordering::Acquire),
            self.inner.throughput_pct.load(Ordering::Acquire),
            self.start_time.elapsed().as_millis() as u64,
        )
    }

    /// Returns the current pressure level.
    pub fn current_level(&self) -> PressureLevel {
        if !self.config.enabled {
            return PressureLevel::None;
        }
        PressureLevel::from_score(self.calculate_score())
    }

    /// Returns the current backpressure score (0.0 to 1.0).
    pub fn current_score(&self) -> f64 {
        if !self.config.enabled {
            return 0.0;
        }
        self.calculate_score()
    }

    /// Returns true if the system is overloaded (level >= Medium).
    pub fn is_overloaded(&self) -> bool {
        if !self.config.enabled {
            return false;
        }
        let level = self.current_level();
        level >= PressureLevel::Medium
    }

    /// Returns a snapshot of the current statistics.
    pub fn stats(&self) -> BackpressureStatsSnapshot {
        BackpressureStatsSnapshot {
            signals_generated: self.inner.signals_generated.load(Ordering::Acquire),
            throttle_events: self.inner.throttle_events.load(Ordering::Acquire),
            max_score_seen: self.inner.max_score_seen.load(Ordering::Acquire) as f64,
            current_level: self.current_level(),
            current_score: self.current_score(),
        }
    }
}

/// Statistics tracking for backpressure monitoring.
pub struct BackpressureStats {
    _private: (),
}

impl Default for BackpressureStats {
    fn default() -> Self {
        Self::new()
    }
}

impl BackpressureStats {
    /// Creates a new backpressure stats instance.
    pub fn new() -> Self {
        Self { _private: () }
    }
}

/// Snapshot of backpressure statistics.
#[derive(Debug, Clone)]
pub struct BackpressureStatsSnapshot {
    /// Total number of signals generated.
    pub signals_generated: u64,
    /// Number of times throttle was engaged.
    pub throttle_events: u64,
    /// Maximum score ever seen.
    pub max_score_seen: f64,
    /// Current pressure level.
    pub current_level: PressureLevel,
    /// Current backpressure score.
    pub current_score: f64,
}

/// Client-side throttle that reacts to backpressure signals.
pub struct BackpressureThrottle {
    config: ThrottleConfig,
    current_rate: std::sync::atomic::AtomicU64,
    last_signal: std::sync::atomic::AtomicU8,
}

impl Default for BackpressureThrottle {
    fn default() -> Self {
        Self::new(ThrottleConfig::default())
    }
}

impl std::fmt::Debug for BackpressureThrottle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BackpressureThrottle")
            .field("config", &self.config)
            .field("current_rate", &self.current_rate.load(Ordering::Acquire))
            .finish()
    }
}

impl BackpressureThrottle {
    /// Creates a new backpressure throttle with the given configuration.
    pub fn new(config: ThrottleConfig) -> Self {
        Self {
            config,
            current_rate: std::sync::atomic::AtomicU64::new(config.initial_send_rate as u64),
            last_signal: std::sync::atomic::AtomicU8::new(0),
        }
    }

    /// Applies a backpressure signal to adjust the send rate.
    pub fn apply_signal(&self, signal: &BackpressureSignal) {
        let current_rate = self.current_rate.load(Ordering::Acquire);

        let new_rate = match signal.level {
            PressureLevel::None => {
                let increased = current_rate as f64 + self.config.increase_step;
                increased.min(self.config.max_send_rate)
            }
            PressureLevel::Low => current_rate as f64,
            PressureLevel::Medium => current_rate as f64 * self.config.decrease_ratio,
            PressureLevel::High => {
                current_rate as f64 * self.config.decrease_ratio * self.config.decrease_ratio
            }
            PressureLevel::Critical => self.config.min_send_rate,
        };

        let new_rate = new_rate
            .max(self.config.min_send_rate)
            .min(self.config.max_send_rate);
        self.current_rate.store(new_rate as u64, Ordering::Release);
        self.last_signal
            .store(signal.level as u8, Ordering::Release);
    }

    /// Returns whether a request should be sent based on current rate.
    /// This uses a simple probabilistic approach based on current rate.
    pub fn should_send(&self) -> bool {
        let rate = self.current_rate.load(Ordering::Acquire) as f64;
        if rate >= self.config.max_send_rate {
            return true;
        }
        if rate <= self.config.min_send_rate {
            return false;
        }
        let probability = rate / self.config.max_send_rate;
        let random_val = rand_simple() % 1000;
        (random_val as f64 / 1000.0) < probability
    }

    /// Returns the current send rate in requests per second.
    pub fn current_rate(&self) -> f64 {
        self.current_rate.load(Ordering::Acquire) as f64
    }

    /// Resets the throttle to initial send rate.
    pub fn reset(&self) {
        self.current_rate
            .store(self.config.initial_send_rate as u64, Ordering::Release);
        self.last_signal.store(0, Ordering::Release);
    }
}

fn rand_simple() -> u32 {
    use std::time::SystemTime;
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    nanos.wrapping_mul(1103515245).wrapping_add(12345)
}

/// Configuration for the backpressure throttle.
#[derive(Debug, Clone, Copy)]
pub struct ThrottleConfig {
    /// Minimum requests per second (default 10.0).
    pub min_send_rate: f64,
    /// Maximum requests per second (default 10000.0).
    pub max_send_rate: f64,
    /// Initial requests per second (default 1000.0).
    pub initial_send_rate: f64,
    /// Multiply rate by this on pressure (default 0.5).
    pub decrease_ratio: f64,
    /// Add this to rate on recovery (default 100.0).
    pub increase_step: f64,
}

impl Default for ThrottleConfig {
    fn default() -> Self {
        Self {
            min_send_rate: 10.0,
            max_send_rate: 10000.0,
            initial_send_rate: 1000.0,
            decrease_ratio: 0.5,
            increase_step: 100.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = BackpressureConfig::default();
        assert_eq!(config.queue_low_watermark, 100);
        assert_eq!(config.queue_high_watermark, 1000);
        assert_eq!(config.memory_low_watermark_pct, 60);
        assert_eq!(config.memory_high_watermark_pct, 85);
        assert_eq!(config.throughput_low_watermark_pct, 70);
        assert_eq!(config.throughput_high_watermark_pct, 90);
        assert_eq!(config.decay_rate, 0.1);
        assert_eq!(config.sample_window_ms, 1000);
        assert!(config.enabled);
    }

    #[test]
    fn test_pressure_level_ordering() {
        assert!(PressureLevel::None < PressureLevel::Low);
        assert!(PressureLevel::Low < PressureLevel::Medium);
        assert!(PressureLevel::Medium < PressureLevel::High);
        assert!(PressureLevel::High < PressureLevel::Critical);
        assert!(PressureLevel::None <= PressureLevel::None);
        assert!(PressureLevel::Critical >= PressureLevel::Critical);
    }

    #[test]
    fn test_monitor_initial_state() {
        let config = BackpressureConfig::default();
        let monitor = BackpressureMonitor::new(config);

        assert_eq!(monitor.current_level(), PressureLevel::None);
        assert!((monitor.current_score() - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_monitor_queue_pressure() {
        let config = BackpressureConfig::default();
        let monitor = BackpressureMonitor::new(config);

        monitor.update_queue_depth(200);
        let signal = monitor.current_signal();
        assert!(signal.score > 0.0);
    }

    #[test]
    fn test_monitor_memory_pressure() {
        let config = BackpressureConfig::default();
        let monitor = BackpressureMonitor::new(config);

        monitor.update_memory_usage(70);
        let signal = monitor.current_signal();
        assert!(signal.score > 0.0);
    }

    #[test]
    fn test_monitor_throughput_pressure() {
        let config = BackpressureConfig::default();
        let monitor = BackpressureMonitor::new(config);

        monitor.update_throughput(80);
        let signal = monitor.current_signal();
        assert!(signal.score > 0.0);
    }

    #[test]
    fn test_monitor_combined_pressure() {
        let config = BackpressureConfig::default();
        let monitor = BackpressureMonitor::new(config);

        monitor.update_queue_depth(500);
        monitor.update_memory_usage(70);
        monitor.update_throughput(80);

        let signal = monitor.current_signal();
        assert!(signal.score > 0.0);
        assert_eq!(signal.queue_depth, 500);
        assert_eq!(signal.memory_used_pct, 70);
        assert_eq!(signal.throughput_pct, 80);
    }

    #[test]
    fn test_monitor_level_none() {
        let config = BackpressureConfig::default();
        let monitor = BackpressureMonitor::new(config);

        monitor.update_queue_depth(10);
        monitor.update_memory_usage(10);
        monitor.update_throughput(10);

        let level = monitor.current_level();
        assert_eq!(level, PressureLevel::None);
    }

    #[test]
    fn test_monitor_level_low() {
        let config = BackpressureConfig::default();
        let monitor = BackpressureMonitor::new(config);

        monitor.update_queue_depth(300);
        monitor.update_memory_usage(65);
        monitor.update_throughput(75);

        let level = monitor.current_level();
        assert!(level >= PressureLevel::Low);
    }

    #[test]
    fn test_monitor_level_medium() {
        let config = BackpressureConfig::default();
        let monitor = BackpressureMonitor::new(config);

        monitor.update_queue_depth(500);
        monitor.update_memory_usage(70);
        monitor.update_throughput(80);

        let level = monitor.current_level();
        assert!(level >= PressureLevel::Medium);
    }

    #[test]
    fn test_monitor_level_high() {
        let config = BackpressureConfig::default();
        let monitor = BackpressureMonitor::new(config);

        monitor.update_queue_depth(800);
        monitor.update_memory_usage(80);
        monitor.update_throughput(85);

        let level = monitor.current_level();
        assert!(level >= PressureLevel::High);
    }

    #[test]
    fn test_monitor_level_critical() {
        let config = BackpressureConfig::default();
        let monitor = BackpressureMonitor::new(config);

        monitor.update_queue_depth(1000);
        monitor.update_memory_usage(90);
        monitor.update_throughput(95);

        let level = monitor.current_level();
        assert_eq!(level, PressureLevel::Critical);
    }

    #[test]
    fn test_monitor_is_overloaded() {
        let config = BackpressureConfig::default();
        let monitor = BackpressureMonitor::new(config);

        monitor.update_queue_depth(50);
        assert!(!monitor.is_overloaded());

        monitor.update_queue_depth(1000);
        assert!(monitor.is_overloaded());
    }

    #[test]
    fn test_monitor_stats() {
        let config = BackpressureConfig::default();
        let monitor = BackpressureMonitor::new(config);

        monitor.update_queue_depth(100);
        let _signal = monitor.current_signal();
        let _signal2 = monitor.current_signal();

        let stats = monitor.stats();
        assert!(stats.signals_generated > 0);
        assert_eq!(stats.current_level, monitor.current_level());
    }

    #[test]
    fn test_throttle_initial_rate() {
        let config = ThrottleConfig::default();
        let throttle = BackpressureThrottle::new(config);

        assert_eq!(throttle.current_rate(), config.initial_send_rate);
    }

    #[test]
    fn test_throttle_decrease_on_pressure() {
        let config = ThrottleConfig::default();
        let throttle = BackpressureThrottle::new(config);

        let initial_rate = throttle.current_rate();

        let signal = BackpressureSignal::new(PressureLevel::High, 0.7, 800, 80, 85, 1000);
        throttle.apply_signal(&signal);

        let new_rate = throttle.current_rate();
        assert!(new_rate < initial_rate);
    }

    #[test]
    fn test_throttle_increase_on_recovery() {
        let config = ThrottleConfig::default();
        let throttle = BackpressureThrottle::new(config);

        let signal = BackpressureSignal::new(PressureLevel::High, 0.7, 800, 80, 85, 1000);
        throttle.apply_signal(&signal);
        let after_pressure = throttle.current_rate();

        let recovery_signal = BackpressureSignal::new(PressureLevel::None, 0.0, 10, 10, 10, 2000);
        throttle.apply_signal(&recovery_signal);

        let after_recovery = throttle.current_rate();
        assert!(after_recovery > after_pressure);
    }

    #[test]
    fn test_throttle_min_rate_floor() {
        let config = ThrottleConfig::default();
        let throttle = BackpressureThrottle::new(config);

        let signal = BackpressureSignal::new(PressureLevel::Critical, 0.95, 2000, 99, 100, 1000);
        throttle.apply_signal(&signal);

        let rate = throttle.current_rate();
        assert!(rate >= config.min_send_rate);
    }

    #[test]
    fn test_throttle_max_rate_ceiling() {
        let config = ThrottleConfig::default();
        let throttle = BackpressureThrottle::new(config);

        for _ in 0..1000 {
            let signal = BackpressureSignal::new(PressureLevel::None, 0.0, 0, 0, 0, 1000);
            throttle.apply_signal(&signal);
        }

        let rate = throttle.current_rate();
        assert!(rate <= config.max_send_rate);
    }

    #[test]
    fn test_throttle_reset() {
        let config = ThrottleConfig::default();
        let throttle = BackpressureThrottle::new(config);

        let signal = BackpressureSignal::new(PressureLevel::High, 0.7, 800, 80, 85, 1000);
        throttle.apply_signal(&signal);

        throttle.reset();

        assert_eq!(throttle.current_rate(), config.initial_send_rate);
    }

    #[test]
    fn test_signal_construction() {
        let signal = BackpressureSignal::new(PressureLevel::Medium, 0.45, 500, 70, 80, 12345);

        assert_eq!(signal.level, PressureLevel::Medium);
        assert!((signal.score - 0.45).abs() < 0.001);
        assert_eq!(signal.queue_depth, 500);
        assert_eq!(signal.memory_used_pct, 70);
        assert_eq!(signal.throughput_pct, 80);
        assert_eq!(signal.timestamp_ms, 12345);
    }

    #[test]
    fn test_disabled_monitor() {
        let mut config = BackpressureConfig::default();
        config.enabled = false;
        let monitor = BackpressureMonitor::new(config);

        monitor.update_queue_depth(2000);
        monitor.update_memory_usage(99);
        monitor.update_throughput(100);

        assert_eq!(monitor.current_level(), PressureLevel::None);
        assert!((monitor.current_score() - 0.0).abs() < 0.001);
        assert!(!monitor.is_overloaded());
    }
}
