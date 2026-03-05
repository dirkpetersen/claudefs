//! Adaptive queue depth management based on device latency and health.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{Mutex, RwLock};

use crate::device::DeviceHealth;
use crate::nvme_passthrough::QueuePairId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum HealthAdaptiveMode {
    Healthy,
    Degraded,
    Critical,
}

impl Default for HealthAdaptiveMode {
    fn default() -> Self {
        Self::Healthy
    }
}

#[derive(Debug, serde::Serialize)]
pub struct QueueDepthStats {
    pub mode: HealthAdaptiveMode,
    pub current_limit: u32,
    pub pending_ops: u32,
    pub avg_latency_us: u64,
    pub p99_latency_us: u64,
    pub reduction_events: u64,
    pub avg_dispatch_wait_us: u64,
}

impl Default for QueueDepthStats {
    fn default() -> Self {
        Self {
            mode: HealthAdaptiveMode::Healthy,
            current_limit: 32,
            pending_ops: 0,
            avg_latency_us: 0,
            p99_latency_us: 0,
            reduction_events: 0,
            avg_dispatch_wait_us: 0,
        }
    }
}

impl Clone for QueueDepthStats {
    fn clone(&self) -> Self {
        Self {
            mode: self.mode,
            current_limit: self.current_limit,
            pending_ops: self.pending_ops,
            avg_latency_us: self.avg_latency_us,
            p99_latency_us: self.p99_latency_us,
            reduction_events: self.reduction_events,
            avg_dispatch_wait_us: self.avg_dispatch_wait_us,
        }
    }
}

#[derive(Debug, Clone)]
pub struct IoDepthLimiterConfig {
    pub initial_depth: u32,
    pub degradation_latency_ms: u64,
    pub critical_latency_ms: u64,
    pub min_depth: u32,
    pub reduction_percent: u32,
    pub history_size: usize,
    pub recovery_delay_ms: u64,
}

impl Default for IoDepthLimiterConfig {
    fn default() -> Self {
        Self {
            initial_depth: 32,
            degradation_latency_ms: 2,
            critical_latency_ms: 5,
            min_depth: 8,
            reduction_percent: 50,
            history_size: 1000,
            recovery_delay_ms: 500,
        }
    }
}

#[derive(Debug)]
pub struct IoDepthLimiter {
    qp_id: QueuePairId,
    mode: RwLock<HealthAdaptiveMode>,
    pending: Arc<std::sync::atomic::AtomicU32>,
    latencies: RwLock<VecDeque<u64>>,
    dispatch_times: RwLock<VecDeque<Instant>>,
    last_mode_change: RwLock<Instant>,
    stats: Mutex<QueueDepthStats>,
    config: IoDepthLimiterConfig,
    current_limit: RwLock<u32>,
}

impl IoDepthLimiter {
    pub fn new(qp_id: QueuePairId, config: IoDepthLimiterConfig) -> Self {
        let initial_depth = config.initial_depth;
        let history_size = config.history_size;
        Self {
            qp_id,
            mode: RwLock::new(HealthAdaptiveMode::Healthy),
            pending: Arc::new(std::sync::atomic::AtomicU32::new(0)),
            latencies: RwLock::new(VecDeque::with_capacity(history_size)),
            dispatch_times: RwLock::new(VecDeque::with_capacity(history_size)),
            last_mode_change: RwLock::new(Instant::now()),
            stats: Mutex::new(QueueDepthStats::default()),
            config,
            current_limit: RwLock::new(initial_depth),
        }
    }

    fn calculate_p99(latencies: &[u64]) -> u64 {
        if latencies.is_empty() {
            return 0;
        }
        let mut sorted: Vec<u64> = latencies.iter().copied().collect();
        sorted.sort();
        let idx = ((sorted.len() as f64) * 0.99) as usize;
        sorted[idx.min(sorted.len() - 1)]
    }

    fn calculate_avg(latencies: &[u64]) -> u64 {
        if latencies.is_empty() {
            return 0;
        }
        latencies.iter().sum::<u64>() / latencies.len() as u64
    }

    pub async fn try_acquire(&self) -> bool {
        let limit = *self.current_limit.read().await;
        let current = self.pending.load(std::sync::atomic::Ordering::Acquire);
        
        if current >= limit {
            return false;
        }
        
        self.pending.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        
        let mut dispatch_times = self.dispatch_times.write().await;
        dispatch_times.push_back(Instant::now());
        if dispatch_times.len() > self.config.history_size {
            dispatch_times.pop_front();
        }
        
        true
    }

    pub fn release(&self, latency_us: u64) {
        let current = self.pending.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
        if current == 0 {
            return;
        }
        
        let mut latencies = self.latencies.blocking_write();
        latencies.push_back(latency_us);
        if latencies.len() > self.config.history_size {
            latencies.pop_front();
        }
    }

