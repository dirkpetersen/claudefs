//! S3 Object Lock support for WORM (Write Once Read Many) compliance.
//!
//! S3 Object Lock prevents objects from being deleted or overwritten for a defined
//! retention period. It supports two modes (Governance and Compliance) and Legal Hold.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, SystemTime};
use thiserror::Error;
use tracing::{debug, warn};

/// Retention mode for Object Lock
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RetentionMode {
    /// Governance mode - admin can override with bypass permission
    Governance,
    /// Compliance mode - no override, stricter
    Compliance,
}

/// Object Lock status for a bucket
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ObjectLockStatus {
    Enabled,
    Disabled,
}

/// Retention period specification
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RetentionPeriod {
    Days(u32),
    Years(u32),
}

impl RetentionPeriod {
    pub fn to_duration(&self) -> Duration {
        match self {
            RetentionPeriod::Days(days) => Duration::from_secs(60 * 60 * 24 * *days as u64),
            RetentionPeriod::Years(years) => {
                Duration::from_secs(60 * 60 * 24 * 365 * *years as u64)
            }
        }
    }
}

/// Default retention configuration for a bucket
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DefaultRetention {
    pub mode: RetentionMode,
    pub retention_period: RetentionPeriod,
}

/// Bucket Object Lock configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BucketObjectLockConfig {
    pub bucket: String,
    pub status: ObjectLockStatus,
    pub default_retention: Option<DefaultRetention>,
}

/// Object retention settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectRetention {
    pub mode: RetentionMode,
    pub retain_until: SystemTime,
}

impl ObjectRetention {
    pub fn is_expired(&self) -> bool {
        SystemTime::now() > self.retain_until
    }
}

/// Legal Hold status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LegalHoldStatus {
    On,
    Off,
}

/// Object Lock information for a specific object version
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectLockInfo {
    pub bucket: String,
    pub key: String,
    pub version_id: String,
    pub retention: Option<ObjectRetention>,
    pub legal_hold: LegalHoldStatus,
}

impl ObjectLockInfo {
    pub fn has_active_retention(&self) -> bool {
        match &self.retention {
            Some(r) => !r.is_expired(),
            None => false,
        }
    }
}

/// Object Lock errors
#[derive(Debug, Error)]
pub enum ObjectLockError {
    #[error("Bucket lock not enabled: {0}")]
    BucketLockNotEnabled(String),
    #[error(
        "Retention period not elapsed for {}/{}. Retention active until {until:?}",
        bucket,
        key
    )]
    RetentionPeriodNotElapsed {
        bucket: String,
        key: String,
        until: SystemTime,
    },
    #[error("Legal hold active on {}/{}. Cannot delete or overwrite", bucket, key)]
    LegalHoldActive { bucket: String, key: String },
    #[error("Compliance mode cannot be bypassed for {}/{}", bucket, key)]
    ComplianceModeCannotBypass { bucket: String, key: String },
    #[error("Invalid retention date: retain_until is in the past")]
    InvalidRetentionDate,
    #[error("Bucket {0} already has object lock configured")]
    BucketAlreadyConfigured(String),
}

/// Object Lock Registry - tracks object lock state for all buckets and objects
#[derive(Debug, Default)]
pub struct ObjectLockRegistry {
    bucket_configs: HashMap<String, BucketObjectLockConfig>,
    object_locks: HashMap<(String, String, String), ObjectLockInfo>,
}

impl ObjectLockRegistry {
    pub fn new() -> Self {
        Self {
            bucket_configs: HashMap::new(),
            object_locks: HashMap::new(),
        }
    }

    /// Configure Object Lock for a bucket
    pub fn configure_bucket(
        &mut self,
        config: BucketObjectLockConfig,
    ) -> Result<(), ObjectLockError> {
        if config.status == ObjectLockStatus::Disabled {
            warn!(
                "Configuring bucket {} with Object Lock disabled",
                config.bucket
            );
        }

        if self.bucket_configs.contains_key(&config.bucket) {
            return Err(ObjectLockError::BucketAlreadyConfigured(config.bucket));
        }

        debug!(
            "Configuring bucket {} with Object Lock status: {:?}",
            config.bucket, config.status
        );
        self.bucket_configs.insert(config.bucket.clone(), config);
        Ok(())
    }

