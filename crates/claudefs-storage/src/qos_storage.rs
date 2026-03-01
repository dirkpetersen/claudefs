//! Storage-Level Quality of Service Enforcement.
//!
//! This module enforces bandwidth/IOPS limits per workload class at the storage layer.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[allow(unused_imports)]
use crate::error::{StorageError, StorageResult};

/// Workload classification for QoS enforcement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WorkloadClass {
    /// AI training workloads - high throughput, large I/O sizes.
    AiTraining,
    /// Database workloads - low latency, high IOPS.
    Database,
    /// Streaming workloads - sequential access, high bandwidth.
    Streaming,
    /// Backup workloads - large sequential I/O, can be throttled.
    Backup,
    /// Interactive workloads - user-facing, latency sensitive.
    Interactive,
    /// Best effort - no guarantees, runs when resources available.
    BestEffort,
}

impl std::fmt::Display for WorkloadClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WorkloadClass::AiTraining => write!(f, "AiTraining"),
            WorkloadClass::Database => write!(f, "Database"),
            WorkloadClass::Streaming => write!(f, "Streaming"),
            WorkloadClass::Backup => write!(f, "Backup"),
            WorkloadClass::Interactive => write!(f, "Interactive"),
            WorkloadClass::BestEffort => write!(f, "BestEffort"),
        }
    }
}

/// QoS policy for a tenant/workload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QosPolicy {
    /// Workload class for this policy.
    pub class: WorkloadClass,
    /// Maximum IOPS allowed (None = unlimited).
    pub max_iops: Option<u64>,
    /// Maximum bandwidth in MB/s (None = unlimited).
    pub max_bandwidth_mbps: Option<u64>,
    /// Minimum guaranteed IOPS.
    pub min_iops: Option<u64>,
    /// Minimum guaranteed bandwidth in MB/s.
    pub min_bandwidth_mbps: Option<u64>,
    /// Priority (0-255, higher = more important).
    pub priority: u8,
    /// Burst IOPS allowance above max.
    pub burst_iops: Option<u64>,
}

impl Default for QosPolicy {
    fn default() -> Self {
        Self {
            class: WorkloadClass::BestEffort,
            max_iops: None,
            max_bandwidth_mbps: None,
            min_iops: None,
            min_bandwidth_mbps: None,
            priority: 128,
            burst_iops: None,
        }
    }
}

/// Token bucket for rate limiting.
#[derive(Debug, Clone)]
pub struct TokenBucket {
    /// Maximum capacity of the bucket.
    capacity: u64,
    /// Current number of tokens in the bucket.
    tokens: f64,
    /// Refill rate in tokens per second.
    refill_rate: f64,
    /// Timestamp of last refill in nanoseconds.
    last_refill_ns: u64,
}

impl TokenBucket {
    /// Creates a new token bucket with the given capacity and rate.
    pub fn new(capacity: u64, rate: f64) -> Self {
        Self {
            capacity,
            tokens: capacity as f64,
            refill_rate: rate,
            last_refill_ns: u64::MAX,
        }
    }

    /// Tries to consume tokens from the bucket.
    /// Returns true if consumption was successful, false otherwise.
    pub fn try_consume(&mut self, tokens: u64, now_ns: u64) -> bool {
        self.refill(now_ns);

        if self.tokens >= tokens as f64 {
            self.tokens -= tokens as f64;
            true
        } else {
            false
        }
    }

    /// Returns the available tokens in the bucket.
    pub fn available(&self) -> u64 {
        self.tokens as u64
    }

    /// Refills the token bucket based on elapsed time.
    pub fn refill(&mut self, now_ns: u64) {
        if self.last_refill_ns == u64::MAX {
            self.last_refill_ns = now_ns;
            return;
        }

        let elapsed_ns = now_ns.saturating_sub(self.last_refill_ns);
        let elapsed_secs = elapsed_ns as f64 / 1_000_000_000.0;

        let new_tokens = elapsed_secs * self.refill_rate;
        self.tokens = (self.tokens + new_tokens).min(self.capacity as f64);
        self.last_refill_ns = now_ns;
    }
}

/// Bandwidth tracker for measuring throughput.
#[derive(Debug, Clone)]
pub struct BandwidthTracker {
    /// Bytes transferred in current window.
    window_bytes: u64,
    /// Start time of current window in nanoseconds.
    window_start_ns: u64,
    /// Window duration in nanoseconds (default 1 second).
    window_duration_ns: u64,
}

