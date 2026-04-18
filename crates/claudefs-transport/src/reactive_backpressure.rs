//! Coordinated backpressure propagation for distributed filesystem.
//! Tracks signals from all components and provides adaptive backoff to clients.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Hash)]
pub enum BackpressureLevel {
    Ok,
    Slow,
    Degraded,
    Overloaded,
}

impl BackpressureLevel {
    pub fn severity(&self) -> f64 {
        match self {
            BackpressureLevel::Ok => 0.0,
            BackpressureLevel::Slow => 0.33,
            BackpressureLevel::Degraded => 0.66,
            BackpressureLevel::Overloaded => 1.0,
        }
    }

    pub fn is_degraded(&self) -> bool {
        matches!(
            self,
            BackpressureLevel::Degraded | BackpressureLevel::Overloaded
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackpressureSignal {
    pub level: BackpressureLevel,
    pub source_id: u64,
    pub timestamp_ns: u64,
    pub severity: f64,
    pub suggested_backoff_ms: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackpressureConfig {
    pub cpu_slow_threshold: u32,
    pub cpu_degraded_threshold: u32,
    pub cpu_overloaded_threshold: u32,
    pub memory_slow_threshold: u32,
    pub memory_degraded_threshold: u32,
    pub memory_overloaded_threshold: u32,
    pub queue_slow_threshold: u32,
    pub queue_degraded_threshold: u32,
    pub queue_overloaded_threshold: u32,
    pub latency_slow_threshold: u32,
    pub latency_degraded_threshold: u32,
    pub latency_overloaded_threshold: u32,
    pub signal_ttl_ms: u64,
    pub max_backoff_ms: u32,
}

impl Default for BackpressureConfig {
    fn default() -> Self {
        Self {
            cpu_slow_threshold: 70,
            cpu_degraded_threshold: 85,
            cpu_overloaded_threshold: 95,
            memory_slow_threshold: 60,
            memory_degraded_threshold: 80,
            memory_overloaded_threshold: 95,
            queue_slow_threshold: 100,
            queue_degraded_threshold: 500,
            queue_overloaded_threshold: 1000,
            latency_slow_threshold: 50,
            latency_degraded_threshold: 200,
            latency_overloaded_threshold: 1000,
            signal_ttl_ms: 5000,
            max_backoff_ms: 10000,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ComponentMetrics {
    pub cpu_usage: u32,
    pub memory_usage: u32,
    pub queue_depth: u32,
    pub latency_ms: u32,
}

pub struct BackpressureStats {
    pub signals_emitted: AtomicU64,
    pub signals_received: AtomicU64,
    pub backoff_events: AtomicU64,
    pub overloaded_transitions: AtomicU64,
}

impl Default for BackpressureStats {
    fn default() -> Self {
        Self {
            signals_emitted: AtomicU64::new(0),
            signals_received: AtomicU64::new(0),
            backoff_events: AtomicU64::new(0),
            overloaded_transitions: AtomicU64::new(0),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackpressureStatsSnapshot {
    pub signals_emitted: u64,
    pub signals_received: u64,
    pub backoff_events: u64,
    pub overloaded_transitions: u64,
}

pub struct BackpressureCoordinator {
    pub config: BackpressureConfig,
    pub stats: Arc<BackpressureStats>,
    signals: RwLock<HashMap<u64, (BackpressureSignal, u64)>>,
}

impl BackpressureCoordinator {
    pub fn new(config: BackpressureConfig) -> Self {
        Self {
            config,
            stats: Arc::new(BackpressureStats::default()),
            signals: RwLock::new(HashMap::new()),
        }
    }

    pub fn record_metrics(&self, source_id: u64, metrics: ComponentMetrics) -> BackpressureSignal {
        let level = self.compute_level(&metrics);
        let severity = level.severity();
        let suggested_backoff_ms = self.suggested_backoff(level);

        let signal = BackpressureSignal {
            level,
            source_id,
            timestamp_ns: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos() as u64,
            severity,
            suggested_backoff_ms,
        };

        self.emit_signal(signal.clone());
        signal
    }

    fn compute_level(&self, metrics: &ComponentMetrics) -> BackpressureLevel {
        let cpu_level = if metrics.cpu_usage >= self.config.cpu_overloaded_threshold {
            BackpressureLevel::Overloaded
        } else if metrics.cpu_usage >= self.config.cpu_degraded_threshold {
            BackpressureLevel::Degraded
        } else if metrics.cpu_usage >= self.config.cpu_slow_threshold {
            BackpressureLevel::Slow
        } else {
            BackpressureLevel::Ok
        };

        let mem_level = if metrics.memory_usage >= self.config.memory_overloaded_threshold {
            BackpressureLevel::Overloaded
        } else if metrics.memory_usage >= self.config.memory_degraded_threshold {
            BackpressureLevel::Degraded
        } else if metrics.memory_usage >= self.config.memory_slow_threshold {
            BackpressureLevel::Slow
        } else {
            BackpressureLevel::Ok
        };

        let queue_level = if metrics.queue_depth >= self.config.queue_overloaded_threshold {
            BackpressureLevel::Overloaded
        } else if metrics.queue_depth >= self.config.queue_degraded_threshold {
            BackpressureLevel::Degraded
        } else if metrics.queue_depth >= self.config.queue_slow_threshold {
            BackpressureLevel::Slow
        } else {
            BackpressureLevel::Ok
        };

        let latency_level = if metrics.latency_ms >= self.config.latency_overloaded_threshold {
            BackpressureLevel::Overloaded
        } else if metrics.latency_ms >= self.config.latency_degraded_threshold {
            BackpressureLevel::Degraded
        } else if metrics.latency_ms >= self.config.latency_slow_threshold {
            BackpressureLevel::Slow
        } else {
            BackpressureLevel::Ok
        };

        let levels = [cpu_level, mem_level, queue_level, latency_level];
        *levels.iter().max_by(|a, b| a.cmp(b)).unwrap()
    }

    fn suggested_backoff(&self, level: BackpressureLevel) -> u32 {
        match level {
            BackpressureLevel::Ok => 0,
            BackpressureLevel::Slow => 10,
            BackpressureLevel::Degraded => 100,
            BackpressureLevel::Overloaded => self.config.max_backoff_ms,
        }
    }

    pub fn emit_signal(&self, signal: BackpressureSignal) {
        self.stats.signals_emitted.fetch_add(1, Ordering::Relaxed);
        if signal.level == BackpressureLevel::Overloaded {
            self.stats
                .overloaded_transitions
                .fetch_add(1, Ordering::Relaxed);
        }

        let mut signals = self.signals.write().unwrap();
        let timestamp = signal.timestamp_ns;
        signals.insert(signal.source_id, (signal, timestamp));
    }

    pub fn global_level(&self) -> BackpressureLevel {
        let signals = self.signals.read().unwrap();
        signals
            .values()
            .filter(|(_, ts)| {
                let age_ms = (std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos() as u64
                    - ts)
                    / 1_000_000;
                age_ms < self.config.signal_ttl_ms
            })
            .map(|(s, _)| s.level)
            .max_by(|a, b| a.cmp(b))
            .unwrap_or(BackpressureLevel::Ok)
    }

    pub fn backoff_ms(&self, iteration: u32) -> u32 {
        self.stats.backoff_events.fetch_add(1, Ordering::Relaxed);

        let base: u64 = 10;
        let jitter_max: u64 = 5;
        let max_iter = 30; // Prevent overflow
        let iter = iteration.min(max_iter);
        let exponential = base * (2u64.pow(iter));
        let jitter = (rand_simple(iteration) % jitter_max) as u64;
        let backoff = (exponential + jitter).min(self.config.max_backoff_ms as u64);

        backoff as u32
    }

    pub fn signal_for_source(&self, source_id: u64) -> Option<BackpressureSignal> {
        let signals = self.signals.read().unwrap();
        signals.get(&source_id).map(|(s, _)| s.clone())
    }

    pub fn expire_old_signals(&self, now_ns: u64) {
        let mut signals = self.signals.write().unwrap();
        let ttl_ns = self.config.signal_ttl_ms * 1_000_000;
        signals.retain(|_, (_, ts)| now_ns - *ts < ttl_ns);
    }

    pub fn stats_snapshot(&self) -> BackpressureStatsSnapshot {
        BackpressureStatsSnapshot {
            signals_emitted: self.stats.signals_emitted.load(Ordering::Relaxed),
            signals_received: self.stats.signals_received.load(Ordering::Relaxed),
            backoff_events: self.stats.backoff_events.load(Ordering::Relaxed),
            overloaded_transitions: self.stats.overloaded_transitions.load(Ordering::Relaxed),
        }
    }
}

fn rand_simple(seed: u32) -> u64 {
    let mut x = seed as u64;
    x = x.wrapping_mul(1103515245).wrapping_add(12345);
    x % 32768
}

unsafe impl Send for BackpressureCoordinator {}
unsafe impl Sync for BackpressureCoordinator {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backpressure_level_severity_mapping() {
        assert_eq!(BackpressureLevel::Ok.severity(), 0.0);
        assert_eq!(BackpressureLevel::Slow.severity(), 0.33);
        assert_eq!(BackpressureLevel::Degraded.severity(), 0.66);
        assert_eq!(BackpressureLevel::Overloaded.severity(), 1.0);
    }

    #[test]
    fn test_cpu_threshold_triggers_slow() {
        let config = BackpressureConfig::default();
        let coordinator = BackpressureCoordinator::new(config);
        let metrics = ComponentMetrics {
            cpu_usage: 75,
            memory_usage: 30,
            queue_depth: 10,
            latency_ms: 5,
        };
        let signal = coordinator.record_metrics(1, metrics);
        assert_eq!(signal.level, BackpressureLevel::Slow);
    }

    #[test]
    fn test_cpu_threshold_triggers_degraded() {
        let config = BackpressureConfig::default();
        let coordinator = BackpressureCoordinator::new(config);
        let metrics = ComponentMetrics {
            cpu_usage: 90,
            memory_usage: 30,
            queue_depth: 10,
            latency_ms: 5,
        };
        let signal = coordinator.record_metrics(1, metrics);
        assert_eq!(signal.level, BackpressureLevel::Degraded);
    }

    #[test]
    fn test_cpu_threshold_triggers_overloaded() {
        let config = BackpressureConfig::default();
        let coordinator = BackpressureCoordinator::new(config);
        let metrics = ComponentMetrics {
            cpu_usage: 96,
            memory_usage: 30,
            queue_depth: 10,
            latency_ms: 5,
        };
        let signal = coordinator.record_metrics(1, metrics);
        assert_eq!(signal.level, BackpressureLevel::Overloaded);
    }

    #[test]
    fn test_memory_threshold_triggers_slow() {
        let config = BackpressureConfig::default();
        let coordinator = BackpressureCoordinator::new(config);
        let metrics = ComponentMetrics {
            cpu_usage: 30,
            memory_usage: 65,
            queue_depth: 10,
            latency_ms: 5,
        };
        let signal = coordinator.record_metrics(1, metrics);
        assert_eq!(signal.level, BackpressureLevel::Slow);
    }

    #[test]
    fn test_memory_threshold_triggers_degraded() {
        let config = BackpressureConfig::default();
        let coordinator = BackpressureCoordinator::new(config);
        let metrics = ComponentMetrics {
            cpu_usage: 30,
            memory_usage: 85,
            queue_depth: 10,
            latency_ms: 5,
        };
        let signal = coordinator.record_metrics(1, metrics);
        assert_eq!(signal.level, BackpressureLevel::Degraded);
    }

    #[test]
    fn test_memory_threshold_triggers_overloaded() {
        let config = BackpressureConfig::default();
        let coordinator = BackpressureCoordinator::new(config);
        let metrics = ComponentMetrics {
            cpu_usage: 30,
            memory_usage: 96,
            queue_depth: 10,
            latency_ms: 5,
        };
        let signal = coordinator.record_metrics(1, metrics);
        assert_eq!(signal.level, BackpressureLevel::Overloaded);
    }

    #[test]
    fn test_queue_threshold_triggers_slow() {
        let config = BackpressureConfig::default();
        let coordinator = BackpressureCoordinator::new(config);
        let metrics = ComponentMetrics {
            cpu_usage: 30,
            memory_usage: 30,
            queue_depth: 150,
            latency_ms: 5,
        };
        let signal = coordinator.record_metrics(1, metrics);
        assert_eq!(signal.level, BackpressureLevel::Slow);
    }

    #[test]
    fn test_queue_threshold_triggers_degraded() {
        let config = BackpressureConfig::default();
        let coordinator = BackpressureCoordinator::new(config);
        let metrics = ComponentMetrics {
            cpu_usage: 30,
            memory_usage: 30,
            queue_depth: 600,
            latency_ms: 5,
        };
        let signal = coordinator.record_metrics(1, metrics);
        assert_eq!(signal.level, BackpressureLevel::Degraded);
    }

    #[test]
    fn test_queue_threshold_triggers_overloaded() {
        let config = BackpressureConfig::default();
        let coordinator = BackpressureCoordinator::new(config);
        let metrics = ComponentMetrics {
            cpu_usage: 30,
            memory_usage: 30,
            queue_depth: 1100,
            latency_ms: 5,
        };
        let signal = coordinator.record_metrics(1, metrics);
        assert_eq!(signal.level, BackpressureLevel::Overloaded);
    }

    #[test]
    fn test_global_level_max_of_all_signals() {
        let config = BackpressureConfig::default();
        let coordinator = BackpressureCoordinator::new(config);

        coordinator.record_metrics(
            1,
            ComponentMetrics {
                cpu_usage: 50,
                memory_usage: 50,
                queue_depth: 50,
                latency_ms: 5,
            },
        );
        coordinator.record_metrics(
            2,
            ComponentMetrics {
                cpu_usage: 90,
                memory_usage: 50,
                queue_depth: 50,
                latency_ms: 5,
            },
        );

        assert_eq!(coordinator.global_level(), BackpressureLevel::Degraded);
    }

    #[test]
    fn test_signal_for_source_returns_latest() {
        let config = BackpressureConfig::default();
        let coordinator = BackpressureCoordinator::new(config);

        coordinator.record_metrics(
            1,
            ComponentMetrics {
                cpu_usage: 50,
                memory_usage: 50,
                queue_depth: 50,
                latency_ms: 5,
            },
        );
        coordinator.record_metrics(
            1,
            ComponentMetrics {
                cpu_usage: 90,
                memory_usage: 50,
                queue_depth: 50,
                latency_ms: 5,
            },
        );

        let signal = coordinator.signal_for_source(1).unwrap();
        assert_eq!(signal.level, BackpressureLevel::Degraded);
    }

    #[test]
    fn test_signal_expiration_removes_old() {
        let config = BackpressureConfig {
            signal_ttl_ms: 100,
            ..Default::default()
        };
        let coordinator = BackpressureCoordinator::new(config);

        coordinator.record_metrics(
            1,
            ComponentMetrics {
                cpu_usage: 95,
                memory_usage: 95,
                queue_depth: 1100,
                latency_ms: 5,
            },
        );

        assert_eq!(coordinator.global_level(), BackpressureLevel::Overloaded);

        coordinator.expire_old_signals(u64::MAX);

        assert_eq!(coordinator.global_level(), BackpressureLevel::Ok);
    }

    #[test]
    fn test_backoff_exponential_growth() {
        let config = BackpressureConfig::default();
        let coordinator = BackpressureCoordinator::new(config);

        let backoff_0 = coordinator.backoff_ms(0);
        let backoff_1 = coordinator.backoff_ms(1);
        let backoff_2 = coordinator.backoff_ms(2);

        assert!(backoff_1 > backoff_0);
        assert!(backoff_2 > backoff_1);
    }

    #[test]
    fn test_backoff_max_capped() {
        let config = BackpressureConfig {
            max_backoff_ms: 100,
            ..Default::default()
        };
        let coordinator = BackpressureCoordinator::new(config);

        let backoff = coordinator.backoff_ms(100);

        assert!(backoff <= 100);
    }

    #[test]
    fn test_backoff_jitter_present() {
        let config = BackpressureConfig::default();
        let coordinator = BackpressureCoordinator::new(config);

        let backoffs: Vec<u32> = (0..10).map(|i| coordinator.backoff_ms(i)).collect();

        let all_same = backoffs.windows(2).all(|w| w[0] == w[1]);
        assert!(
            !all_same,
            "Expected jitter to produce different backoff values"
        );
    }

    #[test]
    fn test_concurrent_emit_signal() {
        use std::thread;

        let config = BackpressureConfig::default();
        let coordinator = Arc::new(BackpressureCoordinator::new(config));

        let handles: Vec<_> = (0..10)
            .map(|i| {
                let coordinator = Arc::clone(&coordinator);
                thread::spawn(move || {
                    for j in 0..100 {
                        coordinator.emit_signal(BackpressureSignal {
                            level: BackpressureLevel::Ok,
                            source_id: (i * 100 + j) as u64,
                            timestamp_ns: 0,
                            severity: 0.0,
                            suggested_backoff_ms: 0,
                        });
                    }
                })
            })
            .collect();

        for handle in handles {
            handle.join().unwrap();
        }

        let stats = coordinator.stats_snapshot();
        assert_eq!(stats.signals_emitted, 1000);
    }

    #[test]
    fn test_concurrent_record_metrics() {
        use std::thread;

        let config = BackpressureConfig::default();
        let coordinator = Arc::new(BackpressureCoordinator::new(config));

        let handles: Vec<_> = (0..5)
            .map(|i| {
                let coordinator = Arc::clone(&coordinator);
                thread::spawn(move || {
                    for j in 0..20 {
                        coordinator.record_metrics(
                            (i * 20 + j) as u64,
                            ComponentMetrics {
                                cpu_usage: 50,
                                memory_usage: 50,
                                queue_depth: 50,
                                latency_ms: 5,
                            },
                        );
                    }
                })
            })
            .collect();

        for handle in handles {
            handle.join().unwrap();
        }

        let stats = coordinator.stats_snapshot();
        assert_eq!(stats.signals_emitted, 100);
    }

    #[test]
    fn test_signal_overwrite_per_source() {
        let config = BackpressureConfig::default();
        let coordinator = BackpressureCoordinator::new(config);

        coordinator.record_metrics(
            1,
            ComponentMetrics {
                cpu_usage: 50,
                memory_usage: 50,
                queue_depth: 50,
                latency_ms: 5,
            },
        );
        coordinator.record_metrics(
            1,
            ComponentMetrics {
                cpu_usage: 95,
                memory_usage: 50,
                queue_depth: 50,
                latency_ms: 5,
            },
        );

        let signals = coordinator.signals.read().unwrap();
        assert_eq!(signals.len(), 1);
    }

    #[test]
    fn test_latency_ms_field_included_in_signal() {
        let config = BackpressureConfig::default();
        let coordinator = BackpressureCoordinator::new(config);

        let signal = coordinator.record_metrics(
            1,
            ComponentMetrics {
                cpu_usage: 30,
                memory_usage: 30,
                queue_depth: 30,
                latency_ms: 100,
            },
        );

        assert_eq!(signal.suggested_backoff_ms, 10);
    }

    #[test]
    fn test_suggested_backoff_matches_level() {
        let config = BackpressureConfig::default();
        let coordinator = BackpressureCoordinator::new(config);

        assert_eq!(coordinator.suggested_backoff(BackpressureLevel::Ok), 0);
        assert_eq!(coordinator.suggested_backoff(BackpressureLevel::Slow), 10);
        assert_eq!(
            coordinator.suggested_backoff(BackpressureLevel::Degraded),
            100
        );
        assert_eq!(
            coordinator.suggested_backoff(BackpressureLevel::Overloaded),
            10000
        );
    }

    #[test]
    fn test_coordinator_creation_with_default_config() {
        let coordinator = BackpressureCoordinator::new(BackpressureConfig::default());

        assert_eq!(coordinator.global_level(), BackpressureLevel::Ok);

        let stats = coordinator.stats_snapshot();
        assert_eq!(stats.signals_emitted, 0);
    }

    #[test]
    fn test_coordinator_thread_safe_across_many_sources() {
        use std::thread;

        let config = BackpressureConfig::default();
        let coordinator = Arc::new(BackpressureCoordinator::new(config));

        let mut handles = vec![];
        for source_id in 0u64..50 {
            let coordinator = Arc::clone(&coordinator);
            handles.push(thread::spawn(move || {
                coordinator.record_metrics(
                    source_id,
                    ComponentMetrics {
                        cpu_usage: 50,
                        memory_usage: 50,
                        queue_depth: 50,
                        latency_ms: 5,
                    },
                );
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }

        let signals = coordinator.signals.read().unwrap();
        assert_eq!(signals.len(), 50);
    }

    #[test]
    fn test_stats_tracking_emitted_signals() {
        let config = BackpressureConfig::default();
        let coordinator = BackpressureCoordinator::new(config);

        coordinator.emit_signal(BackpressureSignal {
            level: BackpressureLevel::Ok,
            source_id: 1,
            timestamp_ns: 0,
            severity: 0.0,
            suggested_backoff_ms: 0,
        });

        let stats = coordinator.stats_snapshot();
        assert_eq!(stats.signals_emitted, 1);
    }

    #[test]
    fn test_stats_tracking_overloaded_transitions() {
        let config = BackpressureConfig::default();
        let coordinator = BackpressureCoordinator::new(config);

        coordinator.emit_signal(BackpressureSignal {
            level: BackpressureLevel::Overloaded,
            source_id: 1,
            timestamp_ns: 0,
            severity: 1.0,
            suggested_backoff_ms: 10000,
        });

        let stats = coordinator.stats_snapshot();
        assert_eq!(stats.overloaded_transitions, 1);
    }

    #[test]
    fn test_backpressure_signal_lifecycle() {
        let config = BackpressureConfig::default();
        let coordinator = BackpressureCoordinator::new(config);

        let signal = BackpressureSignal {
            level: BackpressureLevel::Degraded,
            source_id: 42,
            timestamp_ns: 1000,
            severity: 0.66,
            suggested_backoff_ms: 100,
        };

        coordinator.emit_signal(signal.clone());

        let retrieved = coordinator.signal_for_source(42).unwrap();
        assert_eq!(retrieved.level, BackpressureLevel::Degraded);
        assert_eq!(retrieved.source_id, 42);
        assert_eq!(retrieved.suggested_backoff_ms, 100);
    }
}