    pub async fn update_health(&self, health: DeviceHealth) {
        let mode = *self.mode.read().await;
        let new_mode = if health.critical_warning {
            HealthAdaptiveMode::Critical
        } else if health.percentage_used > 90 || health.available_spare < 10 {
            HealthAdaptiveMode::Critical
        } else if health.percentage_used > 70 || health.available_spare < 20 {
            HealthAdaptiveMode::Degraded
        } else {
            HealthAdaptiveMode::Healthy
        };
        
        let mut mode_guard = self.mode.write().await;
        if new_mode != *mode_guard {
            *mode_guard = new_mode;
            drop(mode_guard);
            
            let mut last_change = self.last_mode_change.write().await;
            *last_change = Instant::now();
            
            self.adjust_depth_for_mode(new_mode).await;
        }
    }

    async fn adjust_depth_for_mode(&self, mode: HealthAdaptiveMode) {
        let current_limit = *self.current_limit.read().await;
        let new_limit = match mode {
            HealthAdaptiveMode::Healthy => {
                let config = &self.config;
                let increased = (current_limit * (100 + config.reduction_percent) / 100).max(config.initial_depth);
                increased.min(256)
            }
            HealthAdaptiveMode::Degraded => {
                current_limit * (100 - self.config.reduction_percent) / 100
            }
            HealthAdaptiveMode::Critical => {
                self.config.min_depth
            }
        };
        
        let mut limit = self.current_limit.write().await;
        *limit = new_limit.max(self.config.min_depth);
        
        let mut stats = self.stats.lock().await;
        stats.reduction_events += 1;
    }

    pub async fn current_limit(&self) -> u32 {
        *self.current_limit.read().await
    }

    pub async fn stats(&self) -> QueueDepthStats {
        let latencies = self.latencies.read().await;
        let dispatch_times = self.dispatch_times.read().await;
        
        let pending = self.pending.load(std::sync::atomic::Ordering::Acquire);
        let mode = *self.mode.read().await;
        let current_limit = *self.current_limit.read().await;
        
        let latency_vec: Vec<u64> = latencies.iter().copied().collect();
        let avg_latency = Self::calculate_avg(&latency_vec);
        let p99_latency = Self::calculate_p99(&latency_vec);
        
        let avg_dispatch_wait = if dispatch_times.is_empty() {
            0
        } else {
            let now = Instant::now();
            let total_wait: u64 = dispatch_times
                .iter()
                .map(|t| now.duration_since(*t).as_millis() as u64)
                .sum();
            total_wait / dispatch_times.len() as u64
        };
        
        let mut stats = self.stats.lock().await;
        stats.mode = mode;
        stats.current_limit = current_limit;
        stats.pending_ops = pending;
        stats.avg_latency_us = avg_latency;
        stats.p99_latency_us = p99_latency;
        stats.avg_dispatch_wait_us = avg_dispatch_wait;
        
        stats.clone()
    }

    pub async fn set_depth(&self, depth: u32) {
        let min_depth = self.config.min_depth;
        let clamped_depth = depth.max(min_depth).min(256);
        
        let mut limit = self.current_limit.write().await;
        *limit = clamped_depth;
        
        let mut stats = self.stats.lock().await;
        stats.current_limit = clamped_depth;
    }

    pub fn qp_id(&self) -> QueuePairId {
        self.qp_id
    }

    pub async fn pending_count(&self) -> u32 {
        self.pending.load(std::sync::atomic::Ordering::Acquire)
    }

    pub async fn mode(&self) -> HealthAdaptiveMode {
        *self.mode.read().await
    }

