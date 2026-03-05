use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PerformanceTrackerError {
    #[error("Storage error: {0}")]
    StorageError(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OperationType {
    Read,
    Write,
    MetadataOp,
    Snapshot,
    Scan,
}

impl OperationType {
    pub fn name(&self) -> &'static str {
        match self {
            OperationType::Read => "read",
            OperationType::Write => "write",
            OperationType::MetadataOp => "metadata",
            OperationType::Snapshot => "snapshot",
            OperationType::Scan => "scan",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatencySample {
    pub op_type: OperationType,
    pub latency_us: u64,
    pub tenant_id: String,
    pub timestamp: DateTime<Utc>,
}

impl LatencySample {
    pub fn new(op_type: OperationType, latency_us: u64, tenant_id: String) -> Self {
        Self {
            op_type,
            latency_us,
            tenant_id,
            timestamp: Utc::now(),
        }
    }

    pub fn for_tenant(op_type: OperationType, latency_us: u64, tenant_id: &str) -> Self {
        Self::new(op_type, latency_us, tenant_id.to_string())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PercentileBucket {
    pub p50: u64,
    pub p90: u64,
    pub p99: u64,
    pub p99_9: u64,
}

impl PercentileBucket {
    pub fn from_samples(samples: &[u64]) -> Self {
        if samples.is_empty() {
            return Self::default();
        }

        let mut sorted = samples.to_vec();
        sorted.sort();

        let len = sorted.len();
        let p50_idx = ((len - 1) as f64 * 0.50) as usize;
        let p90_idx = ((len - 1) as f64 * 0.90) as usize;
        let p99_idx = ((len - 1) as f64 * 0.99) as usize;
        let p99_9_idx = (len as f64 * 0.999) as usize;

        Self {
            p50: sorted[p50_idx.min(len - 1)],
            p90: sorted[p90_idx.min(len - 1)],
            p99: sorted[p99_idx.min(len - 1)],
            p99_9: sorted[p99_9_idx.min(len - 1)],
        }
    }

    pub fn max(&self) -> u64 {
        self.p99_9
    }

    pub fn min(&self) -> u64 {
        self.p50
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SlaComplianceStatus {
    pub percentage_meeting_sla: f64,
    pub total_samples: usize,
    pub meeting_sla: usize,
    pub sla_us: u64,
}

impl SlaComplianceStatus {
    pub fn new(
        percentage_meeting_sla: f64,
        total_samples: usize,
        meeting_sla: usize,
        sla_us: u64,
    ) -> Self {
        Self {
            percentage_meeting_sla,
            total_samples,
            meeting_sla,
            sla_us,
        }
    }

    pub fn compliant(&self) -> bool {
        self.percentage_meeting_sla >= 95.0
    }
}

pub struct PerformanceTracker {
    global_samples: HashMap<OperationType, VecDeque<LatencySample>>,
    tenant_samples: HashMap<String, HashMap<OperationType, VecDeque<LatencySample>>>,
    max_samples_per_type: usize,
    max_samples_per_tenant_per_type: usize,
}

impl PerformanceTracker {
    pub fn new() -> Self {
        Self {
            global_samples: HashMap::new(),
            tenant_samples: HashMap::new(),
            max_samples_per_type: 10000,
            max_samples_per_tenant_per_type: 5000,
        }
    }

    pub fn with_limits(max_global: usize, max_tenant: usize) -> Self {
        Self {
            global_samples: HashMap::new(),
            tenant_samples: HashMap::new(),
            max_samples_per_type: max_global,
            max_samples_per_tenant_per_type: max_tenant,
        }
    }

    pub fn record_sample(&mut self, sample: LatencySample) -> Result<(), PerformanceTrackerError> {
        let op_type = sample.op_type;
        let tenant_id = sample.tenant_id.clone();

        let global_queue = self
            .global_samples
            .entry(op_type)
            .or_insert_with(VecDeque::new);
        if global_queue.len() >= self.max_samples_per_type {
            global_queue.pop_front();
        }
        global_queue.push_back(sample.clone());

        let tenant_ops = self
            .tenant_samples
            .entry(tenant_id.clone())
            .or_insert_with(HashMap::new);
        let tenant_queue = tenant_ops.entry(op_type).or_insert_with(VecDeque::new);
        if tenant_queue.len() >= self.max_samples_per_tenant_per_type {
            tenant_queue.pop_front();
        }
        tenant_queue.push_back(sample);

        Ok(())
    }

    pub fn get_percentiles(&self, op_type: OperationType) -> Option<PercentileBucket> {
        let samples = self.global_samples.get(&op_type)?;
        if samples.is_empty() {
            return None;
        }

        let latencies: Vec<u64> = samples.iter().map(|s| s.latency_us).collect();
        Some(PercentileBucket::from_samples(&latencies))
    }

    pub fn get_tenant_percentiles(
        &self,
        tenant_id: &str,
        op_type: OperationType,
    ) -> Option<PercentileBucket> {
        let tenant_ops = self.tenant_samples.get(tenant_id)?;
        let samples = tenant_ops.get(&op_type)?;
        if samples.is_empty() {
            return None;
        }

        let latencies: Vec<u64> = samples.iter().map(|s| s.latency_us).collect();
        Some(PercentileBucket::from_samples(&latencies))
    }

    pub fn check_sla_compliance(&self, op_type: OperationType, sla_us: u64) -> SlaComplianceStatus {
        let samples = match self.global_samples.get(&op_type) {
            Some(s) => s,
            None => return SlaComplianceStatus::new(0.0, 0, 0, sla_us),
        };

        let total = samples.len();
        if total == 0 {
            return SlaComplianceStatus::new(0.0, 0, 0, sla_us);
        }

        let meeting = samples.iter().filter(|s| s.latency_us <= sla_us).count();
        let percentage = (meeting as f64 / total as f64) * 100.0;

        SlaComplianceStatus::new(percentage, total, meeting, sla_us)
    }

    pub fn check_tenant_sla_compliance(
        &self,
        tenant_id: &str,
        op_type: OperationType,
        sla_us: u64,
    ) -> SlaComplianceStatus {
        let tenant_ops = match self.tenant_samples.get(tenant_id) {
            Some(o) => o,
            None => return SlaComplianceStatus::new(0.0, 0, 0, sla_us),
        };

        let samples = match tenant_ops.get(&op_type) {
            Some(s) => s,
            None => return SlaComplianceStatus::new(0.0, 0, 0, sla_us),
        };

        let total = samples.len();
        if total == 0 {
            return SlaComplianceStatus::new(0.0, 0, 0, sla_us);
        }

        let meeting = samples.iter().filter(|s| s.latency_us <= sla_us).count();
        let percentage = (meeting as f64 / total as f64) * 100.0;

        SlaComplianceStatus::new(percentage, total, meeting, sla_us)
    }

    pub fn operation_types(&self) -> Vec<OperationType> {
        self.global_samples.keys().cloned().collect()
    }

    pub fn sample_count(&self, op_type: OperationType) -> usize {
        self.global_samples
            .get(&op_type)
            .map(|v| v.len())
            .unwrap_or(0)
    }

    pub fn tenant_sample_count(&self, tenant_id: &str, op_type: OperationType) -> usize {
        self.tenant_samples
            .get(tenant_id)
            .and_then(|ops| ops.get(&op_type))
            .map(|v| v.len())
            .unwrap_or(0)
    }

    pub fn tenant_ids(&self) -> Vec<String> {
        self.tenant_samples.keys().cloned().collect()
    }

    pub fn clear_operation(&mut self, op_type: OperationType) {
        self.global_samples.remove(&op_type);
        for tenant_ops in self.tenant_samples.values_mut() {
            tenant_ops.remove(&op_type);
        }
    }

    pub fn clear_tenant(&mut self, tenant_id: &str) {
        self.tenant_samples.remove(tenant_id);
    }

    pub fn clear_all(&mut self) {
        self.global_samples.clear();
        self.tenant_samples.clear();
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
    fn test_operation_type_name() {
        assert_eq!(OperationType::Read.name(), "read");
        assert_eq!(OperationType::Write.name(), "write");
        assert_eq!(OperationType::MetadataOp.name(), "metadata");
        assert_eq!(OperationType::Snapshot.name(), "snapshot");
        assert_eq!(OperationType::Scan.name(), "scan");
    }

    #[test]
    fn test_latency_sample_new() {
        let sample = LatencySample::new(OperationType::Read, 1000, "tenant1".to_string());

        assert_eq!(sample.op_type, OperationType::Read);
        assert_eq!(sample.latency_us, 1000);
        assert_eq!(sample.tenant_id, "tenant1");
    }

    #[test]
    fn test_latency_sample_for_tenant() {
        let sample = LatencySample::for_tenant(OperationType::Write, 500, "tenant1");

        assert_eq!(sample.op_type, OperationType::Write);
        assert_eq!(sample.latency_us, 500);
        assert_eq!(sample.tenant_id, "tenant1");
    }

    #[test]
    fn test_percentile_bucket_from_samples_empty() {
        let bucket = PercentileBucket::from_samples(&[]);
        assert_eq!(bucket.p50, 0);
        assert_eq!(bucket.p90, 0);
    }

    #[test]
    fn test_percentile_bucket_from_samples_single() {
        let bucket = PercentileBucket::from_samples(&[1000]);
        assert_eq!(bucket.p50, 1000);
        assert_eq!(bucket.p90, 1000);
        assert_eq!(bucket.p99, 1000);
    }

    #[test]
    fn test_percentile_bucket_from_samples_multiple() {
        let samples = vec![100, 200, 300, 400, 500, 600, 700, 800, 900, 1000];
        let bucket = PercentileBucket::from_samples(&samples);

        assert_eq!(bucket.p50, 500);
        assert!(bucket.p90 <= 900);
        assert!(bucket.p99 <= 1000);
    }

    #[test]
    fn test_percentile_bucket_max() {
        let bucket = PercentileBucket::from_samples(&[100, 500, 1000]);
        assert_eq!(bucket.max(), 1000);
    }

    #[test]
    fn test_percentile_bucket_min() {
        let bucket = PercentileBucket::from_samples(&[100, 500, 1000]);
        assert_eq!(bucket.min(), 500);
    }

    #[test]
    fn test_sla_compliance_status_new() {
        let status = SlaComplianceStatus::new(95.5, 100, 95, 1000);

        assert_eq!(status.percentage_meeting_sla, 95.5);
        assert_eq!(status.total_samples, 100);
        assert_eq!(status.meeting_sla, 95);
        assert_eq!(status.sla_us, 1000);
    }

    #[test]
    fn test_sla_compliance_status_compliant() {
        let status = SlaComplianceStatus::new(95.0, 100, 95, 1000);
        assert!(status.compliant());

        let status = SlaComplianceStatus::new(94.9, 100, 94, 1000);
        assert!(!status.compliant());
    }

    #[test]
    fn test_performance_tracker_new() {
        let tracker = PerformanceTracker::new();
        assert!(tracker.operation_types().is_empty());
    }

    #[test]
    fn test_performance_tracker_record_sample() {
        let mut tracker = PerformanceTracker::new();
        let sample = LatencySample::new(OperationType::Read, 1000, "tenant1".to_string());

        let result = tracker.record_sample(sample);
        assert!(result.is_ok());
    }

    #[test]
    fn test_performance_tracker_get_percentiles() {
        let mut tracker = PerformanceTracker::new();

        for i in 1..=100 {
            let sample = LatencySample::new(OperationType::Read, i * 100, "tenant1".to_string());
            tracker.record_sample(sample).unwrap();
        }

        let percentiles = tracker.get_percentiles(OperationType::Read);
        assert!(percentiles.is_some());

        let p = percentiles.unwrap();
        assert!(p.p50 <= p.p90);
        assert!(p.p90 <= p.p99);
        assert!(p.p99 <= p.p99_9);
    }

    #[test]
    fn test_performance_tracker_get_percentiles_empty() {
        let tracker = PerformanceTracker::new();
        let percentiles = tracker.get_percentiles(OperationType::Read);
        assert!(percentiles.is_none());
    }

    #[test]
    fn test_performance_tracker_get_tenant_percentiles() {
        let mut tracker = PerformanceTracker::new();

        for i in 1..=50 {
            let sample = LatencySample::new(OperationType::Write, i * 100, "tenant1".to_string());
            tracker.record_sample(sample).unwrap();
        }

        let percentiles = tracker.get_tenant_percentiles("tenant1", OperationType::Write);
        assert!(percentiles.is_some());
    }

    #[test]
    fn test_performance_tracker_get_tenant_percentiles_not_found() {
        let tracker = PerformanceTracker::new();
        let percentiles = tracker.get_tenant_percentiles("nonexistent", OperationType::Read);
        assert!(percentiles.is_none());
    }

    #[test]
    fn test_performance_tracker_check_sla_compliance() {
        let mut tracker = PerformanceTracker::new();

        for _ in 0..100 {
            let sample = LatencySample::new(OperationType::Read, 500, "tenant1".to_string());
            tracker.record_sample(sample).unwrap();
        }

        let status = tracker.check_sla_compliance(OperationType::Read, 1000);
        assert_eq!(status.total_samples, 100);
        assert_eq!(status.meeting_sla, 100);
        assert!(status.compliant());
    }

    #[test]
    fn test_performance_tracker_check_sla_compliance_not_met() {
        let mut tracker = PerformanceTracker::new();

        for i in 0..100 {
            let latency = if i < 90 { 500 } else { 2000 };
            let sample = LatencySample::new(OperationType::Read, latency, "tenant1".to_string());
            tracker.record_sample(sample).unwrap();
        }

        let status = tracker.check_sla_compliance(OperationType::Read, 1000);
        assert_eq!(status.total_samples, 100);
        assert_eq!(status.meeting_sla, 90);
        assert!(!status.compliant());
    }

    #[test]
    fn test_performance_tracker_check_sla_compliance_empty() {
        let tracker = PerformanceTracker::new();
        let status = tracker.check_sla_compliance(OperationType::Read, 1000);

        assert_eq!(status.total_samples, 0);
        assert_eq!(status.percentage_meeting_sla, 0.0);
    }

    #[test]
    fn test_performance_tracker_check_tenant_sla_compliance() {
        let mut tracker = PerformanceTracker::new();

        for _ in 0..50 {
            let sample = LatencySample::new(OperationType::Write, 500, "tenant1".to_string());
            tracker.record_sample(sample).unwrap();
        }

        let status = tracker.check_tenant_sla_compliance("tenant1", OperationType::Write, 1000);
        assert!(status.compliant());
    }

    #[test]
    fn test_performance_tracker_operation_types() {
        let mut tracker = PerformanceTracker::new();

        tracker
            .record_sample(LatencySample::new(
                OperationType::Read,
                100,
                "t1".to_string(),
            ))
            .unwrap();
        tracker
            .record_sample(LatencySample::new(
                OperationType::Write,
                200,
                "t1".to_string(),
            ))
            .unwrap();

        let types = tracker.operation_types();
        assert_eq!(types.len(), 2);
    }

    #[test]
    fn test_performance_tracker_sample_count() {
        let mut tracker = PerformanceTracker::new();

        for _ in 0..10 {
            tracker
                .record_sample(LatencySample::new(
                    OperationType::Read,
                    100,
                    "t1".to_string(),
                ))
                .unwrap();
        }

        assert_eq!(tracker.sample_count(OperationType::Read), 10);
        assert_eq!(tracker.sample_count(OperationType::Write), 0);
    }

    #[test]
    fn test_performance_tracker_tenant_sample_count() {
        let mut tracker = PerformanceTracker::new();

        for _ in 0..5 {
            tracker
                .record_sample(LatencySample::new(
                    OperationType::Read,
                    100,
                    "tenant1".to_string(),
                ))
                .unwrap();
        }

        assert_eq!(
            tracker.tenant_sample_count("tenant1", OperationType::Read),
            5
        );
    }

    #[test]
    fn test_performance_tracker_tenant_ids() {
        let mut tracker = PerformanceTracker::new();

        tracker
            .record_sample(LatencySample::new(
                OperationType::Read,
                100,
                "tenant1".to_string(),
            ))
            .unwrap();
        tracker
            .record_sample(LatencySample::new(
                OperationType::Read,
                200,
                "tenant2".to_string(),
            ))
            .unwrap();

        let ids = tracker.tenant_ids();
        assert_eq!(ids.len(), 2);
    }

    #[test]
    fn test_performance_tracker_clear_operation() {
        let mut tracker = PerformanceTracker::new();

        tracker
            .record_sample(LatencySample::new(
                OperationType::Read,
                100,
                "t1".to_string(),
            ))
            .unwrap();
        tracker
            .record_sample(LatencySample::new(
                OperationType::Write,
                200,
                "t1".to_string(),
            ))
            .unwrap();

        tracker.clear_operation(OperationType::Read);

        assert_eq!(tracker.sample_count(OperationType::Read), 0);
        assert_eq!(tracker.sample_count(OperationType::Write), 1);
    }

    #[test]
    fn test_performance_tracker_clear_tenant() {
        let mut tracker = PerformanceTracker::new();

        tracker
            .record_sample(LatencySample::new(
                OperationType::Read,
                100,
                "tenant1".to_string(),
            ))
            .unwrap();
        tracker
            .record_sample(LatencySample::new(
                OperationType::Read,
                200,
                "tenant2".to_string(),
            ))
            .unwrap();

        tracker.clear_tenant("tenant1");

        let ids = tracker.tenant_ids();
        assert!(!ids.contains(&"tenant1".to_string()));
    }

    #[test]
    fn test_performance_tracker_clear_all() {
        let mut tracker = PerformanceTracker::new();

        tracker
            .record_sample(LatencySample::new(
                OperationType::Read,
                100,
                "tenant1".to_string(),
            ))
            .unwrap();
        tracker
            .record_sample(LatencySample::new(
                OperationType::Write,
                200,
                "tenant2".to_string(),
            ))
            .unwrap();

        tracker.clear_all();

        assert!(tracker.operation_types().is_empty());
        assert!(tracker.tenant_ids().is_empty());
    }

    #[test]
    fn test_performance_tracker_multiple_operation_types() {
        let mut tracker = PerformanceTracker::new();

        tracker
            .record_sample(LatencySample::new(
                OperationType::Read,
                100,
                "t1".to_string(),
            ))
            .unwrap();
        tracker
            .record_sample(LatencySample::new(
                OperationType::Write,
                200,
                "t1".to_string(),
            ))
            .unwrap();
        tracker
            .record_sample(LatencySample::new(
                OperationType::MetadataOp,
                50,
                "t1".to_string(),
            ))
            .unwrap();

        let read_p = tracker.get_percentiles(OperationType::Read);
        let write_p = tracker.get_percentiles(OperationType::Write);
        let meta_p = tracker.get_percentiles(OperationType::MetadataOp);

        assert!(read_p.is_some());
        assert!(write_p.is_some());
        assert!(meta_p.is_some());
    }

    #[test]
    fn test_performance_tracker_sample_limit() {
        let mut tracker = PerformanceTracker::with_limits(5, 5);

        for i in 0..10 {
            tracker
                .record_sample(LatencySample::new(
                    OperationType::Read,
                    i * 100,
                    "t1".to_string(),
                ))
                .unwrap();
        }

        assert_eq!(tracker.sample_count(OperationType::Read), 5);
    }

    #[test]
    fn test_performance_tracker_single_sample_percentiles() {
        let mut tracker = PerformanceTracker::new();
        tracker
            .record_sample(LatencySample::new(
                OperationType::Read,
                1000,
                "t1".to_string(),
            ))
            .unwrap();

        let p = tracker.get_percentiles(OperationType::Read).unwrap();
        assert_eq!(p.p50, 1000);
        assert_eq!(p.p90, 1000);
    }

    #[test]
    fn test_performance_tracker_all_same_value() {
        let mut tracker = PerformanceTracker::new();

        for _ in 0..100 {
            tracker
                .record_sample(LatencySample::new(
                    OperationType::Read,
                    500,
                    "t1".to_string(),
                ))
                .unwrap();
        }

        let p = tracker.get_percentiles(OperationType::Read).unwrap();
        assert_eq!(p.p50, 500);
        assert_eq!(p.p90, 500);
        assert_eq!(p.p99, 500);
    }

    #[test]
    fn test_performance_tracker_extreme_outliers() {
        let mut tracker = PerformanceTracker::new();

        for i in 0..100 {
            let latency = if i < 98 { 100 } else { 1000000 };
            tracker
                .record_sample(LatencySample::new(
                    OperationType::Read,
                    latency,
                    "t1".to_string(),
                ))
                .unwrap();
        }

        let p = tracker.get_percentiles(OperationType::Read).unwrap();
        assert!(p.p99 > 100);
    }

    #[test]
    fn test_sla_compliance_95_percent_reads_under_1ms() {
        let mut tracker = PerformanceTracker::new();

        for i in 0..100 {
            let latency = if i < 95 { 800 } else { 2000 };
            tracker
                .record_sample(LatencySample::new(
                    OperationType::Read,
                    latency,
                    "t1".to_string(),
                ))
                .unwrap();
        }

        let status = tracker.check_sla_compliance(OperationType::Read, 1000);
        assert_eq!(status.percentage_meeting_sla, 95.0);
    }

    #[test]
    fn test_tenant_sla_compliance_independent() {
        let mut tracker = PerformanceTracker::new();

        for _ in 0..50 {
            tracker
                .record_sample(LatencySample::new(
                    OperationType::Read,
                    500,
                    "tenant1".to_string(),
                ))
                .unwrap();
            tracker
                .record_sample(LatencySample::new(
                    OperationType::Read,
                    2000,
                    "tenant2".to_string(),
                ))
                .unwrap();
        }

        let status1 = tracker.check_tenant_sla_compliance("tenant1", OperationType::Read, 1000);
        let status2 = tracker.check_tenant_sla_compliance("tenant2", OperationType::Read, 1000);

        assert!(status1.compliant());
        assert!(!status2.compliant());
    }
}