impl BandwidthTracker {
    /// Creates a new bandwidth tracker with the given window duration.
    pub fn new(window_ns: u64) -> Self {
        Self {
            window_bytes: 0,
            window_start_ns: u64::MAX,
            window_duration_ns: window_ns,
        }
    }

    /// Records bytes transferred.
    pub fn record(&mut self, bytes: u64, now_ns: u64) {
        self.reset_if_expired(now_ns);
        self.window_bytes = self.window_bytes.saturating_add(bytes);
    }

    /// Returns current bandwidth in MB/s.
    pub fn current_mbps(&self, _now_ns: u64) -> f64 {
        if self.window_start_ns == u64::MAX {
            return 0.0;
        }

        let window_duration_secs = self.window_duration_ns as f64 / 1_000_000_000.0;
        if window_duration_secs <= 0.0 {
            return 0.0;
        }

        (self.window_bytes as f64 / (1024.0 * 1024.0)) / window_duration_secs
    }

    /// Resets the window if it has expired.
    pub fn reset_if_expired(&mut self, now_ns: u64) {
        if self.window_start_ns == u64::MAX {
            self.window_start_ns = now_ns;
            return;
        }

        let elapsed = now_ns.saturating_sub(self.window_start_ns);
        if elapsed >= self.window_duration_ns {
            self.window_bytes = 0;
            self.window_start_ns = now_ns;
        }
    }
}

/// Decision returned by QoS enforcer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum QosDecision {
    /// Request is allowed to proceed.
    Allow,
    /// Request should be throttled.
    Throttle {
        /// Recommended delay in nanoseconds.
        delay_ns: u64,
    },
    /// Request should be rejected.
    Reject {
        /// Reason for rejection.
        reason: String,
    },
}

/// Type of I/O operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IoType {
    /// Read operation.
    Read,
    /// Write operation.
    Write,
    /// Metadata operation (stat, lookup, etc.).
    Metadata,
}

/// I/O request for QoS checking.
#[derive(Debug, Clone)]
pub struct IoRequest {
    /// Tenant identifier.
    pub tenant_id: String,
    /// Workload class.
    pub class: WorkloadClass,
    /// Type of I/O operation.
    pub op_type: IoType,
    /// Size in bytes.
    pub bytes: u64,
    /// Request timestamp in nanoseconds.
    pub timestamp_ns: u64,
}

/// Statistics for QoS enforcer.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QosEnforcerStats {
    /// Total requests processed.
    pub total_requests: u64,
    /// Requests allowed.
    pub allowed_requests: u64,
    /// Requests throttled.
    pub throttled_requests: u64,
    /// Requests rejected.
    pub rejected_requests: u64,
    /// Total bytes processed.
    pub total_bytes_processed: u64,
    /// Total delay applied in nanoseconds.
    pub total_delay_ns: u64,
}

/// QoS enforcer for storage-level quality of service.
pub struct QosEnforcer {
    /// Policies keyed by tenant ID.
    policies: HashMap<String, QosPolicy>,
    /// IOPS token buckets keyed by tenant ID.
    iops_buckets: HashMap<String, TokenBucket>,
    /// Bandwidth trackers keyed by tenant ID.
    bw_trackers: HashMap<String, BandwidthTracker>,
    /// Statistics.
    stats: QosEnforcerStats,
}

impl QosEnforcer {
    /// Creates a new QoS enforcer.
    pub fn new() -> Self {
        Self {
            policies: HashMap::new(),
            iops_buckets: HashMap::new(),
            bw_trackers: HashMap::new(),
            stats: QosEnforcerStats::default(),
        }
    }

    /// Sets or updates a QoS policy for a tenant.
    pub fn set_policy(&mut self, tenant_id: String, policy: QosPolicy) {
        if let Some(max_iops) = policy.max_iops {
            let burst_capacity = policy.burst_iops.unwrap_or(max_iops);
            let rate = max_iops as f64;
            self.iops_buckets
                .insert(tenant_id.clone(), TokenBucket::new(burst_capacity, rate));
        } else {
            self.iops_buckets.remove(&tenant_id);
        }

        self.bw_trackers
            .insert(tenant_id.clone(), BandwidthTracker::new(1_000_000_000));

        self.policies.insert(tenant_id, policy);
        tracing::debug!("QoS policy set for tenant");
    }

