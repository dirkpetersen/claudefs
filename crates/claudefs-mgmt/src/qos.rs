use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub enum QosPriority {
    Background = 0,
    Low = 1,
    Normal = 2,
    High = 3,
    Critical = 4,
}

impl QosPriority {
    pub fn weight(&self) -> u32 {
        match self {
            QosPriority::Background => 1,
            QosPriority::Low => 5,
            QosPriority::Normal => 20,
            QosPriority::High => 50,
            QosPriority::Critical => 100,
        }
    }

    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(QosPriority::Background),
            1 => Some(QosPriority::Low),
            2 => Some(QosPriority::Normal),
            3 => Some(QosPriority::High),
            4 => Some(QosPriority::Critical),
            _ => None,
        }
    }
}

impl Default for QosPriority {
    fn default() -> Self {
        QosPriority::Normal
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BandwidthLimit {
    pub read_mbps: Option<u64>,
    pub write_mbps: Option<u64>,
    pub iops_read: Option<u64>,
    pub iops_write: Option<u64>,
}

impl BandwidthLimit {
    pub fn unlimited() -> Self {
        Self {
            read_mbps: None,
            write_mbps: None,
            iops_read: None,
            iops_write: None,
        }
    }

    pub fn read_only_mbps(mbps: u64) -> Self {
        Self {
            read_mbps: Some(mbps),
            write_mbps: None,
            iops_read: None,
            iops_write: None,
        }
    }

    pub fn symmetric_mbps(mbps: u64) -> Self {
        Self {
            read_mbps: Some(mbps),
            write_mbps: Some(mbps),
            iops_read: None,
            iops_write: None,
        }
    }

    pub fn symmetric_iops(iops: u64) -> Self {
        Self {
            read_mbps: None,
            write_mbps: None,
            iops_read: Some(iops),
            iops_write: Some(iops),
        }
    }

    pub fn is_unlimited(&self) -> bool {
        self.read_mbps.is_none()
            && self.write_mbps.is_none()
            && self.iops_read.is_none()
            && self.iops_write.is_none()
    }
}

impl Default for BandwidthLimit {
    fn default() -> Self {
        Self::unlimited()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QosPolicy {
    pub policy_id: String,
    pub name: String,
    pub priority: QosPriority,
    pub limits: BandwidthLimit,
    pub burst_multiplier: f64,
    pub created_at: u64,
}

impl QosPolicy {
    pub fn new(
        policy_id: impl Into<String>,
        name: impl Into<String>,
        priority: QosPriority,
        limits: BandwidthLimit,
    ) -> Self {
        Self {
            policy_id: policy_id.into(),
            name: name.into(),
            priority,
            limits,
            burst_multiplier: 1.0,
            created_at: current_time_ns(),
        }
    }

    pub fn with_burst(mut self, multiplier: f64) -> Self {
        self.burst_multiplier = multiplier;
        self
    }

    pub fn effective_read_mbps(&self) -> Option<u64> {
        self.limits
            .read_mbps
            .map(|m| (m as f64 * self.burst_multiplier) as u64)
    }

    pub fn effective_write_mbps(&self) -> Option<u64> {
        self.limits
            .write_mbps
            .map(|m| (m as f64 * self.burst_multiplier) as u64)
    }

    pub fn effective_iops_read(&self) -> Option<u64> {
        self.limits
            .iops_read
            .map(|i| (i as f64 * self.burst_multiplier) as u64)
    }

    pub fn effective_iops_write(&self) -> Option<u64> {
        self.limits
            .iops_write
            .map(|i| (i as f64 * self.burst_multiplier) as u64)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SubjectKind {
    Tenant,
    ClientIp,
    User,
    Group,
}

impl SubjectKind {
    pub fn name(&self) -> &'static str {
        match self {
            SubjectKind::Tenant => "tenant",
            SubjectKind::ClientIp => "client_ip",
            SubjectKind::User => "user",
            SubjectKind::Group => "group",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QosAssignment {
    pub subject_id: String,
    pub subject_kind: SubjectKind,
    pub policy_id: String,
    pub assigned_at: u64,
}

impl QosAssignment {
    pub fn new(subject_id: String, subject_kind: SubjectKind, policy_id: String) -> Self {
        Self {
            subject_id,
            subject_kind,
            policy_id,
            assigned_at: current_time_ns(),
        }
    }
}

pub struct TokenBucket {
    capacity: u64,
    tokens: f64,
    refill_rate: f64,
}

impl TokenBucket {
    pub fn new(capacity: u64, refill_rate: f64) -> Self {
        Self {
            capacity,
            tokens: capacity as f64,
            refill_rate,
        }
    }

    pub fn try_consume(&mut self, n: u64, elapsed_secs: f64) -> bool {
        self.refill(elapsed_secs);

        if self.tokens >= n as f64 {
            self.tokens -= n as f64;
            true
        } else {
            false
        }
    }

    fn refill(&mut self, elapsed_secs: f64) {
        let new_tokens = self.refill_rate * elapsed_secs;
        self.tokens = (self.tokens + new_tokens).min(self.capacity as f64);
    }

    pub fn fill_level(&self) -> f64 {
        (self.tokens / self.capacity as f64).clamp(0.0, 1.0)
    }

    pub fn reset(&mut self) {
        self.tokens = self.capacity as f64;
    }

    pub fn tokens(&self) -> f64 {
        self.tokens
    }

    pub fn capacity(&self) -> u64 {
        self.capacity
    }
}

#[derive(Debug, Error)]
pub enum QosError {
    #[error("Policy not found: {0}")]
    PolicyNotFound(String),
    #[error("Policy already exists: {0}")]
    PolicyAlreadyExists(String),
    #[error("Assignment not found: {0}")]
    AssignmentNotFound(String),
}

pub struct QosRegistry {
    policies: HashMap<String, QosPolicy>,
    assignments: HashMap<String, QosAssignment>,
}

impl QosRegistry {
    pub fn new() -> Self {
        Self {
            policies: HashMap::new(),
            assignments: HashMap::new(),
        }
    }

    pub fn add_policy(&mut self, policy: QosPolicy) -> Result<(), QosError> {
        if self.policies.contains_key(&policy.policy_id) {
            return Err(QosError::PolicyAlreadyExists(policy.policy_id));
        }
        self.policies.insert(policy.policy_id.clone(), policy);
        Ok(())
    }

    pub fn remove_policy(&mut self, id: &str) -> Result<(), QosError> {
        if self.policies.remove(id).is_none() {
            return Err(QosError::PolicyNotFound(id.to_string()));
        }
        Ok(())
    }

    pub fn get_policy(&self, id: &str) -> Option<&QosPolicy> {
        self.policies.get(id)
    }

    pub fn assign(
        &mut self,
        subject_id: String,
        subject_kind: SubjectKind,
        policy_id: String,
    ) -> Result<(), QosError> {
        if !self.policies.contains_key(&policy_id) {
            return Err(QosError::PolicyNotFound(policy_id));
        }

        let assignment = QosAssignment::new(subject_id.clone(), subject_kind, policy_id);
        self.assignments.insert(subject_id, assignment);
        Ok(())
    }

    pub fn unassign(&mut self, subject_id: &str) -> Result<(), QosError> {
        if self.assignments.remove(subject_id).is_none() {
            return Err(QosError::AssignmentNotFound(subject_id.to_string()));
        }
        Ok(())
    }

    pub fn get_assignment(&self, subject_id: &str) -> Option<&QosAssignment> {
        self.assignments.get(subject_id)
    }

    pub fn effective_policy(&self, subject_id: &str) -> Option<&QosPolicy> {
        let assignment = self.assignments.get(subject_id)?;
        self.policies.get(&assignment.policy_id)
    }

    pub fn policy_count(&self) -> usize {
        self.policies.len()
    }

    pub fn assignment_count(&self) -> usize {
        self.assignments.len()
    }

    pub fn assignments_for_policy(&self, policy_id: &str) -> Vec<&QosAssignment> {
        self.assignments
            .values()
            .filter(|a| a.policy_id == policy_id)
            .collect()
    }

    pub fn list_policies(&self) -> Vec<&QosPolicy> {
        self.policies.values().collect()
    }
}

impl Default for QosRegistry {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_qos_priority_ordering() {
        assert!(QosPriority::Critical > QosPriority::High);
        assert!(QosPriority::High > QosPriority::Normal);
        assert!(QosPriority::Normal > QosPriority::Low);
        assert!(QosPriority::Low > QosPriority::Background);
    }

    #[test]
    fn test_qos_priority_weight_critical() {
        assert_eq!(QosPriority::Critical.weight(), 100);
    }

    #[test]
    fn test_qos_priority_weight_high() {
        assert_eq!(QosPriority::High.weight(), 50);
    }

    #[test]
    fn test_qos_priority_weight_normal() {
        assert_eq!(QosPriority::Normal.weight(), 20);
    }

    #[test]
    fn test_qos_priority_weight_low() {
        assert_eq!(QosPriority::Low.weight(), 5);
    }

    #[test]
    fn test_qos_priority_weight_background() {
        assert_eq!(QosPriority::Background.weight(), 1);
    }

    #[test]
    fn test_qos_priority_from_u8() {
        assert_eq!(QosPriority::from_u8(0), Some(QosPriority::Background));
        assert_eq!(QosPriority::from_u8(4), Some(QosPriority::Critical));
        assert_eq!(QosPriority::from_u8(5), None);
    }

    #[test]
    fn test_bandwidth_limit_unlimited() {
        let limit = BandwidthLimit::unlimited();
        assert!(limit.is_unlimited());
        assert!(limit.read_mbps.is_none());
        assert!(limit.write_mbps.is_none());
    }

    #[test]
    fn test_bandwidth_limit_symmetric_mbps() {
        let limit = BandwidthLimit::symmetric_mbps(1000);
        assert!(!limit.is_unlimited());
        assert_eq!(limit.read_mbps, Some(1000));
        assert_eq!(limit.write_mbps, Some(1000));
    }

    #[test]
    fn test_bandwidth_limit_read_only() {
        let limit = BandwidthLimit::read_only_mbps(500);
        assert_eq!(limit.read_mbps, Some(500));
        assert!(limit.write_mbps.is_none());
    }

    #[test]
    fn test_bandwidth_limit_is_unlimited_only_when_all_none() {
        let limit = BandwidthLimit::symmetric_mbps(100);
        assert!(!limit.is_unlimited());

        let limit2 = BandwidthLimit {
            read_mbps: None,
            write_mbps: None,
            iops_read: Some(100),
            iops_write: None,
        };
        assert!(!limit2.is_unlimited());

        let limit3 = BandwidthLimit::unlimited();
        assert!(limit3.is_unlimited());
    }

    #[test]
    fn test_qos_policy_burst_multiplier() {
        let policy = QosPolicy::new(
            "p1",
            "Test Policy",
            QosPriority::High,
            BandwidthLimit::symmetric_mbps(100),
        )
        .with_burst(1.5);

        assert_eq!(policy.burst_multiplier, 1.5);
    }

    #[test]
    fn test_qos_policy_effective_read_mbps() {
        let policy = QosPolicy::new(
            "p1",
            "Test Policy",
            QosPriority::Normal,
            BandwidthLimit::symmetric_mbps(100),
        )
        .with_burst(2.0);

        assert_eq!(policy.effective_read_mbps(), Some(200));
    }

    #[test]
    fn test_qos_policy_effective_write_mbps() {
        let policy = QosPolicy::new(
            "p1",
            "Test Policy",
            QosPriority::Normal,
            BandwidthLimit::symmetric_mbps(100),
        )
        .with_burst(1.5);

        assert_eq!(policy.effective_write_mbps(), Some(150));
    }

    #[test]
    fn test_qos_policy_effective_mbps_none_when_unlimited() {
        let policy = QosPolicy::new(
            "p1",
            "Test Policy",
            QosPriority::Normal,
            BandwidthLimit::unlimited(),
        );

        assert!(policy.effective_read_mbps().is_none());
        assert!(policy.effective_write_mbps().is_none());
    }

    #[test]
    fn test_token_bucket_new_starts_full() {
        let bucket = TokenBucket::new(1000, 100.0);
        assert_eq!(bucket.tokens(), 1000.0);
        assert!(bucket.fill_level() >= 1.0);
    }

    #[test]
    fn test_token_bucket_try_consume_succeeds() {
        let mut bucket = TokenBucket::new(1000, 100.0);
        let result = bucket.try_consume(100, 0.0);
        assert!(result);
    }

    #[test]
    fn test_token_bucket_try_consume_fails_insufficient() {
        let mut bucket = TokenBucket::new(100, 0.0);
        let result = bucket.try_consume(200, 0.0);
        assert!(!result);
    }

    #[test]
    fn test_token_bucket_refill() {
        let mut bucket = TokenBucket::new(1000, 100.0);
        bucket.try_consume(500, 0.0);
        assert_eq!(bucket.tokens(), 500.0);

        bucket.try_consume(0, 1.0);
        assert!(bucket.tokens() > 500.0);
    }

    #[test]
    fn test_token_bucket_fill_level_empty() {
        let mut bucket = TokenBucket::new(1000, 0.0);
        bucket.try_consume(1000, 0.0);
        assert_eq!(bucket.fill_level(), 0.0);
    }

    #[test]
    fn test_token_bucket_fill_level_full() {
        let bucket = TokenBucket::new(1000, 10000.0);
        assert_eq!(bucket.fill_level(), 1.0);
    }

    #[test]
    fn test_token_bucket_reset() {
        let mut bucket = TokenBucket::new(1000, 0.0);
        bucket.try_consume(500, 0.0);
        bucket.reset();
        assert_eq!(bucket.tokens(), 1000.0);
    }

    #[test]
    fn test_qos_registry_add_get_remove_policy() {
        let mut registry = QosRegistry::new();
        let policy = QosPolicy::new(
            "p1",
            "Policy 1",
            QosPriority::Normal,
            BandwidthLimit::unlimited(),
        );

        registry.add_policy(policy).unwrap();
        assert!(registry.get_policy("p1").is_some());

        registry.remove_policy("p1").unwrap();
        assert!(registry.get_policy("p1").is_none());
    }

    #[test]
    fn test_qos_registry_add_duplicate_policy() {
        let mut registry = QosRegistry::new();
        let policy = QosPolicy::new(
            "p1",
            "Policy 1",
            QosPriority::Normal,
            BandwidthLimit::unlimited(),
        );

        registry.add_policy(policy.clone()).unwrap();
        let result = registry.add_policy(policy);
        assert!(matches!(result, Err(QosError::PolicyAlreadyExists(_))));
    }

    #[test]
    fn test_qos_registry_assign_unassign() {
        let mut registry = QosRegistry::new();
        let policy = QosPolicy::new(
            "p1",
            "Policy 1",
            QosPriority::Normal,
            BandwidthLimit::unlimited(),
        );
        registry.add_policy(policy).unwrap();

        registry
            .assign("tenant1".to_string(), SubjectKind::Tenant, "p1".to_string())
            .unwrap();

        assert!(registry.get_assignment("tenant1").is_some());

        registry.unassign("tenant1").unwrap();
        assert!(registry.get_assignment("tenant1").is_none());
    }

    #[test]
    fn test_qos_registry_get_assignment_after_unassign() {
        let mut registry = QosRegistry::new();
        let policy = QosPolicy::new(
            "p1",
            "Policy 1",
            QosPriority::Normal,
            BandwidthLimit::unlimited(),
        );
        registry.add_policy(policy).unwrap();

        registry
            .assign("tenant1".to_string(), SubjectKind::Tenant, "p1".to_string())
            .unwrap();
        registry.unassign("tenant1").unwrap();

        let result = registry.get_assignment("tenant1");
        assert!(result.is_none());
    }

    #[test]
    fn test_qos_registry_effective_policy_unknown_subject() {
        let registry = QosRegistry::new();
        let result = registry.effective_policy("unknown");
        assert!(result.is_none());
    }

    #[test]
    fn test_qos_registry_effective_policy_returns_policy() {
        let mut registry = QosRegistry::new();
        let policy = QosPolicy::new(
            "p1",
            "Policy 1",
            QosPriority::High,
            BandwidthLimit::symmetric_mbps(100),
        );
        registry.add_policy(policy).unwrap();

        registry
            .assign("tenant1".to_string(), SubjectKind::Tenant, "p1".to_string())
            .unwrap();

        let effective = registry.effective_policy("tenant1");
        assert!(effective.is_some());
        assert_eq!(effective.unwrap().name, "Policy 1");
    }

    #[test]
    fn test_qos_registry_assignments_for_policy() {
        let mut registry = QosRegistry::new();
        let policy = QosPolicy::new(
            "p1",
            "Policy 1",
            QosPriority::Normal,
            BandwidthLimit::unlimited(),
        );
        registry.add_policy(policy).unwrap();

        registry
            .assign("tenant1".to_string(), SubjectKind::Tenant, "p1".to_string())
            .unwrap();
        registry
            .assign("tenant2".to_string(), SubjectKind::Tenant, "p1".to_string())
            .unwrap();

        let assignments = registry.assignments_for_policy("p1");
        assert_eq!(assignments.len(), 2);
    }

    #[test]
    fn test_qos_registry_remove_policy_when_assignments_exist() {
        let mut registry = QosRegistry::new();
        let policy = QosPolicy::new(
            "p1",
            "Policy 1",
            QosPriority::Normal,
            BandwidthLimit::unlimited(),
        );
        registry.add_policy(policy).unwrap();

        registry
            .assign("tenant1".to_string(), SubjectKind::Tenant, "p1".to_string())
            .unwrap();

        registry.remove_policy("p1").unwrap();
        assert!(registry.get_policy("p1").is_none());
    }

    #[test]
    fn test_qos_registry_policy_count() {
        let mut registry = QosRegistry::new();
        let policy1 = QosPolicy::new(
            "p1",
            "Policy 1",
            QosPriority::Normal,
            BandwidthLimit::unlimited(),
        );
        let policy2 = QosPolicy::new(
            "p2",
            "Policy 2",
            QosPriority::High,
            BandwidthLimit::unlimited(),
        );

        registry.add_policy(policy1).unwrap();
        registry.add_policy(policy2).unwrap();

        assert_eq!(registry.policy_count(), 2);
    }

    #[test]
    fn test_qos_registry_assignment_count() {
        let mut registry = QosRegistry::new();
        let policy = QosPolicy::new(
            "p1",
            "Policy 1",
            QosPriority::Normal,
            BandwidthLimit::unlimited(),
        );
        registry.add_policy(policy).unwrap();

        registry
            .assign("tenant1".to_string(), SubjectKind::Tenant, "p1".to_string())
            .unwrap();
        registry
            .assign("tenant2".to_string(), SubjectKind::Tenant, "p1".to_string())
            .unwrap();

        assert_eq!(registry.assignment_count(), 2);
    }

    #[test]
    fn test_qos_policy_new() {
        let policy = QosPolicy::new(
            "p1",
            "Test Policy",
            QosPriority::Normal,
            BandwidthLimit::symmetric_mbps(100),
        );
        assert_eq!(policy.policy_id, "p1");
        assert_eq!(policy.name, "Test Policy");
        assert_eq!(policy.priority, QosPriority::Normal);
        assert_eq!(policy.burst_multiplier, 1.0);
    }

    #[test]
    fn test_subject_kind_name() {
        assert_eq!(SubjectKind::Tenant.name(), "tenant");
        assert_eq!(SubjectKind::ClientIp.name(), "client_ip");
        assert_eq!(SubjectKind::User.name(), "user");
        assert_eq!(SubjectKind::Group.name(), "group");
    }

    #[test]
    fn test_token_bucket_capacity() {
        let bucket = TokenBucket::new(500, 50.0);
        assert_eq!(bucket.capacity(), 500);
    }

    #[test]
    fn test_qos_error_display() {
        let err = QosError::PolicyNotFound("p1".to_string());
        assert!(err.to_string().contains("p1"));

        let err = QosError::PolicyAlreadyExists("p1".to_string());
        assert!(err.to_string().contains("p1"));

        let err = QosError::AssignmentNotFound("tenant1".to_string());
        assert!(err.to_string().contains("tenant1"));
    }
}