    /// Get Object Lock configuration for a bucket
    pub fn get_bucket_config(&self, bucket: &str) -> Option<&BucketObjectLockConfig> {
        self.bucket_configs.get(bucket)
    }

    /// Set retention on an object version
    pub fn set_retention(&mut self, info: ObjectLockInfo) -> Result<(), ObjectLockError> {
        let bucket_config = self
            .bucket_configs
            .get(&info.bucket)
            .ok_or_else(|| ObjectLockError::BucketLockNotEnabled(info.bucket.clone()))?;

        if bucket_config.status != ObjectLockStatus::Enabled {
            return Err(ObjectLockError::BucketLockNotEnabled(info.bucket.clone()));
        }

        if let Some(ref retention) = info.retention {
            if retention.retain_until <= SystemTime::now() {
                return Err(ObjectLockError::InvalidRetentionDate);
            }
        }

        debug!(
            "Setting retention on {}/{}/{}",
            info.bucket, info.key, info.version_id
        );
        let key = (
            info.bucket.clone(),
            info.key.clone(),
            info.version_id.clone(),
        );
        self.object_locks.insert(key, info);
        Ok(())
    }

    /// Get lock info for an object version
    pub fn get_lock_info(
        &self,
        bucket: &str,
        key: &str,
        version_id: &str,
    ) -> Option<&ObjectLockInfo> {
        self.object_locks
            .get(&(bucket.to_string(), key.to_string(), version_id.to_string()))
    }

    /// Set legal hold status on an object version
    pub fn set_legal_hold(
        &mut self,
        bucket: &str,
        key: &str,
        version_id: &str,
        status: LegalHoldStatus,
    ) -> Result<(), ObjectLockError> {
        let bucket_config = self
            .bucket_configs
            .get(bucket)
            .ok_or_else(|| ObjectLockError::BucketLockNotEnabled(bucket.to_string()))?;

        if bucket_config.status != ObjectLockStatus::Enabled {
            return Err(ObjectLockError::BucketLockNotEnabled(bucket.to_string()));
        }

        debug!(
            "Setting legal hold to {:?} on {}/{}/{}",
            status, bucket, key, version_id
        );

        let key_tuple = (bucket.to_string(), key.to_string(), version_id.to_string());
        match self.object_locks.get_mut(&key_tuple) {
            Some(info) => {
                info.legal_hold = status;
            }
            None => {
                let info = ObjectLockInfo {
                    bucket: bucket.to_string(),
                    key: key.to_string(),
                    version_id: version_id.to_string(),
                    retention: None,
                    legal_hold: status,
                };
                self.object_locks.insert(key_tuple, info);
            }
        }
        Ok(())
    }

    /// Check if an object version can be deleted
    pub fn can_delete(
        &self,
        bucket: &str,
        key: &str,
        version_id: &str,
        bypass_governance: bool,
    ) -> Result<(), ObjectLockError> {
        let info = match self.object_locks.get(&(
            bucket.to_string(),
            key.to_string(),
            version_id.to_string(),
        )) {
            Some(info) => info,
            None => return Ok(()),
        };

        if info.legal_hold == LegalHoldStatus::On {
            return Err(ObjectLockError::LegalHoldActive {
                bucket: bucket.to_string(),
                key: key.to_string(),
            });
        }

        if info.has_active_retention() {
            let retention = info.retention.as_ref().unwrap();
            if retention.mode == RetentionMode::Compliance {
                return Err(ObjectLockError::ComplianceModeCannotBypass {
                    bucket: bucket.to_string(),
                    key: key.to_string(),
                });
            }
            if !bypass_governance {
                return Err(ObjectLockError::RetentionPeriodNotElapsed {
                    bucket: bucket.to_string(),
                    key: key.to_string(),
                    until: retention.retain_until,
                });
            }
        }

        Ok(())
    }