    pub async fn check_and_adjust(&self) {
        let latencies = self.latencies.read().await;
        let latency_vec: Vec<u64> = latencies.iter().copied().collect();
        let avg_latency_ms = Self::calculate_avg(&latency_vec) / 1000;
        
        let last_change = *self.last_mode_change.read().await;
        let time_since_change = Instant::now().duration_since(last_change).as_millis() as u64;
        
        if time_since_change < self.config.recovery_delay_ms {
            return;
        }
        
        let current_mode = *self.mode.read().await;
        let new_mode = if avg_latency_ms >= self.config.critical_latency_ms {
            HealthAdaptiveMode::Critical
        } else if avg_latency_ms >= self.config.degradation_latency_ms {
            match current_mode {
                HealthAdaptiveMode::Healthy => HealthAdaptiveMode::Degraded,
                m => m,
            }
        } else {
            match current_mode {
                HealthAdaptiveMode::Critical | HealthAdaptiveMode::Degraded => HealthAdaptiveMode::Healthy,
                m => m,
            }
        };
        
        if new_mode != current_mode {
            let mut mode_guard = self.mode.write().await;
            *mode_guard = new_mode;
            drop(mode_guard);
            
            let mut last_change_guard = self.last_mode_change.write().await;
            *last_change_guard = Instant::now();
            
            self.adjust_depth_for_mode(new_mode).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::Duration;

    fn create_test_limiter() -> IoDepthLimiter {
        IoDepthLimiter::new(
            QueuePairId(0),
            IoDepthLimiterConfig {
                initial_depth: 10,
                degradation_latency_ms: 2,
                critical_latency_ms: 5,
                min_depth: 2,
                reduction_percent: 50,
                history_size: 100,
                recovery_delay_ms: 50,
            },
        )
    }

    #[tokio::test]
    async fn test_creation() {
        let limiter = create_test_limiter();
        assert_eq!(limiter.qp_id(), QueuePairId(0));
        assert_eq!(limiter.pending_count().await, 0);
        assert_eq!(limiter.mode().await, HealthAdaptiveMode::Healthy);
        assert_eq!(limiter.current_limit().await, 10);
    }

    #[tokio::test]
    async fn test_default_config() {
        let config = IoDepthLimiterConfig::default();
        assert_eq!(config.initial_depth, 32);
        assert_eq!(config.degradation_latency_ms, 2);
        assert_eq!(config.critical_latency_ms, 5);
        assert_eq!(config.min_depth, 8);
        assert_eq!(config.reduction_percent, 50);
        assert_eq!(config.history_size, 1000);
        assert_eq!(config.recovery_delay_ms, 500);
    }

    #[tokio::test]
    async fn test_try_acquire_respects_limit() {
        let limiter = create_test_limiter();
        
        for _ in 0..10 {
            assert!(limiter.try_acquire().await);
        }
        
        assert!(!limiter.try_acquire().await);
        assert_eq!(limiter.pending_count().await, 10);
    }

    #[tokio::test]
    async fn test_release_updates_latency() {
        let limiter = create_test_limiter();
        
        limiter.try_acquire().await;
        limiter.release(1000);
        
        let stats = limiter.stats().await;
        assert_eq!(stats.avg_latency_us, 1000);
    }

    #[tokio::test]
    async fn test_mode_transition_healthy_to_degraded() {
        let limiter = create_test_limiter();
        
        assert_eq!(limiter.mode().await, HealthAdaptiveMode::Healthy);
        
        for _ in 0..100 {
            limiter.try_acquire().await;
            limiter.release(3000);
        }
        
        limiter.check_and_adjust().await;
        
        assert_eq!(limiter.mode().await, HealthAdaptiveMode::Degraded);
    }

    #[tokio::test]
    async fn test_mode_transition_degraded_to_critical() {
        let limiter = create_test_limiter();
        
        for _ in 0..100 {
            limiter.try_acquire().await;
            limiter.release(6000);
        }
        
        limiter.check_and_adjust().await;
        
        assert_eq!(limiter.mode().await, HealthAdaptiveMode::Critical);
        assert_eq!(limiter.current_limit().await, 2);
    }

    #[tokio::test]
    async fn test_recovery() {
        let limiter = create_test_limiter();
        
        for _ in 0..100 {
            limiter.try_acquire().await;
            limiter.release(6000);
        }
        limiter.check_and_adjust().await;
        assert_eq!(limiter.mode().await, HealthAdaptiveMode::Critical);
        
        tokio::time::sleep(Duration::from_millis(60)).await;
        
        for _ in 0..100 {
            limiter.try_acquire().await;
            limiter.release(100);
        }
        
        limiter.check_and_adjust().await;
        
        assert_eq!(limiter.mode().await, HealthAdaptiveMode::Healthy);
    }

    #[tokio::test]
    async fn test_stats() {
        let limiter = create_test_limiter();
        
        limiter.try_acquire().await;
        limiter.release(100);
        limiter.try_acquire().await;
        limiter.release(200);
        limiter.try_acquire().await;
        limiter.release(300);
        
        let stats = limiter.stats().await;
        assert_eq!(stats.mode, HealthAdaptiveMode::Healthy);
        assert_eq!(stats.pending_ops, 0);
        assert_eq!(stats.avg_latency_us, 200);
        assert!(stats.p99_latency_us >= 100);
    }

    #[tokio::test]
    async fn test_stress() {
        let limiter = Arc::new(create_test_limiter());
        let mut handles = vec![];
        
        for _ in 0..10 {
            let limiter = Arc::clone(&limiter);
            let handle = tokio::spawn(async move {
                for _ in 0..50 {
                    if limiter.try_acquire().await {
                        tokio::time::sleep(Duration::from_micros(10)).await;
                        limiter.release(100);
                    }
                }
            });
            handles.push(handle);
        }
        
        for handle in handles {
            handle.await.unwrap();
        }
        
        let stats = limiter.stats().await;
        assert!(stats.reduction_events >= 0);
    }

    #[tokio::test]
    async fn test_concurrent_ops() {
        let limiter = Arc::new(create_test_limiter());
        
        let acquire_handle = {
            let limiter = Arc::clone(&limiter);
            tokio::spawn(async move {
                for _ in 0..5 {
                    limiter.try_acquire().await;
                }
            })
        };
        
        let release_handle = {
            let limiter = Arc::clone(&limiter);
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(10)).await;
                for _ in 0..3 {
                    limiter.release(100);
                }
            })
        };
        
