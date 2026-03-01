use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum QuotaError {
    #[error("Quota exceeded for {subject}: used {used} bytes, limit {limit} bytes")]
    Exceeded {
        subject: String,
        used: u64,
        limit: u64,
    },
    #[error("Unknown subject: {0}")]
    UnknownSubject(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum QuotaSubjectType {
    User,
    Group,
    Directory,
    Tenant,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaLimit {
    pub subject: String,
    pub subject_type: QuotaSubjectType,
    pub max_bytes: Option<u64>,
    pub max_files: Option<u64>,
    pub max_iops: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaUsage {
    pub subject: String,
    pub subject_type: QuotaSubjectType,
    pub used_bytes: u64,
    pub used_files: u64,
    pub iops_current: u64,
}

impl QuotaUsage {
    pub fn bytes_available(&self, limit: &QuotaLimit) -> Option<u64> {
        match limit.max_bytes {
            Some(max) => Some(max.saturating_sub(self.used_bytes)),
            None => None,
        }
    }

    pub fn files_available(&self, limit: &QuotaLimit) -> Option<u64> {
        match limit.max_files {
            Some(max) => Some(max.saturating_sub(self.used_files)),
            None => None,
        }
    }

    pub fn is_bytes_exceeded(&self, limit: &QuotaLimit) -> bool {
        match limit.max_bytes {
            Some(max) => self.used_bytes > max,
            None => false,
        }
    }

    pub fn is_files_exceeded(&self, limit: &QuotaLimit) -> bool {
        match limit.max_files {
            Some(max) => self.used_files > max,
            None => false,
        }
    }

    pub fn usage_percent_bytes(&self, limit: &QuotaLimit) -> Option<f64> {
        match limit.max_bytes {
            Some(max) if max > 0 => Some((self.used_bytes as f64 / max as f64) * 100.0),
            _ => None,
        }
    }
}

pub struct QuotaRegistry {
    limits: HashMap<String, QuotaLimit>,
    usage: HashMap<String, QuotaUsage>,
}

impl QuotaRegistry {
    pub fn new() -> Self {
        Self {
            limits: HashMap::new(),
            usage: HashMap::new(),
        }
    }

    pub fn set_limit(&mut self, limit: QuotaLimit) {
        self.limits.insert(limit.subject.clone(), limit);
    }

    pub fn remove_limit(&mut self, subject: &str) -> Option<QuotaLimit> {
        self.limits.remove(subject)
    }

    pub fn get_limit(&self, subject: &str) -> Option<&QuotaLimit> {
        self.limits.get(subject)
    }

    pub fn update_usage(&mut self, usage: QuotaUsage) {
        self.usage.insert(usage.subject.clone(), usage);
    }

    pub fn get_usage(&self, subject: &str) -> Option<&QuotaUsage> {
        self.usage.get(subject)
    }

    pub fn check_quota(&self, subject: &str) -> Result<(), QuotaError> {
        let limit = self
            .limits
            .get(subject)
            .ok_or_else(|| QuotaError::UnknownSubject(subject.to_string()))?;

        let usage = self
            .usage
            .get(subject)
            .ok_or_else(|| QuotaError::UnknownSubject(subject.to_string()))?;

        if usage.is_bytes_exceeded(limit) {
            return Err(QuotaError::Exceeded {
                subject: subject.to_string(),
                used: usage.used_bytes,
                limit: limit.max_bytes.unwrap_or(0),
            });
        }

        if usage.is_files_exceeded(limit) {
            return Err(QuotaError::Exceeded {
                subject: subject.to_string(),
                used: usage.used_files,
                limit: limit.max_files.unwrap_or(0),
            });
        }

        Ok(())
    }

    pub fn over_quota_subjects(&self) -> Vec<(&QuotaLimit, &QuotaUsage)> {
        self.limits
            .iter()
            .filter_map(|(subject, limit)| {
                self.usage
                    .get(subject)
                    .map(|usage| {
                        if usage.is_bytes_exceeded(limit) || usage.is_files_exceeded(limit) {
                            Some((limit, usage))
                        } else {
                            None
                        }
                    })
                    .unwrap_or(None)
            })
            .collect()
    }

    pub fn limit_count(&self) -> usize {
        self.limits.len()
    }

    pub fn near_quota_subjects(&self, threshold: f64) -> Vec<(&QuotaLimit, &QuotaUsage)> {
        let threshold_factor = threshold / 100.0;

        self.limits
            .iter()
            .filter_map(|(subject, limit)| {
                self.usage
                    .get(subject)
                    .map(|usage| {
                        let bytes_near = limit
                            .max_bytes
                            .map(|max| {
                                let ratio = usage.used_bytes as f64 / max as f64;
                                ratio >= threshold_factor
                            })
                            .unwrap_or(false);

                        let files_near = limit
                            .max_files
                            .map(|max| {
                                let ratio = usage.used_files as f64 / max as f64;
                                ratio >= threshold_factor
                            })
                            .unwrap_or(false);

                        if bytes_near || files_near {
                            Some((limit, usage))
                        } else {
                            None
                        }
                    })
                    .unwrap_or(None)
            })
            .collect()
    }
}

impl Default for QuotaRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quota_usage_bytes_available_under() {
        let usage = QuotaUsage {
            subject: "user1".to_string(),
            subject_type: QuotaSubjectType::User,
            used_bytes: 1000,
            used_files: 10,
            iops_current: 100,
        };

        let limit = QuotaLimit {
            subject: "user1".to_string(),
            subject_type: QuotaSubjectType::User,
            max_bytes: Some(10000),
            max_files: Some(100),
            max_iops: None,
        };

        assert_eq!(usage.bytes_available(&limit), Some(9000));
    }

    #[test]
    fn test_quota_usage_bytes_available_none_limit() {
        let usage = QuotaUsage {
            subject: "user1".to_string(),
            subject_type: QuotaSubjectType::User,
            used_bytes: 1000,
            used_files: 10,
            iops_current: 100,
        };

        let limit = QuotaLimit {
            subject: "user1".to_string(),
            subject_type: QuotaSubjectType::User,
            max_bytes: None,
            max_files: Some(100),
            max_iops: None,
        };

        assert_eq!(usage.bytes_available(&limit), None);
    }

    #[test]
    fn test_quota_usage_is_bytes_exceeded_over() {
        let usage = QuotaUsage {
            subject: "user1".to_string(),
            subject_type: QuotaSubjectType::User,
            used_bytes: 15000,
            used_files: 10,
            iops_current: 100,
        };

        let limit = QuotaLimit {
            subject: "user1".to_string(),
            subject_type: QuotaSubjectType::User,
            max_bytes: Some(10000),
            max_files: Some(100),
            max_iops: None,
        };

        assert!(usage.is_bytes_exceeded(&limit));
    }

    #[test]
    fn test_quota_usage_is_bytes_exceeded_under() {
        let usage = QuotaUsage {
            subject: "user1".to_string(),
            subject_type: QuotaSubjectType::User,
            used_bytes: 5000,
            used_files: 10,
            iops_current: 100,
        };

        let limit = QuotaLimit {
            subject: "user1".to_string(),
            subject_type: QuotaSubjectType::User,
            max_bytes: Some(10000),
            max_files: Some(100),
            max_iops: None,
        };

        assert!(!usage.is_bytes_exceeded(&limit));
    }

    #[test]
    fn test_quota_usage_percent_bytes() {
        let usage = QuotaUsage {
            subject: "user1".to_string(),
            subject_type: QuotaSubjectType::User,
            used_bytes: 5000,
            used_files: 10,
            iops_current: 100,
        };

        let limit = QuotaLimit {
            subject: "user1".to_string(),
            subject_type: QuotaSubjectType::User,
            max_bytes: Some(10000),
            max_files: Some(100),
            max_iops: None,
        };

        assert_eq!(usage.usage_percent_bytes(&limit), Some(50.0));
    }

    #[test]
    fn test_quota_usage_percent_bytes_no_limit() {
        let usage = QuotaUsage {
            subject: "user1".to_string(),
            subject_type: QuotaSubjectType::User,
            used_bytes: 5000,
            used_files: 10,
            iops_current: 100,
        };

        let limit = QuotaLimit {
            subject: "user1".to_string(),
            subject_type: QuotaSubjectType::User,
            max_bytes: None,
            max_files: Some(100),
            max_iops: None,
        };

        assert_eq!(usage.usage_percent_bytes(&limit), None);
    }

    #[test]
    fn test_quota_registry_set_and_get_limit() {
        let mut registry = QuotaRegistry::new();

        let limit = QuotaLimit {
            subject: "user1".to_string(),
            subject_type: QuotaSubjectType::User,
            max_bytes: Some(10000),
            max_files: Some(100),
            max_iops: None,
        };

        registry.set_limit(limit.clone());

        let retrieved = registry.get_limit("user1").unwrap();
        assert_eq!(retrieved.subject, "user1");
    }

    #[test]
    fn test_quota_registry_remove_limit() {
        let mut registry = QuotaRegistry::new();

        let limit = QuotaLimit {
            subject: "user1".to_string(),
            subject_type: QuotaSubjectType::User,
            max_bytes: Some(10000),
            max_files: Some(100),
            max_iops: None,
        };

        registry.set_limit(limit);

        let removed = registry.remove_limit("user1");
        assert!(removed.is_some());

        assert!(registry.get_limit("user1").is_none());
    }

    #[test]
    fn test_quota_registry_update_and_get_usage() {
        let mut registry = QuotaRegistry::new();

        let usage = QuotaUsage {
            subject: "user1".to_string(),
            subject_type: QuotaSubjectType::User,
            used_bytes: 5000,
            used_files: 50,
            iops_current: 100,
        };

        registry.update_usage(usage);

        let retrieved = registry.get_usage("user1").unwrap();
        assert_eq!(retrieved.used_bytes, 5000);
    }

    #[test]
    fn test_quota_registry_check_quota_pass() {
        let mut registry = QuotaRegistry::new();

        let limit = QuotaLimit {
            subject: "user1".to_string(),
            subject_type: QuotaSubjectType::User,
            max_bytes: Some(10000),
            max_files: Some(100),
            max_iops: None,
        };

        let usage = QuotaUsage {
            subject: "user1".to_string(),
            subject_type: QuotaSubjectType::User,
            used_bytes: 5000,
            used_files: 50,
            iops_current: 100,
        };

        registry.set_limit(limit);
        registry.update_usage(usage);

        let result = registry.check_quota("user1");
        assert!(result.is_ok());
    }

    #[test]
    fn test_quota_registry_check_quota_fail() {
        let mut registry = QuotaRegistry::new();

        let limit = QuotaLimit {
            subject: "user1".to_string(),
            subject_type: QuotaSubjectType::User,
            max_bytes: Some(10000),
            max_files: Some(100),
            max_iops: None,
        };

        let usage = QuotaUsage {
            subject: "user1".to_string(),
            subject_type: QuotaSubjectType::User,
            used_bytes: 15000,
            used_files: 50,
            iops_current: 100,
        };

        registry.set_limit(limit);
        registry.update_usage(usage);

        let result = registry.check_quota("user1");
        assert!(result.is_err());
    }

    #[test]
    fn test_quota_registry_over_quota_subjects() {
        let mut registry = QuotaRegistry::new();

        let limit1 = QuotaLimit {
            subject: "user1".to_string(),
            subject_type: QuotaSubjectType::User,
            max_bytes: Some(10000),
            max_files: Some(100),
            max_iops: None,
        };

        let usage1 = QuotaUsage {
            subject: "user1".to_string(),
            subject_type: QuotaSubjectType::User,
            used_bytes: 15000,
            used_files: 50,
            iops_current: 100,
        };

        let limit2 = QuotaLimit {
            subject: "user2".to_string(),
            subject_type: QuotaSubjectType::User,
            max_bytes: Some(10000),
            max_files: Some(100),
            max_iops: None,
        };

        let usage2 = QuotaUsage {
            subject: "user2".to_string(),
            subject_type: QuotaSubjectType::User,
            used_bytes: 5000,
            used_files: 50,
            iops_current: 100,
        };

        registry.set_limit(limit1);
        registry.update_usage(usage1);
        registry.set_limit(limit2);
        registry.update_usage(usage2);

        let over = registry.over_quota_subjects();
        assert_eq!(over.len(), 1);
        assert_eq!(over[0].0.subject, "user1");
    }

    #[test]
    fn test_quota_registry_near_quota_subjects() {
        let mut registry = QuotaRegistry::new();

        let limit1 = QuotaLimit {
            subject: "user1".to_string(),
            subject_type: QuotaSubjectType::User,
            max_bytes: Some(10000),
            max_files: Some(100),
            max_iops: None,
        };

        let usage1 = QuotaUsage {
            subject: "user1".to_string(),
            subject_type: QuotaSubjectType::User,
            used_bytes: 9000,
            used_files: 50,
            iops_current: 100,
        };

        let limit2 = QuotaLimit {
            subject: "user2".to_string(),
            subject_type: QuotaSubjectType::User,
            max_bytes: Some(10000),
            max_files: Some(100),
            max_iops: None,
        };

        let usage2 = QuotaUsage {
            subject: "user2".to_string(),
            subject_type: QuotaSubjectType::User,
            used_bytes: 5000,
            used_files: 50,
            iops_current: 100,
        };

        registry.set_limit(limit1);
        registry.update_usage(usage1);
        registry.set_limit(limit2);
        registry.update_usage(usage2);

        let near = registry.near_quota_subjects(80.0);
        assert_eq!(near.len(), 1);
        assert_eq!(near[0].0.subject, "user1");
    }

    #[test]
    fn test_quota_registry_limit_count() {
        let mut registry = QuotaRegistry::new();

        registry.set_limit(QuotaLimit {
            subject: "user1".to_string(),
            subject_type: QuotaSubjectType::User,
            max_bytes: Some(10000),
            max_files: Some(100),
            max_iops: None,
        });

        registry.set_limit(QuotaLimit {
            subject: "user2".to_string(),
            subject_type: QuotaSubjectType::User,
            max_bytes: Some(20000),
            max_files: Some(200),
            max_iops: None,
        });

        assert_eq!(registry.limit_count(), 2);
    }
}