    /// Check if an object version can be overwritten
    pub fn can_overwrite(
        &self,
        bucket: &str,
        key: &str,
        version_id: &str,
        bypass_governance: bool,
    ) -> Result<(), ObjectLockError> {
        self.can_delete(bucket, key, version_id, bypass_governance)
    }

    /// Get default retention end time for a bucket based on its configuration
    pub fn default_retention_for_bucket(&self, bucket: &str) -> Option<SystemTime> {
        let config = self.bucket_configs.get(bucket)?;
        let default_retention = config.default_retention.as_ref()?;
        let duration = default_retention.retention_period.to_duration();
        SystemTime::now().checked_add(duration)
    }

    /// Count of objects with active (non-expired) retention
    pub fn locked_object_count(&self) -> usize {
        self.object_locks
            .values()
            .filter(|info| info.has_active_retention())
            .count()
    }

    /// Count of objects with LegalHoldStatus::On
    pub fn legal_hold_count(&self) -> usize {
        self.object_locks
            .values()
            .filter(|info| info.legal_hold == LegalHoldStatus::On)
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_future_time(days: u32) -> SystemTime {
        SystemTime::now() + Duration::from_secs(60 * 60 * 24 * days as u64)
    }

    fn create_past_time(days: u32) -> SystemTime {
        SystemTime::now() - Duration::from_secs(60 * 60 * 24 * days as u64)
    }

    #[test]
    fn test_configure_and_get_bucket_config() {
        let mut registry = ObjectLockRegistry::new();
        let config = BucketObjectLockConfig {
            bucket: "test-bucket".to_string(),
            status: ObjectLockStatus::Enabled,
            default_retention: Some(DefaultRetention {
                mode: RetentionMode::Governance,
                retention_period: RetentionPeriod::Days(10),
            }),
        };

        registry.configure_bucket(config.clone()).unwrap();
        let retrieved = registry.get_bucket_config("test-bucket");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().bucket, "test-bucket");
        assert_eq!(retrieved.unwrap().status, ObjectLockStatus::Enabled);
    }

    #[test]
    fn test_configure_bucket_already_configured() {
        let mut registry = ObjectLockRegistry::new();
        let config = BucketObjectLockConfig {
            bucket: "test-bucket".to_string(),
            status: ObjectLockStatus::Enabled,
            default_retention: None,
        };

        registry.configure_bucket(config.clone()).unwrap();
        let result = registry.configure_bucket(config);
        assert!(matches!(
            result,
            Err(ObjectLockError::BucketAlreadyConfigured(_))
        ));
    }

    #[test]
    fn test_set_retention_future_date() {
        let mut registry = ObjectLockRegistry::new();
        registry
            .configure_bucket(BucketObjectLockConfig {
                bucket: "test-bucket".to_string(),
                status: ObjectLockStatus::Enabled,
                default_retention: None,
            })
            .unwrap();

        let info = ObjectLockInfo {
            bucket: "test-bucket".to_string(),
            key: "test-key".to_string(),
            version_id: "v1".to_string(),
            retention: Some(ObjectRetention {
                mode: RetentionMode::Governance,
                retain_until: create_future_time(30),
            }),
            legal_hold: LegalHoldStatus::Off,
        };

        let result = registry.set_retention(info);
        assert!(result.is_ok());
    }

    #[test]
    fn test_set_retention_past_date() {
        let mut registry = ObjectLockRegistry::new();
        registry
            .configure_bucket(BucketObjectLockConfig {
                bucket: "test-bucket".to_string(),
                status: ObjectLockStatus::Enabled,
                default_retention: None,
            })
            .unwrap();

        let info = ObjectLockInfo {
            bucket: "test-bucket".to_string(),
            key: "test-key".to_string(),
            version_id: "v1".to_string(),
            retention: Some(ObjectRetention {
                mode: RetentionMode::Governance,
                retain_until: create_past_time(1),
            }),
            legal_hold: LegalHoldStatus::Off,
        };

        let result = registry.set_retention(info);
        assert!(matches!(result, Err(ObjectLockError::InvalidRetentionDate)));
    }

