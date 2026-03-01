//! Quality of Service for cross-site replication.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::debug;

/// Workload priority classes for replication traffic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WorkloadClass {
    /// Critical priority - highest QoS tier
    Critical,
    /// High priority - second tier
    High,
    /// Normal priority - default tier
    Normal,
    /// Background priority - lowest tier
    Background,
}

impl WorkloadClass {
    /// Returns the priority value for this workload class.
    /// Higher values indicate higher priority.
    pub fn priority(&self) -> u8 {
        match self {
            WorkloadClass::Critical => 100,
            WorkloadClass::High => 75,
            WorkloadClass::Normal => 50,
            WorkloadClass::Background => 25,
        }
    }
}

/// Bandwidth allocation for a specific workload class.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BandwidthAllocation {
    /// Maximum bytes per second allowed for this class.
    pub max_bytes_per_sec: u64,
    /// Burst allocation in bytes.
    pub burst_bytes: u64,
    /// The workload class this allocation applies to.
    pub workload_class: WorkloadClass,
}

/// QoS policy defining bandwidth allocations for all workload classes.
#[derive(Debug, Clone)]
pub struct QosPolicy {
    /// Bandwidth allocations per workload class.
    pub allocations: HashMap<WorkloadClass, BandwidthAllocation>,
    /// Total bandwidth limit across all classes in bytes/sec.
    pub total_bandwidth_limit: u64,
}

impl QosPolicy {
    /// Creates a new QoS policy with default allocations.
    ///
    /// Default distribution:
    /// - Critical: 40% of total
    /// - High: 30% of total
    /// - Normal: 20% of total
    /// - Background: 10% of total
    ///
    /// Each class gets burst_bytes = 2x max_bytes_per_sec.
    pub fn new(total_bw: u64) -> Self {
        let mut allocations = HashMap::new();

        let critical_bw = (total_bw as f64 * 0.40) as u64;
        allocations.insert(
            WorkloadClass::Critical,
            BandwidthAllocation {
                max_bytes_per_sec: critical_bw,
                burst_bytes: critical_bw * 2,
                workload_class: WorkloadClass::Critical,
            },
        );

        let high_bw = (total_bw as f64 * 0.30) as u64;
        allocations.insert(
            WorkloadClass::High,
            BandwidthAllocation {
                max_bytes_per_sec: high_bw,
                burst_bytes: high_bw * 2,
                workload_class: WorkloadClass::High,
            },
        );

        let normal_bw = (total_bw as f64 * 0.20) as u64;
        allocations.insert(
            WorkloadClass::Normal,
            BandwidthAllocation {
                max_bytes_per_sec: normal_bw,
                burst_bytes: normal_bw * 2,
                workload_class: WorkloadClass::Normal,
            },
        );

        let background_bw = (total_bw as f64 * 0.10) as u64;
        allocations.insert(
            WorkloadClass::Background,
            BandwidthAllocation {
                max_bytes_per_sec: background_bw,
                burst_bytes: background_bw * 2,
                workload_class: WorkloadClass::Background,
            },
        );

        Self {
            allocations,
            total_bandwidth_limit: total_bw,
        }
    }

    /// Sets the bandwidth allocation for a specific workload class.
    pub fn set_allocation(&mut self, class: WorkloadClass, alloc: BandwidthAllocation) {
        self.allocations.insert(class, alloc);
    }

    /// Gets the bandwidth allocation for a specific workload class.
    pub fn get_allocation(&self, class: &WorkloadClass) -> Option<&BandwidthAllocation> {
        self.allocations.get(class)
    }
}

/// Token representing granted bandwidth allocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QosToken {
    /// Number of bytes allowed in this token.
    pub bytes_allowed: u64,
    /// The workload class this token is for.
    pub class: WorkloadClass,
    /// Timestamp when token was issued (in nanoseconds).
    pub issued_at_ns: u64,
}

/// QoS scheduler for managing bandwidth allocation across workload classes.
#[derive(Debug)]
pub struct QosScheduler {
    /// The QoS policy being enforced.
    policy: QosPolicy,
    /// Bytes used in current window per workload class.
    bytes_used: HashMap<WorkloadClass, u64>,
    /// Start time of current window in nanoseconds.
    window_start_ns: u64,
}

impl QosScheduler {
    /// Creates a new QoS scheduler with the given policy.
    pub fn new(policy: QosPolicy) -> Self {
        Self {
            policy,
            bytes_used: HashMap::new(),
            window_start_ns: 0,
        }
    }

