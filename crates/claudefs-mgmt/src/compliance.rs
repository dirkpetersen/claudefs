use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetentionPolicy {
    pub policy_id: String,
    pub name: String,
    pub retention_days: u32,
    pub worm_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RetentionStatus {
    Active,
    Expired,
    Locked,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetentionRecord {
    pub record_id: String,
    pub path: String,
    pub policy_id: String,
    pub created_at_ms: u64,
    pub expires_at_ms: u64,
    pub worm_enabled: bool,
}

impl RetentionRecord {
    pub fn status(&self, now_ms: u64) -> RetentionStatus {
        if self.worm_enabled {
            RetentionStatus::Locked
        } else if now_ms >= self.expires_at_ms {
            RetentionStatus::Expired
        } else {
            RetentionStatus::Active
        }
    }

    pub fn days_remaining(&self, now_ms: u64) -> i64 {
        let diff_ms = self.expires_at_ms as i64 - now_ms as i64;
        diff_ms / 86400000
    }
}

#[derive(Debug, Error, Clone)]
pub enum ComplianceError {
    #[error("Policy already exists: {0}")]
    PolicyAlreadyExists(String),
    #[error("Policy not found: {0}")]
    PolicyNotFound(String),
    #[error("Record not found: {0}")]
    RecordNotFound(String),
}

#[derive(Debug, Clone)]
pub struct ComplianceRegistry {
    policies: Arc<Mutex<HashMap<String, RetentionPolicy>>>,
    records: Arc<Mutex<HashMap<String, RetentionRecord>>>,
}

impl ComplianceRegistry {
    pub fn new() -> Self {
        Self {
            policies: Arc::new(Mutex::new(HashMap::new())),
            records: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn add_policy(&self, policy: RetentionPolicy) -> Result<(), ComplianceError> {
        let mut policies = self.policies.lock().unwrap();
        if policies.contains_key(&policy.policy_id) {
            return Err(ComplianceError::PolicyAlreadyExists(policy.policy_id));
        }
        policies.insert(policy.policy_id.clone(), policy);
        Ok(())
    }

    pub fn get_policy(&self, policy_id: &str) -> Option<RetentionPolicy> {
        let policies = self.policies.lock().unwrap();
        policies.get(policy_id).cloned()
    }

    pub fn register_file(
        &self,
        path: &str,
        policy_id: &str,
        created_at_ms: u64,
    ) -> Result<String, ComplianceError> {
        let policy = {
            let policies = self.policies.lock().unwrap();
            policies.get(policy_id).cloned()
        };

        let policy =
            policy.ok_or_else(|| ComplianceError::PolicyNotFound(policy_id.to_string()))?;

        let expires_at_ms = created_at_ms + (policy.retention_days as u64 * 86400000);
        let record_id = Uuid::new_v4().to_string();

        let record = RetentionRecord {
            record_id: record_id.clone(),
            path: path.to_string(),
            policy_id: policy_id.to_string(),
            created_at_ms,
            expires_at_ms,
            worm_enabled: policy.worm_enabled,
        };

        let mut records = self.records.lock().unwrap();
        records.insert(record_id.clone(), record);
        Ok(record_id)
    }

    pub fn get_record(&self, record_id: &str) -> Option<RetentionRecord> {
        let records = self.records.lock().unwrap();
        records.get(record_id).cloned()
    }

    pub fn active_records(&self, now_ms: u64) -> Vec<RetentionRecord> {
        let records = self.records.lock().unwrap();
        records
            .values()
            .filter(|r| r.status(now_ms) == RetentionStatus::Active)
            .cloned()
            .collect()
    }

    pub fn expired_records(&self, now_ms: u64) -> Vec<RetentionRecord> {
        let records = self.records.lock().unwrap();
        records
            .values()
            .filter(|r| r.status(now_ms) == RetentionStatus::Expired)
            .cloned()
            .collect()
    }

    pub fn policy_count(&self) -> usize {
        let policies = self.policies.lock().unwrap();
        policies.len()
    }

    pub fn record_count(&self) -> usize {
        let records = self.records.lock().unwrap();
        records.len()
    }
}

impl Default for ComplianceRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS_PER_DAY: u64 = 86400000;

    fn create_registry() -> (ComplianceRegistry, u64) {
        (ComplianceRegistry::new(), 1000 * MS_PER_DAY)
    }

    #[test]
    fn new_registry_has_zero_policies_and_records() {
        let registry = ComplianceRegistry::new();
        assert_eq!(registry.policy_count(), 0);
        assert_eq!(registry.record_count(), 0);
    }

    #[test]
    fn add_policy_stores_policy() {
        let (registry, _) = create_registry();
        let policy = RetentionPolicy {
            policy_id: "p1".to_string(),
            name: "Test Policy".to_string(),
            retention_days: 30,
            worm_enabled: false,
        };
        registry.add_policy(policy.clone()).unwrap();
        assert_eq!(registry.policy_count(), 1);
        assert_eq!(registry.get_policy("p1"), Some(policy));
    }

    #[test]
    fn add_policy_duplicate_returns_error() {
        let (registry, _) = create_registry();
        let policy = RetentionPolicy {
            policy_id: "p1".to_string(),
            name: "Test Policy".to_string(),
            retention_days: 30,
            worm_enabled: false,
        };
        registry.add_policy(policy.clone()).unwrap();
        let result = registry.add_policy(policy);
        assert!(matches!(
            result,
            Err(ComplianceError::PolicyAlreadyExists(_))
        ));
    }

    #[test]
    fn get_policy_returns_none_for_missing() {
        let (registry, _) = create_registry();
        assert_eq!(registry.get_policy("nonexistent"), None);
    }

    #[test]
    fn get_policy_returns_some_for_existing() {
        let (registry, _) = create_registry();
        let policy = RetentionPolicy {
            policy_id: "p1".to_string(),
            name: "Test Policy".to_string(),
            retention_days: 30,
            worm_enabled: false,
        };
        registry.add_policy(policy.clone()).unwrap();
        assert_eq!(registry.get_policy("p1"), Some(policy));
    }

    #[test]
    fn register_file_fails_for_missing_policy_id() {
        let (registry, now) = create_registry();
        let result = registry.register_file("/data/file.txt", "nonexistent", now);
        assert!(matches!(result, Err(ComplianceError::PolicyNotFound(_))));
    }

    #[test]
    fn register_file_creates_record_with_correct_expires_at() {
        let (registry, now) = create_registry();
        let policy = RetentionPolicy {
            policy_id: "p1".to_string(),
            name: "30 Day Policy".to_string(),
            retention_days: 30,
            worm_enabled: false,
        };
        registry.add_policy(policy).unwrap();

        let record_id = registry.register_file("/data/file.txt", "p1", now).unwrap();
        let record = registry.get_record(&record_id).unwrap();

        assert_eq!(record.expires_at_ms, now + 30 * MS_PER_DAY);
    }

    #[test]
    fn register_file_returns_unique_record_id_uuid() {
        let (registry, now) = create_registry();
        let policy = RetentionPolicy {
            policy_id: "p1".to_string(),
            name: "Test Policy".to_string(),
            retention_days: 30,
            worm_enabled: false,
        };
        registry.add_policy(policy).unwrap();

        let id1 = registry
            .register_file("/data/file1.txt", "p1", now)
            .unwrap();
        let id2 = registry
            .register_file("/data/file2.txt", "p1", now)
            .unwrap();

        assert_ne!(id1, id2);
        assert_eq!(id1.len(), 36);
        assert_eq!(id2.len(), 36);
    }

    #[test]
    fn get_record_returns_some_for_existing_record_id() {
        let (registry, now) = create_registry();
        let policy = RetentionPolicy {
            policy_id: "p1".to_string(),
            name: "Test Policy".to_string(),
            retention_days: 30,
            worm_enabled: false,
        };
        registry.add_policy(policy).unwrap();

        let record_id = registry.register_file("/data/file.txt", "p1", now).unwrap();
        let record = registry.get_record(&record_id);

        assert!(record.is_some());
        assert_eq!(record.unwrap().path, "/data/file.txt");
    }

    #[test]
    fn get_record_returns_none_for_missing_record_id() {
        let (registry, _) = create_registry();
        assert_eq!(registry.get_record("nonexistent"), None);
    }

    #[test]
    fn retention_record_days_remaining_positive_when_not_expired() {
        let (registry, now) = create_registry();
        let policy = RetentionPolicy {
            policy_id: "p1".to_string(),
            name: "30 Day Policy".to_string(),
            retention_days: 30,
            worm_enabled: false,
        };
        registry.add_policy(policy).unwrap();

        let record_id = registry.register_file("/data/file.txt", "p1", now).unwrap();
        let record = registry.get_record(&record_id).unwrap();

        assert!(record.days_remaining(now) > 0);
    }

    #[test]
    fn retention_record_days_remaining_negative_when_expired() {
        let (registry, now) = create_registry();
        let policy = RetentionPolicy {
            policy_id: "p1".to_string(),
            name: "30 Day Policy".to_string(),
            retention_days: 30,
            worm_enabled: false,
        };
        registry.add_policy(policy).unwrap();

        let record_id = registry.register_file("/data/file.txt", "p1", now).unwrap();
        let record = registry.get_record(&record_id).unwrap();

        let expired_time = now + 31 * MS_PER_DAY;
        assert!(record.days_remaining(expired_time) < 0);
    }

    #[test]
    fn retention_record_status_active_when_now_less_than_expires() {
        let (registry, now) = create_registry();
        let policy = RetentionPolicy {
            policy_id: "p1".to_string(),
            name: "30 Day Policy".to_string(),
            retention_days: 30,
            worm_enabled: false,
        };
        registry.add_policy(policy).unwrap();

        let record_id = registry.register_file("/data/file.txt", "p1", now).unwrap();
        let record = registry.get_record(&record_id).unwrap();

        assert_eq!(record.status(now), RetentionStatus::Active);
    }

    #[test]
    fn retention_record_status_expired_when_now_greater_or_equal_expires() {
        let (registry, now) = create_registry();
        let policy = RetentionPolicy {
            policy_id: "p1".to_string(),
            name: "30 Day Policy".to_string(),
            retention_days: 30,
            worm_enabled: false,
        };
        registry.add_policy(policy).unwrap();

        let record_id = registry.register_file("/data/file.txt", "p1", now).unwrap();
        let record = registry.get_record(&record_id).unwrap();

        let expired_time = now + 30 * MS_PER_DAY;
        assert_eq!(record.status(expired_time), RetentionStatus::Expired);
    }

    #[test]
    fn active_records_returns_only_active_records() {
        let (registry, now) = create_registry();
        let policy = RetentionPolicy {
            policy_id: "p1".to_string(),
            name: "30 Day Policy".to_string(),
            retention_days: 30,
            worm_enabled: false,
        };
        registry.add_policy(policy.clone()).unwrap();

        let id1 = registry
            .register_file("/data/file1.txt", "p1", now)
            .unwrap();
        let id2 = registry
            .register_file("/data/file2.txt", "p1", now)
            .unwrap();

        let active = registry.active_records(now);
        assert_eq!(active.len(), 2);

        let expired_time = now + 31 * MS_PER_DAY;
        let active = registry.active_records(expired_time);
        assert_eq!(active.len(), 0);
    }

    #[test]
    fn expired_records_returns_only_expired_records() {
        let (registry, now) = create_registry();
        let policy = RetentionPolicy {
            policy_id: "p1".to_string(),
            name: "30 Day Policy".to_string(),
            retention_days: 30,
            worm_enabled: false,
        };
        registry.add_policy(policy.clone()).unwrap();

        registry
            .register_file("/data/file1.txt", "p1", now)
            .unwrap();
        registry
            .register_file("/data/file2.txt", "p1", now)
            .unwrap();

        let expired = registry.expired_records(now);
        assert_eq!(expired.len(), 0);

        let expired_time = now + 31 * MS_PER_DAY;
        let expired = registry.expired_records(expired_time);
        assert_eq!(expired.len(), 2);
    }

    #[test]
    fn policy_count_tracks_additions() {
        let (registry, _) = create_registry();
        assert_eq!(registry.policy_count(), 0);

        registry
            .add_policy(RetentionPolicy {
                policy_id: "p1".to_string(),
                name: "Policy 1".to_string(),
                retention_days: 30,
                worm_enabled: false,
            })
            .unwrap();

        assert_eq!(registry.policy_count(), 1);

        registry
            .add_policy(RetentionPolicy {
                policy_id: "p2".to_string(),
                name: "Policy 2".to_string(),
                retention_days: 60,
                worm_enabled: false,
            })
            .unwrap();

        assert_eq!(registry.policy_count(), 2);
    }

    #[test]
    fn record_count_tracks_registrations() {
        let (registry, now) = create_registry();
        let policy = RetentionPolicy {
            policy_id: "p1".to_string(),
            name: "Test Policy".to_string(),
            retention_days: 30,
            worm_enabled: false,
        };
        registry.add_policy(policy).unwrap();

        assert_eq!(registry.record_count(), 0);

        registry
            .register_file("/data/file1.txt", "p1", now)
            .unwrap();
        assert_eq!(registry.record_count(), 1);

        registry
            .register_file("/data/file2.txt", "p1", now)
            .unwrap();
        assert_eq!(registry.record_count(), 2);
    }

    #[test]
    fn day_policy_record_expires_30_days_after_created_at() {
        let (registry, now) = create_registry();
        let policy = RetentionPolicy {
            policy_id: "p1".to_string(),
            name: "30 Day Policy".to_string(),
            retention_days: 30,
            worm_enabled: false,
        };
        registry.add_policy(policy).unwrap();

        let record_id = registry.register_file("/data/file.txt", "p1", now).unwrap();
        let record = registry.get_record(&record_id).unwrap();

        assert_eq!(record.expires_at_ms - record.created_at_ms, 30 * MS_PER_DAY);
    }

    #[test]
    fn day_policy_record_immediately_expired() {
        let (registry, now) = create_registry();
        let policy = RetentionPolicy {
            policy_id: "p1".to_string(),
            name: "0 Day Policy".to_string(),
            retention_days: 0,
            worm_enabled: false,
        };
        registry.add_policy(policy).unwrap();

        let record_id = registry.register_file("/data/file.txt", "p1", now).unwrap();
        let record = registry.get_record(&record_id).unwrap();

        assert_eq!(record.status(now), RetentionStatus::Expired);
    }

    #[test]
    fn retention_record_status_locked_when_worm_enabled() {
        let (registry, now) = create_registry();
        let policy = RetentionPolicy {
            policy_id: "p1".to_string(),
            name: "WORM Policy".to_string(),
            retention_days: 30,
            worm_enabled: true,
        };
        registry.add_policy(policy).unwrap();

        let record_id = registry.register_file("/data/file.txt", "p1", now).unwrap();
        let record = registry.get_record(&record_id).unwrap();

        assert_eq!(record.status(now), RetentionStatus::Locked);
    }

    #[test]
    fn worm_enabled_records_not_in_expired_list() {
        let (registry, now) = create_registry();
        let policy = RetentionPolicy {
            policy_id: "p1".to_string(),
            name: "WORM Policy".to_string(),
            retention_days: 30,
            worm_enabled: true,
        };
        registry.add_policy(policy).unwrap();

        registry.register_file("/data/file.txt", "p1", now).unwrap();

        let expired_time = now + 31 * MS_PER_DAY;
        let expired = registry.expired_records(expired_time);
        assert_eq!(expired.len(), 0);
    }

    #[test]
    fn worm_enabled_records_not_in_active_list() {
        let (registry, now) = create_registry();
        let policy = RetentionPolicy {
            policy_id: "p1".to_string(),
            name: "WORM Policy".to_string(),
            retention_days: 30,
            worm_enabled: true,
        };
        registry.add_policy(policy).unwrap();

        registry.register_file("/data/file.txt", "p1", now).unwrap();

        let active = registry.active_records(now);
        assert_eq!(active.len(), 0);
    }
}