    #[test]
    fn test_can_delete_legal_hold_on() {
        let mut registry = ObjectLockRegistry::new();
        registry
            .configure_bucket(BucketObjectLockConfig {
                bucket: "test-bucket".to_string(),
                status: ObjectLockStatus::Enabled,
                default_retention: None,
            })
            .unwrap();

        registry
            .set_legal_hold("test-bucket", "test-key", "v1", LegalHoldStatus::On)
            .unwrap();

        let result = registry.can_delete("test-bucket", "test-key", "v1", false);
        assert!(matches!(
            result,
            Err(ObjectLockError::LegalHoldActive { .. })
        ));
    }

    #[test]
    fn test_can_delete_compliance_mode_active() {
        let mut registry = ObjectLockRegistry::new();
        registry
            .configure_bucket(BucketObjectLockConfig {
                bucket: "test-bucket".to_string(),
                status: ObjectLockStatus::Enabled,
                default_retention: None,
            })
            .unwrap();

        let info = ObjectLockInfo {
            bucket: "test-bucket".to_string(),
            key: "test-key".to_string(),
            version_id: "v1".to_string(),
            retention: Some(ObjectRetention {
                mode: RetentionMode::Compliance,
                retain_until: create_future_time(30),
            }),
            legal_hold: LegalHoldStatus::Off,
        };
        registry.set_retention(info).unwrap();

        let result = registry.can_delete("test-bucket", "test-key", "v1", false);
        assert!(matches!(
            result,
            Err(ObjectLockError::ComplianceModeCannotBypass { .. })
        ));
    }

    #[test]
    fn test_can_delete_governance_mode_bypass_false() {
        let mut registry = ObjectLockRegistry::new();
        registry
            .configure_bucket(BucketObjectLockConfig {
                bucket: "test-bucket".to_string(),
                status: ObjectLockStatus::Enabled,
                default_retention: None,
            })
            .unwrap();

        let info = ObjectLockInfo {
            bucket: "test-bucket".to_string(),
            key: "test-key".to_string(),
            version_id: "v1".to_string(),
            retention: Some(ObjectRetention {
                mode: RetentionMode::Governance,
                retain_until: create_future_time(30),
            }),
            legal_hold: LegalHoldStatus::Off,
        };
        registry.set_retention(info).unwrap();

        let result = registry.can_delete("test-bucket", "test-key", "v1", false);
        assert!(matches!(
            result,
            Err(ObjectLockError::RetentionPeriodNotElapsed { .. })
        ));
    }

    #[test]
    fn test_can_delete_governance_mode_bypass_true() {
        let mut registry = ObjectLockRegistry::new();
        registry
            .configure_bucket(BucketObjectLockConfig {
                bucket: "test-bucket".to_string(),
                status: ObjectLockStatus::Enabled,
                default_retention: None,
            })
            .unwrap();

        let info = ObjectLockInfo {
            bucket: "test-bucket".to_string(),
            key: "test-key".to_string(),
            version_id: "v1".to_string(),
            retention: Some(ObjectRetention {
                mode: RetentionMode::Governance,
                retain_until: create_future_time(30),
            }),
            legal_hold: LegalHoldStatus::Off,
        };
        registry.set_retention(info).unwrap();

        let result = registry.can_delete("test-bucket", "test-key", "v1", true);
        assert!(result.is_ok());
    }

    #[test]
    fn test_can_overwrite_no_lock() {
        let registry = ObjectLockRegistry::new();
        let result = registry.can_overwrite("bucket", "key", "v1", false);
        assert!(result.is_ok());
    }