    /// Requests bandwidth for a workload class.
    ///
    /// Returns a token indicating how many bytes are allowed.
    /// If the window has expired (1 second), it resets.
    pub fn request_bandwidth(
        &mut self,
        class: WorkloadClass,
        requested_bytes: u64,
        now_ns: u64,
    ) -> QosToken {
        if now_ns - self.window_start_ns >= 1_000_000_000 {
            self.bytes_used.clear();
            self.window_start_ns = now_ns;
        }

        let max = self
            .policy
            .get_allocation(&class)
            .map(|a| a.max_bytes_per_sec)
            .unwrap_or(0);

        let used = *self.bytes_used.get(&class).unwrap_or(&0);
        let available = max.saturating_sub(used);
        let bytes_allowed = requested_bytes.min(available);

        if bytes_allowed < requested_bytes {
            debug!(
                "Bandwidth capped for {:?}: requested={}, allowed={}, available={}",
                class, requested_bytes, bytes_allowed, available
            );
        }

        *self.bytes_used.entry(class).or_insert(0) += bytes_allowed;

        QosToken {
            bytes_allowed,
            class,
            issued_at_ns: now_ns,
        }
    }

    /// Returns the utilization fraction (0.0 to 1.0) for a workload class.
    ///
    /// Returns 0.0 if max_bytes_per_sec is 0.
    pub fn utilization(&self, class: WorkloadClass) -> f64 {
        let max = self
            .policy
            .get_allocation(&class)
            .map(|a| a.max_bytes_per_sec)
            .unwrap_or(0);

        if max == 0 {
            return 0.0;
        }

        let used = *self.bytes_used.get(&class).unwrap_or(&0);
        (used as f64 / max as f64).clamp(0.0, 1.0)
    }

