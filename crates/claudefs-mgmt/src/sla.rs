use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SlaMetricKind {
    ReadLatencyUs,
    WriteLatencyUs,
    MetadataLatencyUs,
    ThroughputMBps,
    Iops,
    AvailabilityPercent,
}

impl SlaMetricKind {
    pub fn name(&self) -> &'static str {
        match self {
            SlaMetricKind::ReadLatencyUs => "read_latency_us",
            SlaMetricKind::WriteLatencyUs => "write_latency_us",
            SlaMetricKind::MetadataLatencyUs => "metadata_latency_us",
            SlaMetricKind::ThroughputMBps => "throughput_mbps",
            SlaMetricKind::Iops => "iops",
            SlaMetricKind::AvailabilityPercent => "availability_percent",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlaTarget {
    pub kind: SlaMetricKind,
    pub p50_threshold: f64,
    pub p95_threshold: f64,
    pub p99_threshold: f64,
    pub description: String,
}

impl SlaTarget {
    pub fn new(
        kind: SlaMetricKind,
        p50_threshold: f64,
        p95_threshold: f64,
        p99_threshold: f64,
        description: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            p50_threshold,
            p95_threshold,
            p99_threshold,
            description: description.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct LatencySample {
    pub value_us: u64,
    pub timestamp: u64,
}

impl LatencySample {
    pub fn new(value_us: u64, timestamp: u64) -> Self {
        Self {
            value_us,
            timestamp,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PercentileResult {
    pub p50: f64,
    pub p95: f64,
    pub p99: f64,
    pub p999: f64,
    pub min: f64,
    pub max: f64,
    pub mean: f64,
    pub sample_count: usize,
}

pub fn compute_percentiles(samples: &[u64]) -> Option<PercentileResult> {
    if samples.is_empty() {
        return None;
    }

    let mut sorted = samples.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let n = sorted.len();
    let sum: f64 = sorted.iter().map(|&x| x as f64).sum();
    let mean = sum / n as f64;

    let p50_idx = percentile_index(n, 0.50);
    let p95_idx = percentile_index(n, 0.95);
    let p99_idx = percentile_index(n, 0.99);
    let p999_idx = percentile_index(n, 0.999);

    Some(PercentileResult {
        p50: sorted[p50_idx] as f64,
        p95: sorted[p95_idx] as f64,
        p99: sorted[p99_idx] as f64,
        p999: sorted[p999_idx] as f64,
        min: sorted[0] as f64,
        max: sorted[n - 1] as f64,
        mean,
        sample_count: n,
    })
}

fn percentile_index(n: usize, p: f64) -> usize {
    let idx = ((n as f64 - 1.0) * p).floor() as usize;
    idx.min(n - 1)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SlaViolation {
    P50Exceeded { actual: f64, threshold: f64 },
    P95Exceeded { actual: f64, threshold: f64 },
    P99Exceeded { actual: f64, threshold: f64 },
}

impl std::fmt::Display for SlaViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SlaViolation::P50Exceeded { actual, threshold } => {
                write!(f, "P50 {} exceeded threshold {}", actual, threshold)
            }
            SlaViolation::P95Exceeded { actual, threshold } => {
                write!(f, "P95 {} exceeded threshold {}", actual, threshold)
            }
            SlaViolation::P99Exceeded { actual, threshold } => {
                write!(f, "P99 {} exceeded threshold {}", actual, threshold)
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlaCheckResult {
    pub target: SlaMetricKind,
    pub percentiles: PercentileResult,
    pub violations: Vec<SlaViolation>,
    pub compliant: bool,
    pub checked_at: u64,
}

#[derive(Debug, Clone)]
pub struct SlaWindow {
    max_samples: usize,
    samples: VecDeque<LatencySample>,
}

impl SlaWindow {
    pub fn new(max_samples: usize) -> Self {
        Self {
            max_samples,
            samples: VecDeque::with_capacity(max_samples),
        }
    }

    pub fn push(&mut self, value_us: u64, timestamp: u64) {
        if self.samples.len() >= self.max_samples {
            self.samples.pop_front();
        }
        self.samples
            .push_back(LatencySample::new(value_us, timestamp));
    }

    pub fn compute(&self) -> Option<PercentileResult> {
        let values: Vec<u64> = self.samples.iter().map(|s| s.value_us).collect();
        compute_percentiles(&values)
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn clear(&mut self) {
        self.samples.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    pub fn oldest_timestamp(&self) -> Option<u64> {
        self.samples.front().map(|s| s.timestamp)
    }

    pub fn newest_timestamp(&self) -> Option<u64> {
        self.samples.back().map(|s| s.timestamp)
    }
}

pub struct SlaChecker {
    targets: HashMap<SlaMetricKind, SlaTarget>,
}

impl SlaChecker {
    pub fn new() -> Self {
        Self {
            targets: HashMap::new(),
        }
    }

    pub fn add_target(&mut self, target: SlaTarget) {
        self.targets.insert(target.kind, target);
    }

    pub fn get_target(&self, kind: &SlaMetricKind) -> Option<&SlaTarget> {
        self.targets.get(kind)
    }

    pub fn check(&self, kind: &SlaMetricKind, window: &SlaWindow) -> SlaCheckResult {
        let percentiles = match window.compute() {
            Some(p) => p,
            None => {
                return SlaCheckResult {
                    target: *kind,
                    percentiles: PercentileResult {
                        p50: 0.0,
                        p95: 0.0,
                        p99: 0.0,
                        p999: 0.0,
                        min: 0.0,
                        max: 0.0,
                        mean: 0.0,
                        sample_count: 0,
                    },
                    violations: vec![],
                    compliant: true,
                    checked_at: current_time_ns(),
                };
            }
        };

        let target = match self.targets.get(kind) {
            Some(t) => t,
            None => {
                return SlaCheckResult {
                    target: *kind,
                    percentiles,
                    violations: vec![],
                    compliant: true,
                    checked_at: current_time_ns(),
                };
            }
        };

        let mut violations = vec![];

        if percentiles.p50 > target.p50_threshold {
            violations.push(SlaViolation::P50Exceeded {
                actual: percentiles.p50,
                threshold: target.p50_threshold,
            });
        }

        if percentiles.p95 > target.p95_threshold {
            violations.push(SlaViolation::P95Exceeded {
                actual: percentiles.p95,
                threshold: target.p95_threshold,
            });
        }

        if percentiles.p99 > target.p99_threshold {
            violations.push(SlaViolation::P99Exceeded {
                actual: percentiles.p99,
                threshold: target.p99_threshold,
            });
        }

        let is_compliant = violations.is_empty();

        SlaCheckResult {
            target: *kind,
            percentiles,
            violations,
            compliant: is_compliant,
            checked_at: current_time_ns(),
        }
    }

    pub fn check_all(&self, windows: &HashMap<SlaMetricKind, SlaWindow>) -> Vec<SlaCheckResult> {
        self.targets
            .keys()
            .map(|kind| {
                let window = windows
                    .get(kind)
                    .cloned()
                    .unwrap_or_else(|| SlaWindow::new(1000));
                self.check(kind, &window)
            })
            .collect()
    }
}

impl Default for SlaChecker {
    fn default() -> Self {
        Self::new()
    }
}

fn current_time_ns() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlaReport {
    pub generated_at: u64,
    pub cluster_id: String,
    pub checks: Vec<SlaCheckResult>,
    pub compliant_count: usize,
    pub violation_count: usize,
    pub overall_compliant: bool,
}

impl SlaReport {
    pub fn new(cluster_id: String, checks: Vec<SlaCheckResult>) -> Self {
        let compliant_count = checks.iter().filter(|c| c.compliant).count();
        let violation_count = checks.iter().filter(|c| !c.compliant).count();
        let overall_compliant = violation_count == 0;

        Self {
            generated_at: current_time_ns(),
            cluster_id,
            checks,
            compliant_count,
            violation_count,
            overall_compliant,
        }
    }

    pub fn summary_line(&self) -> String {
        let total = self.compliant_count + self.violation_count;
        if total == 0 {
            "No SLAs defined".to_string()
        } else {
            format!(
                "{}/{} SLAs met, {} violation{}",
                self.compliant_count,
                total,
                self.violation_count,
                if self.violation_count == 1 { "" } else { "s" }
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_percentiles_empty_returns_none() {
        let result = compute_percentiles(&[]);
        assert!(result.is_none());
    }

    #[test]
    fn test_compute_percentiles_single_element() {
        let result = compute_percentiles(&[100]).unwrap();
        assert_eq!(result.p50, 100.0);
        assert_eq!(result.p95, 100.0);
        assert_eq!(result.p99, 100.0);
        assert_eq!(result.min, 100.0);
        assert_eq!(result.max, 100.0);
        assert_eq!(result.mean, 100.0);
        assert_eq!(result.sample_count, 1);
    }

    #[test]
    fn test_compute_percentiles_sorted_slice() {
        let sorted = vec![10, 20, 30, 40, 50, 60, 70, 80, 90, 100];
        let result = compute_percentiles(&sorted).unwrap();

        assert_eq!(result.min, 10.0);
        assert_eq!(result.max, 100.0);
        assert_eq!(result.mean, 55.0);
    }

    #[test]
    fn test_compute_percentiles_unsorted_same_as_sorted() {
        let unsorted = vec![100, 10, 50, 30, 90, 20, 70, 40, 60, 80];
        let sorted = vec![10, 20, 30, 40, 50, 60, 70, 80, 90, 100];

        let result_unsorted = compute_percentiles(&unsorted).unwrap();
        let result_sorted = compute_percentiles(&sorted).unwrap();

        assert_eq!(result_unsorted.p50, result_sorted.p50);
        assert_eq!(result_unsorted.p99, result_sorted.p99);
        assert_eq!(result_unsorted.mean, result_sorted.mean);
    }

    #[test]
    fn test_compute_percentiles_many_samples() {
        let samples: Vec<u64> = (1..=1000).collect();
        let result = compute_percentiles(&samples).unwrap();

        assert!((result.p50 - 500.0).abs() < 1.0);
        assert!((result.p95 - 950.0).abs() < 2.0);
        assert!((result.p99 - 990.0).abs() < 2.0);
    }

    #[test]
    fn test_sla_window_push_samples() {
        let mut window = SlaWindow::new(10);
        window.push(100, 1000);
        window.push(200, 2000);
        window.push(300, 3000);

        assert_eq!(window.len(), 3);
    }

    #[test]
    fn test_sla_window_max_samples_bounds_ring() {
        let mut window = SlaWindow::new(3);
        window.push(1, 1000);
        window.push(2, 2000);
        window.push(3, 3000);
        window.push(4, 4000);
        window.push(5, 5000);

        assert_eq!(window.len(), 3);
    }

    #[test]
    fn test_sla_window_oldest_newest_timestamps() {
        let mut window = SlaWindow::new(10);
        window.push(100, 1000);
        window.push(200, 2000);
        window.push(300, 3000);

        assert_eq!(window.oldest_timestamp(), Some(1000));
        assert_eq!(window.newest_timestamp(), Some(3000));
    }

    #[test]
    fn test_sla_window_clear_resets() {
        let mut window = SlaWindow::new(10);
        window.push(100, 1000);
        window.clear();

        assert!(window.is_empty());
    }

    #[test]
    fn test_sla_window_compute_returns_percentiles() {
        let mut window = SlaWindow::new(10);
        window.push(100, 1000);
        window.push(200, 2000);
        window.push(300, 3000);

        let result = window.compute();
        assert!(result.is_some());
        assert_eq!(result.unwrap().sample_count, 3);
    }

    #[test]
    fn test_sla_checker_no_violation_when_under_threshold() {
        let mut checker = SlaChecker::new();
        checker.add_target(SlaTarget::new(
            SlaMetricKind::ReadLatencyUs,
            1000.0,
            5000.0,
            10000.0,
            "Read latency SLA",
        ));

        let mut window = SlaWindow::new(100);
        for _ in 0..100 {
            window.push(500, current_time_ns());
        }

        let result = checker.check(&SlaMetricKind::ReadLatencyUs, &window);
        assert!(result.compliant);
        assert!(result.violations.is_empty());
    }

    #[test]
    fn test_sla_checker_p95_violation() {
        let mut checker = SlaChecker::new();
        checker.add_target(SlaTarget::new(
            SlaMetricKind::ReadLatencyUs,
            1000.0,
            2000.0,
            5000.0,
            "Read latency SLA",
        ));

        let mut window = SlaWindow::new(100);
        for i in 0..100 {
            let val = if i < 94 { 1000 } else { 3000 };
            window.push(val, current_time_ns());
        }

        let result = checker.check(&SlaMetricKind::ReadLatencyUs, &window);
        assert!(!result.compliant);
        assert!(result
            .violations
            .iter()
            .any(|v| matches!(v, SlaViolation::P95Exceeded { .. })));
    }

    #[test]
    fn test_sla_checker_p99_violation() {
        let mut checker = SlaChecker::new();
        checker.add_target(SlaTarget::new(
            SlaMetricKind::WriteLatencyUs,
            2000.0,
            5000.0,
            8000.0,
            "Write latency SLA",
        ));

        let mut window = SlaWindow::new(100);
        for i in 0..100 {
            let val = if i < 98 { 1000 } else { 15000 };
            window.push(val, current_time_ns());
        }

        let result = checker.check(&SlaMetricKind::WriteLatencyUs, &window);
        assert!(!result.compliant);
        assert!(result
            .violations
            .iter()
            .any(|v| matches!(v, SlaViolation::P99Exceeded { .. })));
    }

    #[test]
    fn test_sla_report_compliant_count_computed() {
        let checks = vec![
            SlaCheckResult {
                target: SlaMetricKind::ReadLatencyUs,
                percentiles: PercentileResult {
                    p50: 100.0,
                    p95: 200.0,
                    p99: 300.0,
                    p999: 350.0,
                    min: 50.0,
                    max: 400.0,
                    mean: 150.0,
                    sample_count: 10,
                },
                violations: vec![],
                compliant: true,
                checked_at: 0,
            },
            SlaCheckResult {
                target: SlaMetricKind::WriteLatencyUs,
                percentiles: PercentileResult {
                    p50: 100.0,
                    p95: 200.0,
                    p99: 300.0,
                    p999: 350.0,
                    min: 50.0,
                    max: 400.0,
                    mean: 150.0,
                    sample_count: 10,
                },
                violations: vec![SlaViolation::P95Exceeded {
                    actual: 200.0,
                    threshold: 150.0,
                }],
                compliant: false,
                checked_at: 0,
            },
        ];

        let report = SlaReport::new("cluster1".to_string(), checks);
        assert_eq!(report.compliant_count, 1);
        assert_eq!(report.violation_count, 1);
    }

    #[test]
    fn test_sla_report_overall_compliant_false_when_any_violation() {
        let checks = vec![
            SlaCheckResult {
                target: SlaMetricKind::ReadLatencyUs,
                percentiles: PercentileResult {
                    p50: 100.0,
                    p95: 200.0,
                    p99: 300.0,
                    p999: 350.0,
                    min: 50.0,
                    max: 400.0,
                    mean: 150.0,
                    sample_count: 10,
                },
                violations: vec![],
                compliant: true,
                checked_at: 0,
            },
            SlaCheckResult {
                target: SlaMetricKind::WriteLatencyUs,
                percentiles: PercentileResult {
                    p50: 100.0,
                    p95: 200.0,
                    p99: 300.0,
                    p999: 350.0,
                    min: 50.0,
                    max: 400.0,
                    mean: 150.0,
                    sample_count: 10,
                },
                violations: vec![SlaViolation::P95Exceeded {
                    actual: 200.0,
                    threshold: 150.0,
                }],
                compliant: false,
                checked_at: 0,
            },
        ];

        let report = SlaReport::new("cluster1".to_string(), checks);
        assert!(!report.overall_compliant);
    }

    #[test]
    fn test_sla_report_overall_compliant_true_when_all_pass() {
        let checks = vec![
            SlaCheckResult {
                target: SlaMetricKind::ReadLatencyUs,
                percentiles: PercentileResult {
                    p50: 100.0,
                    p95: 200.0,
                    p99: 300.0,
                    p999: 350.0,
                    min: 50.0,
                    max: 400.0,
                    mean: 150.0,
                    sample_count: 10,
                },
                violations: vec![],
                compliant: true,
                checked_at: 0,
            },
            SlaCheckResult {
                target: SlaMetricKind::WriteLatencyUs,
                percentiles: PercentileResult {
                    p50: 100.0,
                    p95: 200.0,
                    p99: 300.0,
                    p999: 350.0,
                    min: 50.0,
                    max: 400.0,
                    mean: 150.0,
                    sample_count: 10,
                },
                violations: vec![],
                compliant: true,
                checked_at: 0,
            },
        ];

        let report = SlaReport::new("cluster1".to_string(), checks);
        assert!(report.overall_compliant);
    }

    #[test]
    fn test_sla_report_summary_line_format() {
        let checks = vec![
            SlaCheckResult {
                target: SlaMetricKind::ReadLatencyUs,
                percentiles: PercentileResult {
                    p50: 100.0,
                    p95: 200.0,
                    p99: 300.0,
                    p999: 350.0,
                    min: 50.0,
                    max: 400.0,
                    mean: 150.0,
                    sample_count: 10,
                },
                violations: vec![],
                compliant: true,
                checked_at: 0,
            },
            SlaCheckResult {
                target: SlaMetricKind::WriteLatencyUs,
                percentiles: PercentileResult {
                    p50: 100.0,
                    p95: 200.0,
                    p99: 300.0,
                    p999: 350.0,
                    min: 50.0,
                    max: 400.0,
                    mean: 150.0,
                    sample_count: 10,
                },
                violations: vec![],
                compliant: true,
                checked_at: 0,
            },
        ];

        let report = SlaReport::new("cluster1".to_string(), checks);
        let summary = report.summary_line();
        assert_eq!(summary, "2/2 SLAs met, 0 violations");
    }

    #[test]
    fn test_sla_report_summary_line_single_violation() {
        let checks = vec![
            SlaCheckResult {
                target: SlaMetricKind::ReadLatencyUs,
                percentiles: PercentileResult {
                    p50: 100.0,
                    p95: 200.0,
                    p99: 300.0,
                    p999: 350.0,
                    min: 50.0,
                    max: 400.0,
                    mean: 150.0,
                    sample_count: 10,
                },
                violations: vec![],
                compliant: true,
                checked_at: 0,
            },
            SlaCheckResult {
                target: SlaMetricKind::WriteLatencyUs,
                percentiles: PercentileResult {
                    p50: 100.0,
                    p95: 200.0,
                    p99: 300.0,
                    p999: 350.0,
                    min: 50.0,
                    max: 400.0,
                    mean: 150.0,
                    sample_count: 10,
                },
                violations: vec![SlaViolation::P95Exceeded {
                    actual: 200.0,
                    threshold: 150.0,
                }],
                compliant: false,
                checked_at: 0,
            },
        ];

        let report = SlaReport::new("cluster1".to_string(), checks);
        let summary = report.summary_line();
        assert_eq!(summary, "1/2 SLAs met, 1 violation");
    }

    #[test]
    fn test_sla_report_summary_line_no_targets() {
        let report = SlaReport::new("cluster1".to_string(), vec![]);
        let summary = report.summary_line();
        assert_eq!(summary, "No SLAs defined");
    }

    #[test]
    fn test_latency_sample_serde_round_trip() {
        let sample = LatencySample::new(1000, 1234567890);
        let json = serde_json::to_string(&sample).unwrap();
        let decoded: LatencySample = serde_json::from_str(&json).unwrap();
        assert_eq!(sample.value_us, decoded.value_us);
        assert_eq!(sample.timestamp, decoded.timestamp);
    }

    #[test]
    fn test_sla_metric_kind_name() {
        assert_eq!(SlaMetricKind::ReadLatencyUs.name(), "read_latency_us");
        assert_eq!(SlaMetricKind::WriteLatencyUs.name(), "write_latency_us");
        assert_eq!(
            SlaMetricKind::MetadataLatencyUs.name(),
            "metadata_latency_us"
        );
        assert_eq!(SlaMetricKind::ThroughputMBps.name(), "throughput_mbps");
        assert_eq!(SlaMetricKind::Iops.name(), "iops");
        assert_eq!(
            SlaMetricKind::AvailabilityPercent.name(),
            "availability_percent"
        );
    }

    #[test]
    fn test_sla_target_new() {
        let target = SlaTarget::new(
            SlaMetricKind::ReadLatencyUs,
            1000.0,
            5000.0,
            10000.0,
            "Test SLA",
        );
        assert_eq!(target.kind, SlaMetricKind::ReadLatencyUs);
        assert_eq!(target.p50_threshold, 1000.0);
        assert_eq!(target.p95_threshold, 5000.0);
        assert_eq!(target.p99_threshold, 10000.0);
        assert_eq!(target.description, "Test SLA");
    }

    #[test]
    fn test_sla_checker_empty_window() {
        let checker = SlaChecker::new();
        let window = SlaWindow::new(10);

        let result = checker.check(&SlaMetricKind::ReadLatencyUs, &window);
        assert!(result.compliant);
    }

    #[test]
    fn test_sla_checker_check_all() {
        let mut checker = SlaChecker::new();
        checker.add_target(SlaTarget::new(
            SlaMetricKind::ReadLatencyUs,
            1000.0,
            5000.0,
            10000.0,
            "Read SLA",
        ));
        checker.add_target(SlaTarget::new(
            SlaMetricKind::WriteLatencyUs,
            1000.0,
            5000.0,
            10000.0,
            "Write SLA",
        ));

        let mut windows = HashMap::new();
        windows.insert(SlaMetricKind::ReadLatencyUs, SlaWindow::new(10));
        windows.insert(SlaMetricKind::WriteLatencyUs, SlaWindow::new(10));

        let results = checker.check_all(&windows);
        assert_eq!(results.len(), 2);
    }
}