    #[test]
    fn test_can_overwrite_legal_hold_on() {
        let mut registry = ObjectLockRegistry::new();
        registry
            .configure_bucket(BucketObjectLockConfig {
                bucket: "test-bucket".to_string(),
                status: ObjectLockStatus::Enabled,
                default_retention: None,
            })
            .unwrap();

        registry
            .set_legal_hold("test-bucket", "test-key", "v1", LegalHoldStatus::On)
            .unwrap();

        let result = registry.can_overwrite("test-bucket", "test-key", "v1", false);
        assert!(matches!(
            result,
            Err(ObjectLockError::LegalHoldActive { .. })
        ));
    }

    #[test]
    fn test_can_overwrite_governance_mode_bypass_true() {
        let mut registry = ObjectLockRegistry::new();
        registry
            .configure_bucket(BucketObjectLockConfig {
                bucket: "test-bucket".to_string(),
                status: ObjectLockStatus::Enabled,
                default_retention: None,
            })
            .unwrap();

        let info = ObjectLockInfo {
            bucket: "test-bucket".to_string(),
            key: "test-key".to_string(),
            version_id: "v1".to_string(),
            retention: Some(ObjectRetention {
                mode: RetentionMode::Governance,
                retain_until: create_future_time(30),
            }),
            legal_hold: LegalHoldStatus::Off,
        };
        registry.set_retention(info).unwrap();

        let result = registry.can_overwrite("test-bucket", "test-key", "v1", true);
        assert!(result.is_ok());
    }

    #[test]
    fn test_set_legal_hold_on() {
        let mut registry = ObjectLockRegistry::new();
        registry
            .configure_bucket(BucketObjectLockConfig {
                bucket: "test-bucket".to_string(),
                status: ObjectLockStatus::Enabled,
                default_retention: None,
            })
            .unwrap();

        registry
            .set_legal_hold("test-bucket", "test-key", "v1", LegalHoldStatus::On)
            .unwrap();

        let info = registry.get_lock_info("test-bucket", "test-key", "v1");
        assert!(info.is_some());
        assert_eq!(info.unwrap().legal_hold, LegalHoldStatus::On);
    }

    #[test]
    fn test_set_legal_hold_off() {
        let mut registry = ObjectLockRegistry::new();
        registry
            .configure_bucket(BucketObjectLockConfig {
                bucket: "test-bucket".to_string(),
                status: ObjectLockStatus::Enabled,
                default_retention: None,
            })
            .unwrap();

        registry
            .set_legal_hold("test-bucket", "test-key", "v1", LegalHoldStatus::On)
            .unwrap();
        registry
            .set_legal_hold("test-bucket", "test-key", "v1", LegalHoldStatus::Off)
            .unwrap();

        let info = registry.get_lock_info("test-bucket", "test-key", "v1");
        assert!(info.is_some());
        assert_eq!(info.unwrap().legal_hold, LegalHoldStatus::Off);
    }

    #[test]
    fn test_legal_hold_count() {
        let mut registry = ObjectLockRegistry::new();
        registry
            .configure_bucket(BucketObjectLockConfig {
                bucket: "test-bucket".to_string(),
                status: ObjectLockStatus::Enabled,
                default_retention: None,
            })
            .unwrap();

        registry
            .set_legal_hold("test-bucket", "key1", "v1", LegalHoldStatus::On)
            .unwrap();
        registry
            .set_legal_hold("test-bucket", "key2", "v1", LegalHoldStatus::On)
            .unwrap();
        registry
            .set_legal_hold("test-bucket", "key3", "v1", LegalHoldStatus::Off)
            .unwrap();

        assert_eq!(registry.legal_hold_count(), 2);
    }

