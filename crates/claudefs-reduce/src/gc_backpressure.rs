use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GcBackpressureConfig {
    pub stall_threshold_ms: u64,
    pub max_delay_us: u64,
    pub increment_factor: f64,
    pub decrement_amount_us: u64,
}

impl Default for GcBackpressureConfig {
    fn default() -> Self {
        Self {
            stall_threshold_ms: 1000,
            max_delay_us: 10_000,
            increment_factor: 1.5,
            decrement_amount_us: 100,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackpressureState {
    pub current_delay_us: u64,
    pub is_under_pressure: bool,
    pub last_collection_latency_ms: u64,
    pub total_delays_applied: u64,
}

#[derive(Debug)]
pub struct GcBackpressure {
    config: GcBackpressureConfig,
    current_delay_us: AtomicU64,
    last_collection_time: Instant,
    last_collection_latency_ms: AtomicU64,
    total_delays_applied: AtomicU64,
}

impl GcBackpressure {
    pub fn new(config: GcBackpressureConfig) -> Self {
        Self {
            config,
            current_delay_us: AtomicU64::new(0),
            last_collection_time: Instant::now(),
            last_collection_latency_ms: AtomicU64::new(0),
            total_delays_applied: AtomicU64::new(0),
        }
    }

    pub fn with_default() -> Self {
        Self::new(GcBackpressureConfig::default())
    }

    pub fn record_collection(&mut self, latency_ms: u64) {
        self.last_collection_latency_ms.store(latency_ms, Ordering::Relaxed);
        self.last_collection_time = Instant::now();
    }

    pub fn calculate_delay(&self) -> Option<std::time::Duration> {
        let latency_ms = self.last_collection_latency_ms.load(Ordering::Relaxed);
        let current_delay = self.current_delay_us.load(Ordering::Relaxed);

        if latency_ms > self.config.stall_threshold_ms {
            let new_delay = if current_delay == 0 {
                1000
            } else {
                ((current_delay as f64) * self.config.increment_factor) as u64
            };
            let capped_delay = new_delay.min(self.config.max_delay_us);
            self.current_delay_us.store(capped_delay, Ordering::Relaxed);
            self.total_delays_applied.fetch_add(1, Ordering::Relaxed);

            Some(std::time::Duration::from_micros(capped_delay))
        } else if current_delay > 0 {
            let new_delay = current_delay.saturating_sub(self.config.decrement_amount_us);
            self.current_delay_us.store(new_delay, Ordering::Relaxed);

            if new_delay > 0 {
                Some(std::time::Duration::from_micros(new_delay))
            } else {
                None
            }
        } else {
            None
        }
    }

    pub async fn apply_if_needed(&self) {
        if let Some(delay) = self.calculate_delay() {
            if delay.as_micros() > 0 {
                tokio::time::sleep(delay).await;
            }
        }
    }

    pub fn get_state(&self) -> BackpressureState {
        let current_delay = self.current_delay_us.load(Ordering::Relaxed);
        let latency_ms = self.last_collection_latency_ms.load(Ordering::Relaxed);
        BackpressureState {
            current_delay_us: current_delay,
            is_under_pressure: latency_ms > self.config.stall_threshold_ms,
            last_collection_latency_ms: latency_ms,
            total_delays_applied: self.total_delays_applied.load(Ordering::Relaxed),
        }
    }

    pub fn reset(&self) {
        self.current_delay_us.store(0, Ordering::Relaxed);
        self.last_collection_latency_ms.store(0, Ordering::Relaxed);
    }
}

impl Default for GcBackpressure {
    fn default() -> Self {
        Self::with_default()
    }
}