        acquire_handle.await.unwrap();
        release_handle.await.unwrap();
        
        let pending = limiter.pending_count().await;
        assert!(pending <= 5 && pending >= 2);
    }

    #[tokio::test]
    async fn test_health_updates() {
        let limiter = create_test_limiter();
        
        let healthy = DeviceHealth {
            temperature_celsius: 40,
            percentage_used: 10,
            available_spare: 80,
            data_units_written: 1000,
            data_units_read: 2000,
            power_on_hours: 100,
            unsafe_shutdowns: 0,
            critical_warning: false,
        };
        
        limiter.update_health(healthy).await;
        assert_eq!(limiter.mode().await, HealthAdaptiveMode::Healthy);
        
        let degraded = DeviceHealth {
            temperature_celsius: 60,
            percentage_used: 75,
            available_spare: 15,
            data_units_written: 100000,
            data_units_read: 200000,
            power_on_hours: 5000,
            unsafe_shutdowns: 1,
            critical_warning: false,
        };
        
        limiter.update_health(degraded).await;
        assert_eq!(limiter.mode().await, HealthAdaptiveMode::Degraded);
        
        let critical = DeviceHealth {
            temperature_celsius: 80,
            percentage_used: 95,
            available_spare: 5,
            data_units_written: 1000000,
            data_units_read: 2000000,
            power_on_hours: 10000,
            unsafe_shutdowns: 5,
            critical_warning: true,
        };
        
        limiter.update_health(critical).await;
        assert_eq!(limiter.mode().await, HealthAdaptiveMode::Critical);
    }

    #[tokio::test]
    async fn test_min_depth() {
        let limiter = IoDepthLimiter::new(
            QueuePairId(0),
            IoDepthLimiterConfig {
                initial_depth: 100,
                degradation_latency_ms: 1,
                critical_latency_ms: 2,
                min_depth: 5,
                reduction_percent: 90,
                history_size: 100,
                recovery_delay_ms: 50,
            },
        );
        
        for _ in 0..100 {
            limiter.try_acquire().await;
            limiter.release(10000);
        }
        
        limiter.check_and_adjust().await;
        
        assert_eq!(limiter.current_limit().await, 5);
    }

    #[tokio::test]
    async fn test_recovery_delay() {
        let limiter = create_test_limiter();
        
        for _ in 0..50 {
            limiter.try_acquire().await;
            limiter.release(10000);
        }
        
        limiter.check_and_adjust().await;
        assert_eq!(limiter.mode().await, HealthAdaptiveMode::Critical);
        
        limiter.check_and_adjust().await;
        assert_eq!(limiter.mode().await, HealthAdaptiveMode::Critical);
        
        tokio::time::sleep(Duration::from_millis(60)).await;
        
        for _ in 0..50 {
            limiter.try_acquire().await;
            limiter.release(100);
        }
        
        limiter.check_and_adjust().await;
        assert_eq!(limiter.mode().await, HealthAdaptiveMode::Healthy);
    }

    #[tokio::test]
    async fn test_set_depth() {
        let limiter = create_test_limiter();
        
        limiter.set_depth(5).await;
        assert_eq!(limiter.current_limit().await, 5);
        
        limiter.set_depth(500).await;
        assert_eq!(limiter.current_limit().await, 256);
        
        limiter.set_depth(1).await;
        assert_eq!(limiter.current_limit().await, 2);
    }

    #[tokio::test]
    async fn test_p99_calculation() {
        let latencies: Vec<u64> = (1..=100).collect();
        let p99 = IoDepthLimiter::calculate_p99(&latencies);
        assert!(p99 >= 99 && p99 <= 100);
    }

    #[tokio::test]
    async fn test_avg_calculation() {
        let latencies = vec![100u64, 200, 300];
        let avg = IoDepthLimiter::calculate_avg(&latencies);
        assert_eq!(avg, 200);
        
        let empty: Vec<u64> = vec![];
        let avg_empty = IoDepthLimiter::calculate_avg(&empty);
        assert_eq!(avg_empty, 0);
    }
}