    /// Returns a reference to the QoS policy.
    pub fn policy(&self) -> &QosPolicy {
        &self.policy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workload_class_priority_critical() {
        assert_eq!(WorkloadClass::Critical.priority(), 100);
    }

    #[test]
    fn workload_class_priority_high() {
        assert_eq!(WorkloadClass::High.priority(), 75);
    }

    #[test]
    fn workload_class_priority_normal() {
        assert_eq!(WorkloadClass::Normal.priority(), 50);
    }

    #[test]
    fn workload_class_priority_background() {
        assert_eq!(WorkloadClass::Background.priority(), 25);
    }

    #[test]
    fn qos_policy_new_total_bw() {
        let policy = QosPolicy::new(1000);
        assert_eq!(policy.total_bandwidth_limit, 1000);
    }

    #[test]
    fn qos_policy_new_critical_allocation() {
        let policy = QosPolicy::new(1000);
        let critical = policy.get_allocation(&WorkloadClass::Critical).unwrap();
        assert_eq!(critical.max_bytes_per_sec, 400);
        assert_eq!(critical.burst_bytes, 800);
    }

    #[test]
    fn qos_policy_new_background_allocation() {
        let policy = QosPolicy::new(1000);
        let background = policy.get_allocation(&WorkloadClass::Background).unwrap();
        assert_eq!(background.max_bytes_per_sec, 100);
        assert_eq!(background.burst_bytes, 200);
    }

    #[test]
    fn qos_policy_set_allocation() {
        let mut policy = QosPolicy::new(1000);
        let new_alloc = BandwidthAllocation {
            max_bytes_per_sec: 500,
            burst_bytes: 1000,
            workload_class: WorkloadClass::Critical,
        };
        policy.set_allocation(WorkloadClass::Critical, new_alloc);
        let critical = policy.get_allocation(&WorkloadClass::Critical).unwrap();
        assert_eq!(critical.max_bytes_per_sec, 500);
    }

    #[test]
    fn qos_policy_get_allocation() {
        let policy = QosPolicy::new(1000);
        assert!(policy.get_allocation(&WorkloadClass::Critical).is_some());
        assert!(policy.get_allocation(&WorkloadClass::High).is_some());
        assert!(policy.get_allocation(&WorkloadClass::Normal).is_some());
        assert!(policy.get_allocation(&WorkloadClass::Background).is_some());
    }

    #[test]
    fn scheduler_request_full_budget() {
        let policy = QosPolicy::new(1000);
        let mut scheduler = QosScheduler::new(policy);
        let token = scheduler.request_bandwidth(WorkloadClass::Critical, 100, 0);
        assert_eq!(token.bytes_allowed, 100);
    }

    #[test]
    fn scheduler_request_capped_at_budget() {
        let policy = QosPolicy::new(100);
        let mut scheduler = QosScheduler::new(policy);
        let _ = scheduler.request_bandwidth(WorkloadClass::Critical, 60, 0);
        let token = scheduler.request_bandwidth(WorkloadClass::Critical, 60, 0);
        assert_eq!(token.bytes_allowed, 40);
    }

    #[test]
    fn scheduler_window_resets_after_one_second() {
        let policy = QosPolicy::new(100);
        let mut scheduler = QosScheduler::new(policy);
        let _ = scheduler.request_bandwidth(WorkloadClass::Critical, 100, 0);
        let token = scheduler.request_bandwidth(WorkloadClass::Critical, 100, 1_000_000_001);
        assert_eq!(token.bytes_allowed, 100);
    }

    #[test]
    fn scheduler_consecutive_requests_deplete_budget() {
        let policy = QosPolicy::new(100);
        let mut scheduler = QosScheduler::new(policy);
        let t1 = scheduler.request_bandwidth(WorkloadClass::High, 30, 0);
        assert_eq!(t1.bytes_allowed, 30);
        let t2 = scheduler.request_bandwidth(WorkloadClass::High, 30, 0);
        assert_eq!(t2.bytes_allowed, 30);
        let t3 = scheduler.request_bandwidth(WorkloadClass::High, 30, 0);
        assert_eq!(t3.bytes_allowed, 30);
        let t4 = scheduler.request_bandwidth(WorkloadClass::High, 30, 0);
        assert_eq!(t4.bytes_allowed, 10);
    }

    #[test]
    fn scheduler_utilization_zero_when_unused() {
        let policy = QosPolicy::new(1000);
        let scheduler = QosScheduler::new(policy);
        assert_eq!(scheduler.utilization(WorkloadClass::Critical), 0.0);
    }

    #[test]
    fn scheduler_utilization_correct_fraction() {
        let policy = QosPolicy::new(100);
        let mut scheduler = QosScheduler::new(policy);
        let _ = scheduler.request_bandwidth(WorkloadClass::Normal, 50, 0);
        assert!((scheduler.utilization(WorkloadClass::Normal) - 0.5).abs() < 0.001);
    }

    #[test]
    fn scheduler_utilization_clamped_at_one() {
        let policy = QosPolicy::new(100);
        let mut scheduler = QosScheduler::new(policy);
        let _ = scheduler.request_bandwidth(WorkloadClass::Background, 150, 0);
        assert!((scheduler.utilization(WorkloadClass::Background) - 1.0).abs() < 0.001);
    }

    #[test]
    fn scheduler_critical_more_than_background() {
        let policy = QosPolicy::new(1000);
        let mut scheduler = QosScheduler::new(policy);
        let critical_token = scheduler.request_bandwidth(WorkloadClass::Critical, 1000, 0);
        scheduler = QosScheduler::new(QosPolicy::new(1000));
        let bg_token = scheduler.request_bandwidth(WorkloadClass::Background, 1000, 0);
        assert!(critical_token.bytes_allowed > bg_token.bytes_allowed);
    }

    #[test]
    fn scheduler_token_fields() {
        let policy = QosPolicy::new(1000);
        let mut scheduler = QosScheduler::new(policy);
        let token = scheduler.request_bandwidth(WorkloadClass::High, 500, 1234567890);
        assert_eq!(token.bytes_allowed, 500);
        assert_eq!(token.class, WorkloadClass::High);
        assert_eq!(token.issued_at_ns, 1234567890);
    }

    #[test]
    fn scheduler_policy_getter() {
        let policy = QosPolicy::new(1000);
        let scheduler = QosScheduler::new(policy);
        assert_eq!(scheduler.policy().total_bandwidth_limit, 1000);
    }

    #[test]
    fn scheduler_multiple_classes_independent() {
        let policy = QosPolicy::new(1000);
        let mut scheduler = QosScheduler::new(policy);
        let _ = scheduler.request_bandwidth(WorkloadClass::Critical, 400, 0);
        let _ = scheduler.request_bandwidth(WorkloadClass::Background, 100, 0);
        assert!((scheduler.utilization(WorkloadClass::Critical) - 1.0).abs() < 0.001);
        assert!((scheduler.utilization(WorkloadClass::Background) - 1.0).abs() < 0.001);
        let normal_util = scheduler.utilization(WorkloadClass::Normal);
        assert!((normal_util - 0.0).abs() < 0.001);
    }
}