    /// Removes a QoS policy for a tenant.
    pub fn remove_policy(&mut self, tenant_id: &str) {
        self.policies.remove(tenant_id);
        self.iops_buckets.remove(tenant_id);
        self.bw_trackers.remove(tenant_id);
        tracing::debug!(tenant_id = %tenant_id, "QoS policy removed");
    }

    /// Checks if a request should be allowed, throttled, or rejected.
    pub fn check_request(&mut self, request: &IoRequest) -> QosDecision {
        self.stats.total_requests += 1;

        let policy = match self.policies.get(&request.tenant_id) {
            Some(p) => p,
            None => {
                self.stats.rejected_requests += 1;
                return QosDecision::Reject {
                    reason: "No QoS policy defined for tenant".to_string(),
                };
            }
        };

        if let Some(bucket) = self.iops_buckets.get_mut(&request.tenant_id) {
            if !bucket.try_consume(1, request.timestamp_ns) {
                let available = bucket.available();
                if available > 0 {
                    let delay_ns = ((1 - available) as f64 / policy.max_iops.unwrap_or(1) as f64
                        * 1_000_000_000.0) as u64;
                    self.stats.throttled_requests += 1;
                    return QosDecision::Throttle { delay_ns };
                } else {
                    self.stats.throttled_requests += 1;
                    return QosDecision::Throttle {
                        delay_ns: 1_000_000_000,
                    };
                }
            }
        }

        if let Some(max_bw) = policy.max_bandwidth_mbps {
            let tracker = self.bw_trackers.get_mut(&request.tenant_id);
            if let Some(t) = tracker {
                let current_mbps = t.current_mbps(request.timestamp_ns);
                if current_mbps >= max_bw as f64 {
                    let delay_ns = (request.bytes as f64 / (max_bw as f64 * 1024.0 * 1024.0)
                        * 1_000_000_000.0) as u64;
                    self.stats.throttled_requests += 1;
                    return QosDecision::Throttle { delay_ns };
                }
                t.record(request.bytes, request.timestamp_ns);
            }
        }

        self.stats.allowed_requests += 1;
        QosDecision::Allow
    }

    /// Records completion of a request for tracking.
    pub fn record_completion(&mut self, tenant_id: &str, bytes: u64, _duration_ns: u64) {
        self.stats.total_bytes_processed = self.stats.total_bytes_processed.saturating_add(bytes);

        if let Some(tracker) = self.bw_trackers.get_mut(tenant_id) {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos() as u64;
            tracker.record(bytes, now);
        }
    }

    /// Gets the QoS policy for a tenant.
    pub fn get_policy(&self, tenant_id: &str) -> Option<&QosPolicy> {
        self.policies.get(tenant_id)
    }

    /// Returns the current bandwidth in MB/s for a tenant.
    pub fn tenant_bandwidth_mbps(&self, tenant_id: &str) -> f64 {
        let tracker = self.bw_trackers.get(tenant_id);
        if let Some(t) = tracker {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos() as u64;
            t.current_mbps(now)
        } else {
            0.0
        }
    }

    /// Returns a reference to the QoS enforcer statistics.
    pub fn stats(&self) -> &QosEnforcerStats {
        &self.stats
    }

    /// Resets all statistics to zero.
    pub fn reset_stats(&mut self) {
        self.stats = QosEnforcerStats::default();
        tracing::debug!("QoS stats reset");
    }
}

