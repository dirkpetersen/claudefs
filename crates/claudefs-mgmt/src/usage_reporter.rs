use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum UsageReporterError {
    #[error("Invalid tenant ID: {0}")]
    InvalidTenantId(String),
    #[error("Storage error: {0}")]
    StorageError(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantUsageSnapshot {
    pub tenant_id: String,
    pub timestamp: DateTime<Utc>,
    pub bytes_used: u64,
    pub files_count: u64,
    pub iops_current: u64,
    pub read_mbps_current: f64,
    pub write_mbps_current: f64,
}

impl TenantUsageSnapshot {
    pub fn new(
        tenant_id: String,
        bytes_used: u64,
        files_count: u64,
        iops_current: u64,
        read_mbps_current: f64,
        write_mbps_current: f64,
    ) -> Self {
        Self {
            tenant_id,
            timestamp: Utc::now(),
            bytes_used,
            files_count,
            iops_current,
            read_mbps_current,
            write_mbps_current,
        }
    }

    pub fn total_iops(&self) -> u64 {
        self.iops_current
    }

    pub fn total_mbps(&self) -> f64 {
        self.read_mbps_current + self.write_mbps_current
    }
}

#[derive(Debug, Clone)]
pub struct BurstAlert {
    pub tenant_id: String,
    pub detected_at: DateTime<Utc>,
    pub baseline_iops: u64,
    pub current_iops: u64,
    pub overage_pct: u32,
}

impl BurstAlert {
    pub fn new(tenant_id: String, baseline_iops: u64, current_iops: u64, overage_pct: u32) -> Self {
        Self {
            tenant_id,
            detected_at: Utc::now(),
            baseline_iops,
            current_iops,
            overage_pct,
        }
    }
}

#[derive(Debug, Clone)]
pub struct BurstDetector {
    baseline_iops: u64,
    burst_threshold_pct: u32,
    window_secs: u64,
    history: VecDeque<(DateTime<Utc>, u64)>,
}

impl BurstDetector {
    pub fn new(baseline_iops: u64, burst_threshold_pct: u32, window_secs: u64) -> Self {
        Self {
            baseline_iops,
            burst_threshold_pct,
            window_secs,
            history: VecDeque::new(),
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(0, 150, 60)
    }

    pub fn update_baseline(&mut self, iops: u64) {
        self.baseline_iops = iops;
        self.history.clear();
    }

    pub fn record_sample(&mut self, timestamp: DateTime<Utc>, iops: u64) {
        self.history.push_back((timestamp, iops));

        let window_start = timestamp - chrono::Duration::seconds(self.window_secs as i64);
        while let Some((ts, _)) = self.history.front() {
            if *ts < window_start {
                self.history.pop_front();
            } else {
                break;
            }
        }
    }

    pub fn detect_burst(&self) -> Option<BurstAlert> {
        if self.baseline_iops == 0 {
            return None;
        }

        let threshold = self.baseline_iops * self.burst_threshold_pct as u64 / 100;

        let avg_iops: u64 = if !self.history.is_empty() {
            let sum: u64 = self.history.iter().map(|(_, iops)| iops).sum();
            sum / self.history.len() as u64
        } else {
            return None;
        };

        if avg_iops > threshold {
            let overage_pct = ((avg_iops as f64 / self.baseline_iops as f64) * 100.0) as u32;
            return Some(BurstAlert {
                tenant_id: String::new(),
                detected_at: Utc::now(),
                baseline_iops: self.baseline_iops,
                current_iops: avg_iops,
                overage_pct,
            });
        }

        None
    }

    pub fn baseline_iops(&self) -> u64 {
        self.baseline_iops
    }

    pub fn threshold(&self) -> u64 {
        self.baseline_iops * self.burst_threshold_pct as u64 / 100
    }

    pub fn sample_count(&self) -> usize {
        self.history.len()
    }
}

impl Default for BurstDetector {
    fn default() -> Self {
        Self::with_defaults()
    }
}

pub struct UsageReporter {
    snapshots: HashMap<String, TenantUsageSnapshot>,
    burst_detectors: HashMap<String, BurstDetector>,
    recent_order: VecDeque<String>,
}

impl UsageReporter {
    pub fn new() -> Self {
        Self {
            snapshots: HashMap::new(),
            burst_detectors: HashMap::new(),
            recent_order: VecDeque::new(),
        }
    }

    pub fn record_snapshot(
        &mut self,
        snapshot: TenantUsageSnapshot,
    ) -> Result<(), UsageReporterError> {
        if snapshot.tenant_id.is_empty() {
            return Err(UsageReporterError::InvalidTenantId(
                "tenant ID cannot be empty".to_string(),
            ));
        }

        let tenant_id = snapshot.tenant_id.clone();

        if let Some(existing) = self.snapshots.get(&tenant_id) {
            let detector = self
                .burst_detectors
                .entry(tenant_id.clone())
                .or_insert_with(|| BurstDetector::with_defaults());
            detector.update_baseline(existing.iops_current);
            detector.record_sample(snapshot.timestamp, snapshot.iops_current);
        } else {
            let mut detector = BurstDetector::with_defaults();
            detector.update_baseline(snapshot.iops_current);
            detector.record_sample(snapshot.timestamp, snapshot.iops_current);
            self.burst_detectors.insert(tenant_id.clone(), detector);
        }

        self.snapshots.insert(tenant_id.clone(), snapshot);

        if let Some(pos) = self.recent_order.iter().position(|t| t == &tenant_id) {
            self.recent_order.remove(pos);
        }
        self.recent_order.push_front(tenant_id);

        Ok(())
    }

    pub fn detect_burst(&self, tenant_id: &str) -> Option<BurstAlert> {
        let detector = self.burst_detectors.get(tenant_id)?;
        let mut alert = detector.detect_burst()?;
        alert.tenant_id = tenant_id.to_string();
        Some(alert)
    }

    pub fn get_latest_snapshot(&self, tenant_id: &str) -> Option<TenantUsageSnapshot> {
        self.snapshots.get(tenant_id).cloned()
    }

    pub fn recent_tenants(&self, limit: usize) -> Vec<String> {
        self.recent_order.iter().take(limit).cloned().collect()
    }

    pub fn all_tenants(&self) -> Vec<String> {
        self.snapshots.keys().cloned().collect()
    }

    pub fn tenant_count(&self) -> usize {
        self.snapshots.len()
    }

    pub fn clear(&mut self) {
        self.snapshots.clear();
        self.burst_detectors.clear();
        self.recent_order.clear();
    }
}

impl Default for UsageReporter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tenant_usage_snapshot_new() {
        let snapshot = TenantUsageSnapshot::new("tenant1".to_string(), 1000, 100, 500, 10.0, 5.0);

        assert_eq!(snapshot.tenant_id, "tenant1");
        assert_eq!(snapshot.bytes_used, 1000);
        assert_eq!(snapshot.files_count, 100);
        assert_eq!(snapshot.iops_current, 500);
        assert_eq!(snapshot.read_mbps_current, 10.0);
        assert_eq!(snapshot.write_mbps_current, 5.0);
    }

    #[test]
    fn test_tenant_usage_snapshot_total_iops() {
        let snapshot = TenantUsageSnapshot::new("tenant1".to_string(), 1000, 100, 500, 10.0, 5.0);

        assert_eq!(snapshot.total_iops(), 500);
    }

    #[test]
    fn test_tenant_usage_snapshot_total_mbps() {
        let snapshot = TenantUsageSnapshot::new("tenant1".to_string(), 1000, 100, 500, 10.0, 5.0);

        assert_eq!(snapshot.total_mbps(), 15.0);
    }

    #[test]
    fn test_burst_detector_default() {
        let detector = BurstDetector::default();
        assert_eq!(detector.baseline_iops(), 0);
        assert_eq!(detector.sample_count(), 0);
    }

    #[test]
    fn test_burst_detector_with_defaults() {
        let detector = BurstDetector::with_defaults();
        assert_eq!(detector.baseline_iops(), 0);
    }

    #[test]
    fn test_burst_detector_update_baseline() {
        let mut detector = BurstDetector::with_defaults();
        detector.update_baseline(1000);
        assert_eq!(detector.baseline_iops(), 1000);
    }

    #[test]
    fn test_burst_detector_record_sample() {
        let mut detector = BurstDetector::with_defaults();
        detector.update_baseline(1000);

        let now = Utc::now();
        detector.record_sample(now, 500);
        detector.record_sample(now + chrono::Duration::seconds(10), 600);

        assert_eq!(detector.sample_count(), 2);
    }

    #[test]
    fn test_burst_detector_threshold() {
        let detector = BurstDetector::new(1000, 150, 60);
        assert_eq!(detector.threshold(), 1500);
    }

    #[test]
    fn test_burst_detector_no_burst_when_below_threshold() {
        let mut detector = BurstDetector::new(1000, 150, 60);

        let now = Utc::now();
        detector.record_sample(now, 1200);

        let alert = detector.detect_burst();
        assert!(alert.is_none());
    }

    #[test]
    fn test_burst_detector_burst_detected() {
        let mut detector = BurstDetector::new(1000, 150, 60);

        let now = Utc::now();
        detector.record_sample(now, 1600);
        detector.record_sample(now + chrono::Duration::seconds(5), 1700);
        detector.record_sample(now + chrono::Duration::seconds(10), 1800);

        let alert = detector.detect_burst();
        assert!(alert.is_some());
        assert!(alert.unwrap().overage_pct >= 150);
    }

    #[test]
    fn test_burst_alert_new() {
        let alert = BurstAlert::new("tenant1".to_string(), 1000, 1800, 180);

        assert_eq!(alert.tenant_id, "tenant1");
        assert_eq!(alert.baseline_iops, 1000);
        assert_eq!(alert.current_iops, 1800);
        assert_eq!(alert.overage_pct, 180);
    }

    #[test]
    fn test_usage_reporter_new() {
        let reporter = UsageReporter::new();
        assert_eq!(reporter.tenant_count(), 0);
    }

    #[test]
    fn test_usage_reporter_record_snapshot() {
        let mut reporter = UsageReporter::new();
        let snapshot = TenantUsageSnapshot::new("tenant1".to_string(), 1000, 100, 500, 10.0, 5.0);

        let result = reporter.record_snapshot(snapshot);
        assert!(result.is_ok());
        assert_eq!(reporter.tenant_count(), 1);
    }

    #[test]
    fn test_usage_reporter_record_snapshot_invalid_tenant() {
        let mut reporter = UsageReporter::new();
        let snapshot = TenantUsageSnapshot::new("".to_string(), 1000, 100, 500, 10.0, 5.0);

        let result = reporter.record_snapshot(snapshot);
        assert!(matches!(
            result,
            Err(UsageReporterError::InvalidTenantId(_))
        ));
    }

    #[test]
    fn test_usage_reporter_get_latest_snapshot() {
        let mut reporter = UsageReporter::new();
        let snapshot = TenantUsageSnapshot::new("tenant1".to_string(), 1000, 100, 500, 10.0, 5.0);

        reporter.record_snapshot(snapshot).unwrap();

        let retrieved = reporter.get_latest_snapshot("tenant1");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().bytes_used, 1000);
    }

    #[test]
    fn test_usage_reporter_get_latest_snapshot_not_found() {
        let reporter = UsageReporter::new();
        let retrieved = reporter.get_latest_snapshot("nonexistent");
        assert!(retrieved.is_none());
    }

    #[test]
    fn test_usage_reporter_detect_burst() {
        let mut reporter = UsageReporter::new();

        let now = Utc::now();
        let snapshot1 = TenantUsageSnapshot {
            tenant_id: "tenant1".to_string(),
            timestamp: now,
            bytes_used: 1000,
            files_count: 100,
            iops_current: 1000,
            read_mbps_current: 10.0,
            write_mbps_current: 5.0,
        };
        reporter.record_snapshot(snapshot1).unwrap();

        let snapshot2 = TenantUsageSnapshot {
            tenant_id: "tenant1".to_string(),
            timestamp: now + chrono::Duration::seconds(30),
            bytes_used: 2000,
            files_count: 200,
            iops_current: 1800,
            read_mbps_current: 20.0,
            write_mbps_current: 10.0,
        };
        reporter.record_snapshot(snapshot2).unwrap();

        let alert = reporter.detect_burst("tenant1");
        assert!(alert.is_some());
    }

    #[test]
    fn test_usage_reporter_recent_tenants() {
        let mut reporter = UsageReporter::new();

        let snapshot1 = TenantUsageSnapshot::new("tenant1".to_string(), 1000, 100, 500, 10.0, 5.0);
        let snapshot2 = TenantUsageSnapshot::new("tenant2".to_string(), 2000, 200, 600, 20.0, 10.0);
        let snapshot3 = TenantUsageSnapshot::new("tenant3".to_string(), 3000, 300, 700, 30.0, 15.0);

        reporter.record_snapshot(snapshot1).unwrap();
        reporter.record_snapshot(snapshot2).unwrap();
        reporter.record_snapshot(snapshot3).unwrap();

        let recent = reporter.recent_tenants(2);
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0], "tenant3");
        assert_eq!(recent[1], "tenant2");
    }

    #[test]
    fn test_usage_reporter_recent_tenants_updates_order() {
        let mut reporter = UsageReporter::new();

        let snapshot1 = TenantUsageSnapshot::new("tenant1".to_string(), 1000, 100, 500, 10.0, 5.0);
        let snapshot2 = TenantUsageSnapshot::new("tenant2".to_string(), 2000, 200, 600, 20.0, 10.0);

        reporter.record_snapshot(snapshot1).unwrap();
        reporter.record_snapshot(snapshot2).unwrap();

        let snapshot1_updated =
            TenantUsageSnapshot::new("tenant1".to_string(), 1500, 150, 550, 15.0, 7.5);
        reporter.record_snapshot(snapshot1_updated).unwrap();

        let recent = reporter.recent_tenants(2);
        assert_eq!(recent[0], "tenant1");
    }

    #[test]
    fn test_usage_reporter_recent_tenants_limit() {
        let mut reporter = UsageReporter::new();

        for i in 0..5 {
            let snapshot = TenantUsageSnapshot::new(
                format!("tenant{}", i),
                1000 * i as u64,
                100,
                500,
                10.0,
                5.0,
            );
            reporter.record_snapshot(snapshot).unwrap();
        }

        let recent = reporter.recent_tenants(3);
        assert_eq!(recent.len(), 3);
    }

    #[test]
    fn test_usage_reporter_all_tenants() {
        let mut reporter = UsageReporter::new();

        let snapshot1 = TenantUsageSnapshot::new("tenant1".to_string(), 1000, 100, 500, 10.0, 5.0);
        let snapshot2 = TenantUsageSnapshot::new("tenant2".to_string(), 2000, 200, 600, 20.0, 10.0);

        reporter.record_snapshot(snapshot1).unwrap();
        reporter.record_snapshot(snapshot2).unwrap();

        let all = reporter.all_tenants();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_usage_reporter_clear() {
        let mut reporter = UsageReporter::new();

        let snapshot = TenantUsageSnapshot::new("tenant1".to_string(), 1000, 100, 500, 10.0, 5.0);
        reporter.record_snapshot(snapshot).unwrap();

        reporter.clear();

        assert_eq!(reporter.tenant_count(), 0);
        assert!(reporter.recent_tenants(10).is_empty());
    }

    #[test]
    fn test_usage_reporter_duplicate_tenant_update() {
        let mut reporter = UsageReporter::new();

        let snapshot1 = TenantUsageSnapshot::new("tenant1".to_string(), 1000, 100, 500, 10.0, 5.0);
        let snapshot2 = TenantUsageSnapshot::new("tenant1".to_string(), 2000, 200, 600, 20.0, 10.0);

        reporter.record_snapshot(snapshot1).unwrap();
        reporter.record_snapshot(snapshot2).unwrap();

        assert_eq!(reporter.tenant_count(), 1);

        let retrieved = reporter.get_latest_snapshot("tenant1").unwrap();
        assert_eq!(retrieved.bytes_used, 2000);
    }

    #[test]
    fn test_usage_reporter_zero_value_snapshot() {
        let mut reporter = UsageReporter::new();
        let snapshot = TenantUsageSnapshot::new("tenant1".to_string(), 0, 0, 0, 0.0, 0.0);

        let result = reporter.record_snapshot(snapshot);
        assert!(result.is_ok());

        let retrieved = reporter.get_latest_snapshot("tenant1").unwrap();
        assert_eq!(retrieved.bytes_used, 0);
    }

    #[test]
    fn test_burst_detector_old_samples_pruned() {
        let mut detector = BurstDetector::new(1000, 150, 60);

        let old_time = Utc::now() - chrono::Duration::seconds(120);
        detector.record_sample(old_time, 2000);
        detector.record_sample(old_time + chrono::Duration::seconds(10), 2000);

        let now = Utc::now();
        detector.record_sample(now, 1200);

        assert!(detector.sample_count() <= 2);
    }

    #[test]
    fn test_burst_detector_zero_baseline_no_burst() {
        let mut detector = BurstDetector::with_defaults();
        let now = Utc::now();
        detector.record_sample(now, 2000);

        let alert = detector.detect_burst();
        assert!(alert.is_none());
    }

    #[test]
    fn test_tenant_usage_snapshot_serialization() {
        let snapshot = TenantUsageSnapshot::new("tenant1".to_string(), 1000, 100, 500, 10.5, 5.5);

        let json = serde_json::to_string(&snapshot).unwrap();
        let decoded: TenantUsageSnapshot = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded.tenant_id, "tenant1");
        assert_eq!(decoded.bytes_used, 1000);
    }
}