    #[test]
    fn test_locked_object_count() {
        let mut registry = ObjectLockRegistry::new();
        registry
            .configure_bucket(BucketObjectLockConfig {
                bucket: "test-bucket".to_string(),
                status: ObjectLockStatus::Enabled,
                default_retention: None,
            })
            .unwrap();

        let info1 = ObjectLockInfo {
            bucket: "test-bucket".to_string(),
            key: "key1".to_string(),
            version_id: "v1".to_string(),
            retention: Some(ObjectRetention {
                mode: RetentionMode::Governance,
                retain_until: create_future_time(30),
            }),
            legal_hold: LegalHoldStatus::Off,
        };
        let info2 = ObjectLockInfo {
            bucket: "test-bucket".to_string(),
            key: "key2".to_string(),
            version_id: "v1".to_string(),
            retention: Some(ObjectRetention {
                mode: RetentionMode::Governance,
                retain_until: create_future_time(30),
            }),
            legal_hold: LegalHoldStatus::Off,
        };
        let info3 = ObjectLockInfo {
            bucket: "test-bucket".to_string(),
            key: "key3".to_string(),
            version_id: "v1".to_string(),
            retention: Some(ObjectRetention {
                mode: RetentionMode::Governance,
                retain_until: create_future_time(1),
            }),
            legal_hold: LegalHoldStatus::Off,
        };

        registry.set_retention(info1).unwrap();
        registry.set_retention(info2).unwrap();
        registry.set_retention(info3).unwrap();

        assert_eq!(registry.locked_object_count(), 3);
    }

    #[test]
    fn test_default_retention_for_bucket_days() {
        let mut registry = ObjectLockRegistry::new();
        registry
            .configure_bucket(BucketObjectLockConfig {
                bucket: "test-bucket".to_string(),
                status: ObjectLockStatus::Enabled,
                default_retention: Some(DefaultRetention {
                    mode: RetentionMode::Governance,
                    retention_period: RetentionPeriod::Days(10),
                }),
            })
            .unwrap();

        let retain_until = registry.default_retention_for_bucket("test-bucket");
        assert!(retain_until.is_some());

        let now = SystemTime::now();
        let expected = now + Duration::from_secs(60 * 60 * 24 * 10);
        let retain_time = retain_until.unwrap();
        let diff = if retain_time > expected {
            retain_time.duration_since(expected).unwrap()
        } else {
            expected.duration_since(retain_time).unwrap()
        };
        assert!(diff < Duration::from_secs(1));
    }

    #[test]
    fn test_default_retention_for_bucket_years() {
        let mut registry = ObjectLockRegistry::new();
        registry
            .configure_bucket(BucketObjectLockConfig {
                bucket: "test-bucket".to_string(),
                status: ObjectLockStatus::Enabled,
                default_retention: Some(DefaultRetention {
                    mode: RetentionMode::Compliance,
                    retention_period: RetentionPeriod::Years(1),
                }),
            })
            .unwrap();

        let retain_until = registry.default_retention_for_bucket("test-bucket");
        assert!(retain_until.is_some());

        let now = SystemTime::now();
        let expected = now + Duration::from_secs(60 * 60 * 24 * 365);
        let retain_time = retain_until.unwrap();
        let diff = if retain_time > expected {
            retain_time.duration_since(expected).unwrap()
        } else {
            expected.duration_since(retain_time).unwrap()
        };
        assert!(diff < Duration::from_secs(1));
    }

    #[test]
    fn test_default_retention_no_config() {
        let registry = ObjectLockRegistry::new();
        let result = registry.default_retention_for_bucket("nonexistent");
        assert!(result.is_none());
    }

    #[test]
    fn test_bucket_lock_not_enabled_for_retention() {
        let mut registry = ObjectLockRegistry::new();
        let info = ObjectLockInfo {
            bucket: "test-bucket".to_string(),
            key: "test-key".to_string(),
            version_id: "v1".to_string(),
            retention: Some(ObjectRetention {
                mode: RetentionMode::Governance,
                retain_until: create_future_time(30),
            }),
            legal_hold: LegalHoldStatus::Off,
        };

        let result = registry.set_retention(info);
        assert!(matches!(
            result,
            Err(ObjectLockError::BucketLockNotEnabled(_))
        ));
    }

    #[test]
    fn test_bucket_lock_not_enabled_for_legal_hold() {
        let mut registry = ObjectLockRegistry::new();

        let result = registry.set_legal_hold("test-bucket", "test-key", "v1", LegalHoldStatus::On);
        assert!(matches!(
            result,
            Err(ObjectLockError::BucketLockNotEnabled(_))
        ));
    }
}
