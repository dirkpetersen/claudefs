use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

const MAX_HISTOGRAM_CAPACITY: usize = 100_000;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OpKind {
    Read,
    Write,
    Stat,
    Open,
    Fsync,
    Rename,
    ListDir,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LatencySample {
    pub op: OpKind,
    pub latency_us: u64,
    pub timestamp_ns: u64,
    pub node_id: String,
}

#[derive(Clone, Debug)]
pub struct LatencyHistogram {
    samples: Vec<u64>,
}

impl LatencyHistogram {
    pub fn new() -> Self {
        Self {
            samples: Vec::with_capacity(MAX_HISTOGRAM_CAPACITY),
        }
    }

    pub fn record(&mut self, latency_us: u64) {
        if self.samples.len() >= MAX_HISTOGRAM_CAPACITY {
            self.samples.remove(0);
        }
        self.samples.push(latency_us);
    }

    pub fn count(&self) -> usize {
        self.samples.len()
    }

    pub fn percentile(&self, p: f64) -> u64 {
        if self.samples.is_empty() {
            return 0;
        }
        let mut sorted = self.samples.clone();
        sorted.sort();
        let index = ((p / 100.0) * sorted.len() as f64).floor() as usize;
        sorted[index.min(sorted.len() - 1)]
    }

    pub fn mean_us(&self) -> f64 {
        if self.samples.is_empty() {
            return 0.0;
        }
        let sum: u64 = self.samples.iter().sum();
        sum as f64 / self.samples.len() as f64
    }
}

impl Default for LatencyHistogram {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SlaThreshold {
    pub op: OpKind,
    pub p99_target_us: u64,
    pub p50_target_us: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SlaViolation {
    pub op: OpKind,
    pub percentile: u8,
    pub measured_us: u64,
    pub target_us: u64,
    pub timestamp_ns: u64,
}

pub struct PerformanceTracker {
    histograms: Mutex<HashMap<OpKind, LatencyHistogram>>,
    thresholds: Mutex<Vec<SlaThreshold>>,
}

impl PerformanceTracker {
    pub fn new() -> Self {
        Self {
            histograms: Mutex::new(HashMap::new()),
            thresholds: Mutex::new(Vec::new()),
        }
    }

    pub fn set_threshold(&self, threshold: SlaThreshold) {
        let mut thresholds: MutexGuard<'_, Vec<SlaThreshold>> = self.thresholds.lock().unwrap();
        if let Some(existing) = thresholds.iter_mut().find(|t| t.op == threshold.op) {
            existing.p99_target_us = threshold.p99_target_us;
            existing.p50_target_us = threshold.p50_target_us;
        } else {
            thresholds.push(threshold);
        }
    }

    pub fn record_sample(&self, sample: LatencySample) {
        let mut histograms: MutexGuard<'_, HashMap<OpKind, LatencyHistogram>> =
            self.histograms.lock().unwrap();
        let histogram = histograms
            .entry(sample.op.clone())
            .or_insert_with(LatencyHistogram::new);
        histogram.record(sample.latency_us);
    }

    pub fn check_violations(&self, now_ns: u64) -> Vec<SlaViolation> {
        let histograms: MutexGuard<'_, HashMap<OpKind, LatencyHistogram>> =
            self.histograms.lock().unwrap();
        let thresholds: MutexGuard<'_, Vec<SlaThreshold>> = self.thresholds.lock().unwrap();
        let mut violations = Vec::new();

        for threshold in thresholds.iter() {
            if let Some(histogram) = histograms.get(&threshold.op) {
                let p99: u64 = histogram.percentile(99.0);
                let p50: u64 = histogram.percentile(50.0);

                if p99 > threshold.p99_target_us {
                    violations.push(SlaViolation {
                        op: threshold.op.clone(),
                        percentile: 99,
                        measured_us: p99,
                        target_us: threshold.p99_target_us,
                        timestamp_ns: now_ns,
                    });
                }

                if p50 > threshold.p50_target_us {
                    violations.push(SlaViolation {
                        op: threshold.op.clone(),
                        percentile: 50,
                        measured_us: p50,
                        target_us: threshold.p50_target_us,
                        timestamp_ns: now_ns,
                    });
                }
            }
        }

        violations
    }

    pub fn histogram_for(&self, op: OpKind) -> Option<LatencyHistogram> {
        let histograms: MutexGuard<'_, HashMap<OpKind, LatencyHistogram>> =
            self.histograms.lock().unwrap();
        histograms.get(&op).cloned()
    }

    pub fn p99_us(&self, op: OpKind) -> u64 {
        let histograms: MutexGuard<'_, HashMap<OpKind, LatencyHistogram>> =
            self.histograms.lock().unwrap();
        histograms
            .get(&op)
            .map(|h: &LatencyHistogram| h.percentile(99.0))
            .unwrap_or(0)
    }

    pub fn p50_us(&self, op: OpKind) -> u64 {
        let histograms: MutexGuard<'_, HashMap<OpKind, LatencyHistogram>> =
            self.histograms.lock().unwrap();
        histograms
            .get(&op)
            .map(|h: &LatencyHistogram| h.percentile(50.0))
            .unwrap_or(0)
    }
}

impl Default for PerformanceTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_latency_histogram_new() {
        let hist = LatencyHistogram::new();
        assert_eq!(hist.count(), 0);
    }

    #[test]
    fn test_latency_histogram_record_and_count() {
        let mut hist = LatencyHistogram::new();
        hist.record(100);
        hist.record(200);
        hist.record(300);
        assert_eq!(hist.count(), 3);
    }

    #[test]
    fn test_latency_histogram_percentile_p0() {
        let mut hist = LatencyHistogram::new();
        hist.record(100);
        hist.record(200);
        hist.record(300);
        hist.record(400);
        hist.record(500);
        assert_eq!(hist.percentile(0.0), 100);
    }

    #[test]
    fn test_latency_histogram_percentile_p50() {
        let mut hist = LatencyHistogram::new();
        hist.record(100);
        hist.record(200);
        hist.record(300);
        hist.record(400);
        hist.record(500);
        assert_eq!(hist.percentile(50.0), 300);
    }

    #[test]
    fn test_latency_histogram_percentile_p99() {
        let mut hist = LatencyHistogram::new();
        for i in 1..=100 {
            hist.record(i * 100);
        }
        assert_eq!(hist.percentile(99.0), 10000);
    }

    #[test]
    fn test_latency_histogram_percentile_p100() {
        let mut hist = LatencyHistogram::new();
        hist.record(100);
        hist.record(200);
        hist.record(300);
        assert_eq!(hist.percentile(100.0), 300);
    }

    #[test]
    fn test_latency_histogram_percentile_empty() {
        let hist = LatencyHistogram::new();
        assert_eq!(hist.percentile(50.0), 0);
    }

    #[test]
    fn test_latency_histogram_mean_us() {
        let mut hist = LatencyHistogram::new();
        hist.record(100);
        hist.record(200);
        hist.record(300);
        assert!((hist.mean_us() - 200.0).abs() < 0.001);
    }

    #[test]
    fn test_latency_histogram_mean_us_empty() {
        let hist = LatencyHistogram::new();
        assert_eq!(hist.mean_us(), 0.0);
    }

    #[test]
    fn test_latency_histogram_wraps_at_capacity() {
        let mut hist = LatencyHistogram::new();
        for i in 0..MAX_HISTOGRAM_CAPACITY + 1000 {
            hist.record(i as u64);
        }
        assert_eq!(hist.count(), MAX_HISTOGRAM_CAPACITY);
    }

    #[test]
    fn test_performance_tracker_new() {
        let tracker = PerformanceTracker::new();
        assert_eq!(tracker.histogram_for(OpKind::Read).is_some(), false);
    }

    #[test]
    fn test_record_sample_stores_in_correct_histogram() {
        let tracker = PerformanceTracker::new();
        tracker.record_sample(LatencySample {
            op: OpKind::Read,
            latency_us: 500,
            timestamp_ns: 1000,
            node_id: "node1".to_string(),
        });
        let hist = tracker.histogram_for(OpKind::Read).unwrap();
        assert_eq!(hist.count(), 1);
        assert_eq!(hist.percentile(50.0), 500);
    }

    #[test]
    fn test_record_sample_multiple_ops() {
        let tracker = PerformanceTracker::new();
        tracker.record_sample(LatencySample {
            op: OpKind::Read,
            latency_us: 100,
            timestamp_ns: 1000,
            node_id: "node1".to_string(),
        });
        tracker.record_sample(LatencySample {
            op: OpKind::Write,
            latency_us: 200,
            timestamp_ns: 2000,
            node_id: "node1".to_string(),
        });
        assert_eq!(tracker.histogram_for(OpKind::Read).unwrap().count(), 1);
        assert_eq!(tracker.histogram_for(OpKind::Write).unwrap().count(), 1);
    }

    #[test]
    fn test_set_threshold_new_threshold() {
        let tracker = PerformanceTracker::new();
        tracker.set_threshold(SlaThreshold {
            op: OpKind::Read,
            p99_target_us: 1000,
            p50_target_us: 500,
        });
        let violations = tracker.check_violations(0);
        assert!(violations.is_empty());
    }

    #[test]
    fn test_set_threshold_updates_existing() {
        let tracker = PerformanceTracker::new();
        tracker.set_threshold(SlaThreshold {
            op: OpKind::Read,
            p99_target_us: 1000,
            p50_target_us: 500,
        });
        tracker.set_threshold(SlaThreshold {
            op: OpKind::Read,
            p99_target_us: 2000,
            p50_target_us: 1000,
        });
        let thresholds = tracker.thresholds.lock().unwrap();
        assert_eq!(thresholds.len(), 1);
        assert_eq!(thresholds[0].p99_target_us, 2000);
    }

    #[test]
    fn test_check_violations_detects_p99_breach() {
        let tracker = PerformanceTracker::new();
        for _ in 0..100 {
            tracker.record_sample(LatencySample {
                op: OpKind::Read,
                latency_us: 2000,
                timestamp_ns: 0,
                node_id: "node1".to_string(),
            });
        }
        tracker.set_threshold(SlaThreshold {
            op: OpKind::Read,
            p99_target_us: 1000,
            p50_target_us: 500,
        });
        let violations = tracker.check_violations(1000);
        assert!(violations
            .iter()
            .any(|v| v.percentile == 99 && v.measured_us > 1000));
    }

    #[test]
    fn test_check_violations_detects_p50_breach() {
        let tracker = PerformanceTracker::new();
        for _ in 0..100 {
            tracker.record_sample(LatencySample {
                op: OpKind::Write,
                latency_us: 1500,
                timestamp_ns: 0,
                node_id: "node1".to_string(),
            });
        }
        tracker.set_threshold(SlaThreshold {
            op: OpKind::Write,
            p99_target_us: 5000,
            p50_target_us: 500,
        });
        let violations = tracker.check_violations(1000);
        assert!(violations
            .iter()
            .any(|v| v.percentile == 50 && v.measured_us > 500));
    }

    #[test]
    fn test_check_violations_returns_empty_when_under_threshold() {
        let tracker = PerformanceTracker::new();
        for _ in 0..100 {
            tracker.record_sample(LatencySample {
                op: OpKind::Stat,
                latency_us: 100,
                timestamp_ns: 0,
                node_id: "node1".to_string(),
            });
        }
        tracker.set_threshold(SlaThreshold {
            op: OpKind::Stat,
            p99_target_us: 1000,
            p50_target_us: 500,
        });
        let violations = tracker.check_violations(1000);
        assert!(violations.is_empty());
    }

    #[test]
    fn test_p99_us_convenience_method() {
        let tracker = PerformanceTracker::new();
        for i in 1..=100 {
            tracker.record_sample(LatencySample {
                op: OpKind::Read,
                latency_us: i * 100,
                timestamp_ns: 0,
                node_id: "node1".to_string(),
            });
        }
        assert_eq!(tracker.p99_us(OpKind::Read), 10000);
    }

    #[test]
    fn test_p50_us_convenience_method() {
        let tracker = PerformanceTracker::new();
        for i in 1..=100 {
            tracker.record_sample(LatencySample {
                op: OpKind::Write,
                latency_us: i * 100,
                timestamp_ns: 0,
                node_id: "node1".to_string(),
            });
        }
        assert_eq!(tracker.p50_us(OpKind::Write), 5100);
    }

    #[test]
    fn test_p99_us_returns_zero_when_no_data() {
        let tracker = PerformanceTracker::new();
        assert_eq!(tracker.p99_us(OpKind::Read), 0);
    }

    #[test]
    fn test_p50_us_returns_zero_when_no_data() {
        let tracker = PerformanceTracker::new();
        assert_eq!(tracker.p50_us(OpKind::Write), 0);
    }

    #[test]
    fn test_histogram_for_returns_none_for_missing_op() {
        let tracker = PerformanceTracker::new();
        assert!(tracker.histogram_for(OpKind::Read).is_none());
    }

    #[test]
    fn test_violation_struct_fields() {
        let violation = SlaViolation {
            op: OpKind::Read,
            percentile: 99,
            measured_us: 1500,
            target_us: 1000,
            timestamp_ns: 12345,
        };
        assert_eq!(violation.op, OpKind::Read);
        assert_eq!(violation.percentile, 99);
        assert_eq!(violation.measured_us, 1500);
        assert_eq!(violation.target_us, 1000);
        assert_eq!(violation.timestamp_ns, 12345);
    }

    #[test]
    fn test_op_kind_derives() {
        let op1 = OpKind::Read;
        let op2 = OpKind::Read;
        let op3 = OpKind::Write;
        assert_eq!(op1, op2);
        assert_ne!(op1, op3);
        let _ = format!("{:?}", op1);
        let _ = format!("{:?}", op3);
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(OpKind::Read);
        set.insert(OpKind::Write);
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn test_serialize_op_kind() {
        let op = OpKind::Read;
        let serialized = serde_json::to_string(&op).unwrap();
        assert!(serialized.contains("Read"));
    }

    #[test]
    fn test_deserialize_op_kind() {
        let json = r#""Write""#;
        let op: OpKind = serde_json::from_str(json).unwrap();
        assert_eq!(op, OpKind::Write);
    }

    #[test]
    fn test_latency_sample_serialize() {
        let sample = LatencySample {
            op: OpKind::Open,
            latency_us: 500,
            timestamp_ns: 1000000,
            node_id: "node1".to_string(),
        };
        let serialized = serde_json::to_string(&sample).unwrap();
        assert!(serialized.contains("Open"));
        assert!(serialized.contains("500"));
    }

    #[test]
    fn test_check_violations_multiple_thresholds() {
        let tracker = PerformanceTracker::new();

        for _ in 0..100 {
            tracker.record_sample(LatencySample {
                op: OpKind::Read,
                latency_us: 2000,
                timestamp_ns: 0,
                node_id: "node1".to_string(),
            });
            tracker.record_sample(LatencySample {
                op: OpKind::Write,
                latency_us: 100,
                timestamp_ns: 0,
                node_id: "node1".to_string(),
            });
        }

        tracker.set_threshold(SlaThreshold {
            op: OpKind::Read,
            p99_target_us: 1000,
            p50_target_us: 500,
        });
        tracker.set_threshold(SlaThreshold {
            op: OpKind::Write,
            p99_target_us: 1000,
            p50_target_us: 500,
        });

        let violations = tracker.check_violations(1000);
        assert!(violations.len() >= 1);
    }

    #[test]
    fn test_histogram_single_sample_percentile() {
        let mut hist = LatencyHistogram::new();
        hist.record(500);
        assert_eq!(hist.percentile(0.0), 500);
        assert_eq!(hist.percentile(50.0), 500);
        assert_eq!(hist.percentile(100.0), 500);
    }

    #[test]
    fn test_performance_tracker_thread_safety_basic() {
        let tracker = PerformanceTracker::new();
        tracker.record_sample(LatencySample {
            op: OpKind::Read,
            latency_us: 100,
            timestamp_ns: 1,
            node_id: "n1".to_string(),
        });
        tracker.set_threshold(SlaThreshold {
            op: OpKind::Read,
            p99_target_us: 200,
            p50_target_us: 100,
        });
        let _ = tracker.check_violations(10);
        let _ = tracker.p99_us(OpKind::Read);
        let _ = tracker.p50_us(OpKind::Read);
    }
}