impl Default for QosEnforcer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NS_PER_SEC: u64 = 1_000_000_000;

    #[test]
    fn test_workload_class_display() {
        assert_eq!(format!("{}", WorkloadClass::AiTraining), "AiTraining");
        assert_eq!(format!("{}", WorkloadClass::Database), "Database");
        assert_eq!(format!("{}", WorkloadClass::Streaming), "Streaming");
        assert_eq!(format!("{}", WorkloadClass::Backup), "Backup");
        assert_eq!(format!("{}", WorkloadClass::Interactive), "Interactive");
        assert_eq!(format!("{}", WorkloadClass::BestEffort), "BestEffort");
    }

    #[test]
    fn test_qos_policy_defaults() {
        let policy = QosPolicy::default();
        assert_eq!(policy.class, WorkloadClass::BestEffort);
        assert_eq!(policy.max_iops, None);
        assert_eq!(policy.max_bandwidth_mbps, None);
        assert_eq!(policy.priority, 128);
        assert_eq!(policy.burst_iops, None);
    }

    #[test]
    fn test_token_bucket_new() {
        let bucket = TokenBucket::new(100, 10.0);
        assert_eq!(bucket.available(), 100);
    }

    #[test]
    fn test_token_bucket_consume() {
        let mut bucket = TokenBucket::new(100, 10.0);
        assert!(bucket.try_consume(30, 0));
        assert_eq!(bucket.available(), 70);
    }

    #[test]
    fn test_token_bucket_empty() {
        let mut bucket = TokenBucket::new(10, 1.0);
        assert!(bucket.try_consume(5, 0));
        assert!(bucket.try_consume(5, 0));
        assert!(!bucket.try_consume(1, 0));
    }

    #[test]
    fn test_token_bucket_refill() {
        let mut bucket = TokenBucket::new(100, 10.0);
        bucket.try_consume(100, 0);
        assert_eq!(bucket.available(), 0);

        bucket.refill(NS_PER_SEC);
        let available = bucket.available();
        assert!(available >= 9 && available <= 11);
    }

    #[test]
    fn test_token_bucket_burst() {
        let bucket = TokenBucket::new(100, 10.0);
        assert_eq!(bucket.available(), 100);
    }

    #[test]
    fn test_bandwidth_tracker_new() {
        let tracker = BandwidthTracker::new(NS_PER_SEC);
        assert_eq!(tracker.current_mbps(0), 0.0);
    }

    #[test]
    fn test_bandwidth_tracker_record() {
        let mut tracker = BandwidthTracker::new(NS_PER_SEC);
        tracker.record(1024 * 1024, NS_PER_SEC);
        let mbps = tracker.current_mbps(NS_PER_SEC);
        assert!(mbps > 0.0);
    }

    #[test]
    fn test_bandwidth_tracker_mbps() {
        let mut tracker = BandwidthTracker::new(NS_PER_SEC);
        tracker.record(1024 * 1024, NS_PER_SEC / 2);
        let mbps = tracker.current_mbps(NS_PER_SEC);
        assert!((mbps - 1.0).abs() < 0.1);
    }

    #[test]
    fn test_bandwidth_tracker_window_reset() {
        let mut tracker = BandwidthTracker::new(NS_PER_SEC);
        tracker.record(1024, 0);
        tracker.reset_if_expired(NS_PER_SEC + 1);
        assert_eq!(tracker.current_mbps(NS_PER_SEC + 1), 0.0);
    }

    #[test]
    fn test_qos_enforcer_new() {
        let enforcer = QosEnforcer::new();
        assert_eq!(enforcer.stats().total_requests, 0);
    }

    #[test]
    fn test_set_policy() {
        let mut enforcer = QosEnforcer::new();
        let policy = QosPolicy {
            class: WorkloadClass::Database,
            max_iops: Some(1000),
            max_bandwidth_mbps: Some(100),
            priority: 200,
            ..Default::default()
        };
        enforcer.set_policy("tenant1".to_string(), policy);

        let stored = enforcer.get_policy("tenant1").unwrap();
        assert_eq!(stored.class, WorkloadClass::Database);
        assert_eq!(stored.max_iops, Some(1000));
    }

    #[test]
    fn test_remove_policy() {
        let mut enforcer = QosEnforcer::new();
        let policy = QosPolicy::default();
        enforcer.set_policy("tenant1".to_string(), policy);
        enforcer.remove_policy("tenant1");
        assert!(enforcer.get_policy("tenant1").is_none());
    }

    #[test]
    fn test_check_allow() {
        let mut enforcer = QosEnforcer::new();
        let policy = QosPolicy {
            max_iops: Some(10000),
            max_bandwidth_mbps: Some(1000),
            ..Default::default()
        };
        enforcer.set_policy("tenant1".to_string(), policy);

        let request = IoRequest {
            tenant_id: "tenant1".to_string(),
            class: WorkloadClass::Database,
            op_type: IoType::Read,
            bytes: 4096,
            timestamp_ns: 0,
        };

        let decision = enforcer.check_request(&request);
        assert!(matches!(decision, QosDecision::Allow));
    }

    #[test]
    fn test_check_throttle() {
        let mut enforcer = QosEnforcer::new();
        let policy = QosPolicy {
            max_iops: Some(1),
            burst_iops: Some(1),
            ..Default::default()
        };
        enforcer.set_policy("tenant1".to_string(), policy);

        let request = IoRequest {
            tenant_id: "tenant1".to_string(),
            class: WorkloadClass::Database,
            op_type: IoType::Read,
            bytes: 4096,
            timestamp_ns: 0,
        };

        enforcer.check_request(&request);

        let request2 = IoRequest {
            tenant_id: "tenant1".to_string(),
            class: WorkloadClass::Database,
            op_type: IoType::Read,
            bytes: 4096,
            timestamp_ns: 1000,
        };

        let decision = enforcer.check_request(&request2);
        assert!(matches!(decision, QosDecision::Throttle { .. }));
    }

    #[test]
    fn test_check_reject_no_policy() {
        let mut enforcer = QosEnforcer::new();

        let request = IoRequest {
            tenant_id: "unknown".to_string(),
            class: WorkloadClass::Database,
            op_type: IoType::Read,
            bytes: 4096,
            timestamp_ns: 0,
        };

        let decision = enforcer.check_request(&request);
        assert!(matches!(decision, QosDecision::Reject { .. }));
    }

    #[test]
    fn test_record_completion() {
        let mut enforcer = QosEnforcer::new();
        let policy = QosPolicy::default();
        enforcer.set_policy("tenant1".to_string(), policy);

        enforcer.record_completion("tenant1", 4096, 1000);

        assert_eq!(enforcer.stats().total_bytes_processed, 4096);
    }

    #[test]
    fn test_bandwidth_limit() {
        let mut enforcer = QosEnforcer::new();
        let policy = QosPolicy {
            max_iops: Some(100000),
            max_bandwidth_mbps: Some(1),
            ..Default::default()
        };
        enforcer.set_policy("tenant1".to_string(), policy);

        let mut request = IoRequest {
            tenant_id: "tenant1".to_string(),
            class: WorkloadClass::Streaming,
            op_type: IoType::Read,
            bytes: 10 * 1024 * 1024,
            timestamp_ns: 0,
        };

        let decision1 = enforcer.check_request(&request);
        assert!(matches!(decision1, QosDecision::Allow));

        request.timestamp_ns = 100_000;
        let decision2 = enforcer.check_request(&request);

        if let QosDecision::Throttle { .. } = decision2 {
            assert!(true);
        } else {
            assert!(false, "Expected throttle decision");
        }
    }

    #[test]
    fn test_priority_ordering() {
        let mut enforcer = QosEnforcer::new();

        let mut policy1 = QosPolicy::default();
        policy1.priority = 100;
        policy1.max_iops = Some(1000);
        enforcer.set_policy("tenant1".to_string(), policy1);

        let mut policy2 = QosPolicy::default();
        policy2.priority = 200;
        policy2.max_iops = Some(1000);
        enforcer.set_policy("tenant2".to_string(), policy2);

        let p1 = enforcer.get_policy("tenant1").unwrap();
        let p2 = enforcer.get_policy("tenant2").unwrap();

        assert!(p2.priority > p1.priority);
    }

    #[test]
    fn test_stats_counting() {
        let mut enforcer = QosEnforcer::new();
        let policy = QosPolicy {
            max_iops: Some(100000),
            max_bandwidth_mbps: Some(1000),
            ..Default::default()
        };
        enforcer.set_policy("tenant1".to_string(), policy);

        let request = IoRequest {
            tenant_id: "tenant1".to_string(),
            class: WorkloadClass::Database,
            op_type: IoType::Read,
            bytes: 4096,
            timestamp_ns: 0,
        };

        enforcer.check_request(&request);

        let stats = enforcer.stats();
        assert_eq!(stats.total_requests, 1);
        assert_eq!(stats.allowed_requests, 1);
    }

    #[test]
    fn test_reset_stats() {
        let mut enforcer = QosEnforcer::new();
        let policy = QosPolicy {
            max_iops: Some(100000),
            max_bandwidth_mbps: Some(1000),
            ..Default::default()
        };
        enforcer.set_policy("tenant1".to_string(), policy);

        let request = IoRequest {
            tenant_id: "tenant1".to_string(),
            class: WorkloadClass::Database,
            op_type: IoType::Read,
            bytes: 4096,
            timestamp_ns: 0,
        };

        enforcer.check_request(&request);
        enforcer.reset_stats();

        assert_eq!(enforcer.stats().total_requests, 0);
        assert_eq!(enforcer.stats().allowed_requests, 0);
    }

    #[test]
    fn test_tenant_bandwidth_mbps() {
        let mut enforcer = QosEnforcer::new();
        let policy = QosPolicy::default();
        enforcer.set_policy("tenant1".to_string(), policy);

        enforcer.record_completion("tenant1", 1024 * 1024, 1000);

        let mbps = enforcer.tenant_bandwidth_mbps("tenant1");
        assert!(mbps >= 0.0);
    }
}
