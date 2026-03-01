use serde::{Deserialize, Serialize};
use tracing::{info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LagSla {
    pub warn_threshold_ms: u64,
    pub critical_threshold_ms: u64,
    pub max_acceptable_ms: u64,
}

impl Default for LagSla {
    fn default() -> Self {
        Self {
            warn_threshold_ms: 100,
            critical_threshold_ms: 500,
            max_acceptable_ms: 2000,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum LagStatus {
    Ok,
    Warning { lag_ms: u64 },
    Critical { lag_ms: u64 },
    Exceeded { lag_ms: u64 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LagSample {
    pub site_id: String,
    pub lag_ms: u64,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Default)]
pub struct LagStats {
    pub sample_count: u64,
    pub avg_lag_ms: f64,
    pub max_lag_ms: u64,
    pub warning_count: u64,
    pub critical_count: u64,
}

#[derive(Debug)]
pub struct LagMonitor {
    sla: LagSla,
    samples: Vec<LagSample>,
    stats: LagStats,
}

impl LagMonitor {
    pub fn new(sla: LagSla) -> Self {
        info!(
            "LagMonitor initialized with SLA: warn={}ms, critical={}ms, max={}ms",
            sla.warn_threshold_ms, sla.critical_threshold_ms, sla.max_acceptable_ms
        );
        Self {
            sla,
            samples: Vec::new(),
            stats: LagStats::default(),
        }
    }

    pub fn record_sample(&mut self, site_id: String, lag_ms: u64) -> LagStatus {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        self.samples.push(LagSample {
            site_id: site_id.clone(),
            lag_ms,
            timestamp,
        });

        self.stats.sample_count += 1;

        if lag_ms > self.sla.max_acceptable_ms {
            self.stats.critical_count += 1;
            warn!(
                "Site {} lag {}ms exceeded max acceptable {}ms",
                site_id, lag_ms, self.sla.max_acceptable_ms
            );
            return LagStatus::Exceeded { lag_ms };
        }

        if lag_ms >= self.sla.critical_threshold_ms {
            self.stats.critical_count += 1;
            warn!(
                "Site {} lag {}ms is critical (threshold {}ms)",
                site_id, lag_ms, self.sla.critical_threshold_ms
            );
            return LagStatus::Critical { lag_ms };
        }

        if lag_ms >= self.sla.warn_threshold_ms {
            self.stats.warning_count += 1;
            info!(
                "Site {} lag {}ms is warning level (threshold {}ms)",
                site_id, lag_ms, self.sla.warn_threshold_ms
            );
            return LagStatus::Warning { lag_ms };
        }

        self.stats.avg_lag_ms = if self.stats.sample_count == 1 {
            lag_ms as f64
        } else {
            let prev_total = self.stats.avg_lag_ms * (self.stats.sample_count - 1) as f64;
            (prev_total + lag_ms as f64) / self.stats.sample_count as f64
        };

        if lag_ms > self.stats.max_lag_ms {
            self.stats.max_lag_ms = lag_ms;
        }

        LagStatus::Ok
    }

    pub fn status_for(&self, site_id: &str) -> LagStatus {
        self.samples
            .iter()
            .filter(|s| s.site_id == site_id)
            .last()
            .map(|s| {
                let lag_ms = s.lag_ms;
                if lag_ms > self.sla.max_acceptable_ms {
                    LagStatus::Exceeded { lag_ms }
                } else if lag_ms >= self.sla.critical_threshold_ms {
                    LagStatus::Critical { lag_ms }
                } else if lag_ms >= self.sla.warn_threshold_ms {
                    LagStatus::Warning { lag_ms }
                } else {
                    LagStatus::Ok
                }
            })
            .unwrap_or(LagStatus::Ok)
    }

    pub fn stats(&self) -> &LagStats {
        &self.stats
    }

    pub fn clear_samples(&mut self) {
        self.samples.clear();
        self.stats = LagStats::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lag_sla_default() {
        let sla = LagSla::default();
        assert_eq!(sla.warn_threshold_ms, 100);
        assert_eq!(sla.critical_threshold_ms, 500);
        assert_eq!(sla.max_acceptable_ms, 2000);
    }

    #[test]
    fn test_lag_sla_custom() {
        let sla = LagSla {
            warn_threshold_ms: 50,
            critical_threshold_ms: 200,
            max_acceptable_ms: 1000,
        };
        assert_eq!(sla.warn_threshold_ms, 50);
        assert_eq!(sla.critical_threshold_ms, 200);
        assert_eq!(sla.max_acceptable_ms, 1000);
    }

    #[test]
    fn test_lag_monitor_new() {
        let sla = LagSla::default();
        let monitor = LagMonitor::new(sla);
        assert_eq!(monitor.stats().sample_count, 0);
    }

    #[test]
    fn test_record_sample_ok() {
        let sla = LagSla::default();
        let mut monitor = LagMonitor::new(sla);
        let status = monitor.record_sample("site-a".to_string(), 50);
        assert_eq!(status, LagStatus::Ok);
        assert_eq!(monitor.stats().sample_count, 1);
    }

    #[test]
    fn test_record_sample_warning() {
        let sla = LagSla::default();
        let mut monitor = LagMonitor::new(sla);
        let status = monitor.record_sample("site-a".to_string(), 150);
        assert_eq!(status, LagStatus::Warning { lag_ms: 150 });
        assert_eq!(monitor.stats().warning_count, 1);
    }

    #[test]
    fn test_record_sample_critical() {
        let sla = LagSla::default();
        let mut monitor = LagMonitor::new(sla);
        let status = monitor.record_sample("site-a".to_string(), 600);
        assert_eq!(status, LagStatus::Critical { lag_ms: 600 });
        assert_eq!(monitor.stats().critical_count, 1);
    }

    #[test]
    fn test_record_sample_exceeded() {
        let sla = LagSla::default();
        let mut monitor = LagMonitor::new(sla);
        let status = monitor.record_sample("site-a".to_string(), 2500);
        assert_eq!(status, LagStatus::Exceeded { lag_ms: 2500 });
        assert_eq!(monitor.stats().critical_count, 1);
    }

    #[test]
    fn test_status_for_existing_site() {
        let sla = LagSla::default();
        let mut monitor = LagMonitor::new(sla);
        monitor.record_sample("site-a".to_string(), 150);
        let status = monitor.status_for("site-a");
        assert_eq!(status, LagStatus::Warning { lag_ms: 150 });
    }

    #[test]
    fn test_status_for_nonexistent_site() {
        let sla = LagSla::default();
        let monitor = LagMonitor::new(sla);
        let status = monitor.status_for("nonexistent");
        assert_eq!(status, LagStatus::Ok);
    }

    #[test]
    fn test_stats_max_lag_tracking() {
        let sla = LagSla::default();
        let mut monitor = LagMonitor::new(sla);
        monitor.record_sample("site-a".to_string(), 50);
        monitor.record_sample("site-a".to_string(), 200);
        monitor.record_sample("site-a".to_string(), 150);
        assert_eq!(monitor.stats().max_lag_ms, 200);
    }

    #[test]
    fn test_stats_avg_lag_calculation() {
        let sla = LagSla::default();
        let mut monitor = LagMonitor::new(sla);
        monitor.record_sample("site-a".to_string(), 100);
        monitor.record_sample("site-a".to_string(), 200);
        assert!((monitor.stats().avg_lag_ms - 150.0).abs() < 0.001);
    }

    #[test]
    fn test_clear_samples() {
        let sla = LagSla::default();
        let mut monitor = LagMonitor::new(sla);
        monitor.record_sample("site-a".to_string(), 50);
        monitor.record_sample("site-b".to_string(), 150);
        monitor.clear_samples();
        assert_eq!(monitor.stats().sample_count, 0);
    }

    #[test]
    fn test_multiple_sites_different_statuses() {
        let sla = LagSla::default();
        let mut monitor = LagMonitor::new(sla);
        monitor.record_sample("site-a".to_string(), 50);
        monitor.record_sample("site-b".to_string(), 150);
        monitor.record_sample("site-c".to_string(), 600);

        assert_eq!(monitor.status_for("site-a"), LagStatus::Ok);
        assert_eq!(
            monitor.status_for("site-b"),
            LagStatus::Warning { lag_ms: 150 }
        );
        assert_eq!(
            monitor.status_for("site-c"),
            LagStatus::Critical { lag_ms: 600 }
        );
    }

    #[test]
    fn test_lag_status_equality() {
        let s1 = LagStatus::Warning { lag_ms: 100 };
        let s2 = LagStatus::Warning { lag_ms: 100 };
        let s3 = LagStatus::Warning { lag_ms: 200 };
        assert_eq!(s1, s2);
        assert_ne!(s1, s3);
    }

    #[test]
    fn test_lag_status_ok_equality() {
        assert_eq!(LagStatus::Ok, LagStatus::Ok);
    }

    #[test]
    fn test_lag_status_exceeded() {
        let status = LagStatus::Exceeded { lag_ms: 3000 };
        assert_ne!(status, LagStatus::Ok);
    }

    #[test]
    fn test_serialization_lag_sla() {
        let sla = LagSla::default();
        let json = serde_json::to_string(&sla).unwrap();
        let sla2: LagSla = serde_json::from_str(&json).unwrap();
        assert_eq!(sla.warn_threshold_ms, sla2.warn_threshold_ms);
    }

    #[test]
    fn test_serialization_lag_status() {
        let status = LagStatus::Critical { lag_ms: 750 };
        let json = serde_json::to_string(&status).unwrap();
        let status2: LagStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(status, status2);
    }

    #[test]
    fn test_serialization_lag_sample() {
        let sample = LagSample {
            site_id: "test-site".to_string(),
            lag_ms: 123,
            timestamp: 1234567890,
        };
        let json = serde_json::to_string(&sample).unwrap();
        let sample2: LagSample = serde_json::from_str(&json).unwrap();
        assert_eq!(sample.site_id, sample2.site_id);
        assert_eq!(sample.lag_ms, sample2.lag_ms);
    }

    #[test]
    fn test_stats_default() {
        let stats = LagStats::default();
        assert_eq!(stats.sample_count, 0);
        assert_eq!(stats.avg_lag_ms, 0.0);
        assert_eq!(stats.max_lag_ms, 0);
        assert_eq!(stats.warning_count, 0);
        assert_eq!(stats.critical_count, 0);
    }

    #[test]
    fn test_warning_threshold_boundary() {
        let sla = LagSla {
            warn_threshold_ms: 100,
            critical_threshold_ms: 500,
            max_acceptable_ms: 2000,
        };
        let mut monitor = LagMonitor::new(sla);

        assert_eq!(monitor.record_sample("site".to_string(), 99), LagStatus::Ok);
        assert_eq!(
            monitor.record_sample("site".to_string(), 100),
            LagStatus::Warning { lag_ms: 100 }
        );
    }

    #[test]
    fn test_critical_threshold_boundary() {
        let sla = LagSla {
            warn_threshold_ms: 100,
            critical_threshold_ms: 500,
            max_acceptable_ms: 2000,
        };
        let mut monitor = LagMonitor::new(sla);

        assert_eq!(
            monitor.record_sample("site".to_string(), 499),
            LagStatus::Warning { lag_ms: 499 }
        );
        assert_eq!(
            monitor.record_sample("site".to_string(), 500),
            LagStatus::Critical { lag_ms: 500 }
        );
    }

    #[test]
    fn test_max_acceptable_boundary() {
        let sla = LagSla {
            warn_threshold_ms: 100,
            critical_threshold_ms: 500,
            max_acceptable_ms: 2000,
        };
        let mut monitor = LagMonitor::new(sla);

        assert_eq!(
            monitor.record_sample("site".to_string(), 1999),
            LagStatus::Critical { lag_ms: 1999 }
        );
        assert_eq!(
            monitor.record_sample("site".to_string(), 2000),
            LagStatus::Exceeded { lag_ms: 2000 }
        );
    }

    #[test]
    fn test_stats_clone() {
        let sla = LagSla::default();
        let monitor = LagMonitor::new(sla);
        let stats_clone = monitor.stats().clone();
        assert_eq!(stats_clone.sample_count, 0);
    }
}
