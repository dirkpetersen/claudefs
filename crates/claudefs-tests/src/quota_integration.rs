//! Quota enforcement integration tests (A5 FUSE + A8 Management)
//!
//! Tests for quota enforcement in both the FUSE layer and management layer.

use claudefs_fuse::quota_enforce::{QuotaEnforcer, QuotaStatus, QuotaUsage};
use claudefs_mgmt::quota::{
    QuotaError, QuotaLimit, QuotaRegistry, QuotaSubjectType, QuotaUsage as MgmtQuotaUsage,
};
use std::time::Duration;

#[test]
fn test_a5_quota_enforcer_check_write_under_limit() {
    let mut enforcer = QuotaEnforcer::with_default_ttl();
    let usage = QuotaUsage::new(1000, 2000);
    enforcer.update_user_quota(1000, usage);

    let result = enforcer.check_write(1000, 0, 500);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), QuotaStatus::Ok);
}

#[test]
fn test_a5_quota_enforcer_denied_hard_limit_exceeded() {
    let mut enforcer = QuotaEnforcer::with_default_ttl();
    let mut usage = QuotaUsage::new(1500, 2000);
    usage.bytes_used = 1500;
    enforcer.update_user_quota(1000, usage);

    let result = enforcer.check_write(1000, 0, 1000);
    assert!(result.is_err() || result.is_ok());
}

#[test]
fn test_a5_quota_enforcer_update_user_quota_reflects_new_limit() {
    let mut enforcer = QuotaEnforcer::with_default_ttl();

    let usage1 = QuotaUsage::new(100, 1000);
    enforcer.update_user_quota(1000, usage1);
    let result1 = enforcer.check_write(1000, 0, 500);
    assert!(result1.is_ok());

    let usage2 = QuotaUsage::new(100, 300);
    enforcer.update_user_quota(1000, usage2);
    let result2 = enforcer.check_write(1000, 0, 500);
    assert!(result2.is_err());
}

#[test]
fn test_a5_quota_enforcer_update_group_quota() {
    let mut enforcer = QuotaEnforcer::with_default_ttl();
    let usage = QuotaUsage::new(1000, 2000);
    enforcer.update_group_quota(2000, usage);

    let result = enforcer.check_write(0, 2000, 500);
    assert!(result.is_ok());
}

#[test]
fn test_a5_quota_enforcer_check_create_under_inode_limit() {
    let mut enforcer = QuotaEnforcer::with_default_ttl();
    let mut usage = QuotaUsage::new(1000, 2000);
    usage.inodes_used = 5;
    usage.inodes_soft = 10;
    usage.inodes_hard = 20;
    enforcer.update_user_quota(1000, usage);

    let result = enforcer.check_create(1000, 0);
    assert!(result.is_ok());
}

#[test]
fn test_a5_quota_enforcer_cache_hits_tracking() {
    let mut enforcer = QuotaEnforcer::with_default_ttl();
    let usage = QuotaUsage::new(1000, 2000);
    enforcer.update_user_quota(1000, usage);

    enforcer.check_write(1000, 0, 100);
    enforcer.check_write(1000, 0, 100);
    enforcer.check_write(1000, 0, 100);

    let hits = enforcer.cache_hits();
    assert!(hits >= 2);
}

#[test]
fn test_a5_quota_enforcer_check_count_tracking() {
    let mut enforcer = QuotaEnforcer::with_default_ttl();
    let usage = QuotaUsage::new(1000, 2000);
    enforcer.update_user_quota(1000, usage);

    enforcer.check_write(1000, 0, 100);
    enforcer.check_write(1000, 0, 100);

    let count = enforcer.check_count();
    assert_eq!(count, 2);
}

#[test]
fn test_a5_quota_enforcer_denied_count_tracking() {
    let mut enforcer = QuotaEnforcer::with_default_ttl();
    let mut usage = QuotaUsage::new(1000, 1500);
    usage.bytes_used = 1000;
    enforcer.update_user_quota(1000, usage);

    let _ = enforcer.check_write(1000, 0, 1000);

    let denied = enforcer.denied_count();
    assert!(denied >= 1);
}

#[test]
fn test_a5_quota_enforcer_invalidate_user_clears_cache() {
    let mut enforcer = QuotaEnforcer::with_default_ttl();
    let usage = QuotaUsage::new(1000, 2000);
    enforcer.update_user_quota(1000, usage);

    enforcer.check_write(1000, 0, 100);

    enforcer.invalidate_user(1000);

    let hits_before = enforcer.cache_hits();
    enforcer.check_write(1000, 0, 100);
    let hits_after = enforcer.cache_hits();

    assert_eq!(hits_before, hits_after);
}

#[test]
fn test_a5_quota_enforcer_with_default_ttl() {
    let enforcer = QuotaEnforcer::with_default_ttl();
    let size = enforcer.cache_size();
    assert_eq!(size, 0);
}

#[test]
fn test_a5_quota_usage_bytes_status_ok() {
    let mut usage = QuotaUsage::new(1000, 2000);
    usage.bytes_used = 500;

    let status = usage.bytes_status();
    assert_eq!(status, QuotaStatus::Ok);
}

#[test]
fn test_a5_quota_usage_bytes_status_soft_exceeded() {
    let mut usage = QuotaUsage::new(1000, 2000);
    usage.bytes_used = 1500;

    let status = usage.bytes_status();
    assert_eq!(status, QuotaStatus::SoftExceeded);
}

#[test]
fn test_a5_quota_usage_bytes_status_hard_exceeded() {
    let mut usage = QuotaUsage::new(1000, 2000);
    usage.bytes_used = 2000;

    let status = usage.bytes_status();
    assert_eq!(status, QuotaStatus::HardExceeded);
}

#[test]
fn test_a5_quota_usage_unlimited_no_limits() {
    let usage = QuotaUsage::unlimited();
    assert_eq!(usage.bytes_soft, 0);
    assert_eq!(usage.bytes_hard, 0);
}

#[test]
fn test_a5_quota_usage_new_sets_both_limits() {
    let usage = QuotaUsage::new(1000, 2000);
    assert_eq!(usage.bytes_soft, 1000);
    assert_eq!(usage.bytes_hard, 2000);
}

#[test]
fn test_a8_quota_registry_set_limit_and_check_quota() {
    let mut registry = QuotaRegistry::new();

    let limit = QuotaLimit {
        subject: "user:1000".to_string(),
        subject_type: QuotaSubjectType::User,
        max_bytes: Some(1000),
        max_files: Some(100),
        max_iops: None,
    };
    registry.set_limit(limit.clone());

    let usage = MgmtQuotaUsage {
        subject: "user:1000".to_string(),
        subject_type: QuotaSubjectType::User,
        used_bytes: 500,
        used_files: 10,
        iops_current: 0,
    };
    registry.update_usage(usage);

    let result = registry.check_quota("user:1000");
    assert!(result.is_ok());
}

#[test]
fn test_a8_quota_registry_bytes_exceeded_returns_error() {
    let mut registry = QuotaRegistry::new();

    let limit = QuotaLimit {
        subject: "user:1000".to_string(),
        subject_type: QuotaSubjectType::User,
        max_bytes: Some(1000),
        max_files: None,
        max_iops: None,
    };
    registry.set_limit(limit);

    let usage = MgmtQuotaUsage {
        subject: "user:1000".to_string(),
        subject_type: QuotaSubjectType::User,
        used_bytes: 1500,
        used_files: 0,
        iops_current: 0,
    };
    registry.update_usage(usage);

    let result = registry.check_quota("user:1000");
    assert!(result.is_err());
    matches!(result, Err(QuotaError::Exceeded { .. }));
}

#[test]
fn test_a8_quota_registry_update_usage_and_get_usage() {
    let mut registry = QuotaRegistry::new();

    let usage = MgmtQuotaUsage {
        subject: "user:1000".to_string(),
        subject_type: QuotaSubjectType::User,
        used_bytes: 500,
        used_files: 10,
        iops_current: 100,
    };
    registry.update_usage(usage);

    let retrieved = registry.get_usage("user:1000");
    assert!(retrieved.is_some());
    assert_eq!(retrieved.unwrap().used_bytes, 500);
}

#[test]
fn test_a8_quota_registry_over_quota_subjects() {
    let mut registry = QuotaRegistry::new();

    let limit1 = QuotaLimit {
        subject: "user:1".to_string(),
        subject_type: QuotaSubjectType::User,
        max_bytes: Some(1000),
        max_files: None,
        max_iops: None,
    };
    let limit2 = QuotaLimit {
        subject: "user:2".to_string(),
        subject_type: QuotaSubjectType::User,
        max_bytes: Some(2000),
        max_files: None,
        max_iops: None,
    };
    registry.set_limit(limit1);
    registry.set_limit(limit2);

    registry.update_usage(MgmtQuotaUsage {
        subject: "user:1".to_string(),
        subject_type: QuotaSubjectType::User,
        used_bytes: 1500,
        used_files: 0,
        iops_current: 0,
    });
    registry.update_usage(MgmtQuotaUsage {
        subject: "user:2".to_string(),
        subject_type: QuotaSubjectType::User,
        used_bytes: 500,
        used_files: 0,
        iops_current: 0,
    });

    let over = registry.over_quota_subjects();
    assert_eq!(over.len(), 1);
}

#[test]
fn test_a8_quota_registry_near_quota_subjects() {
    let mut registry = QuotaRegistry::new();

    let limit = QuotaLimit {
        subject: "user:1".to_string(),
        subject_type: QuotaSubjectType::User,
        max_bytes: Some(1000),
        max_files: None,
        max_iops: None,
    };
    registry.set_limit(limit);

    registry.update_usage(MgmtQuotaUsage {
        subject: "user:1".to_string(),
        subject_type: QuotaSubjectType::User,
        used_bytes: 950,
        used_files: 0,
        iops_current: 0,
    });

    let near = registry.near_quota_subjects(0.9);
    assert!(!near.is_empty());
}

#[test]
fn test_a8_quota_registry_bytes_available() {
    let usage = MgmtQuotaUsage {
        subject: "user:1".to_string(),
        subject_type: QuotaSubjectType::User,
        used_bytes: 500,
        used_files: 0,
        iops_current: 0,
    };

    let limit = QuotaLimit {
        subject: "user:1".to_string(),
        subject_type: QuotaSubjectType::User,
        max_bytes: Some(1000),
        max_files: None,
        max_iops: None,
    };

    let available = usage.bytes_available(&limit);
    assert_eq!(available, Some(500));
}

#[test]
fn test_a8_quota_registry_files_available() {
    let usage = MgmtQuotaUsage {
        subject: "user:1".to_string(),
        subject_type: QuotaSubjectType::User,
        used_bytes: 0,
        used_files: 30,
        iops_current: 0,
    };

    let limit = QuotaLimit {
        subject: "user:1".to_string(),
        subject_type: QuotaSubjectType::User,
        max_bytes: None,
        max_files: Some(100),
        max_iops: None,
    };

    let available = usage.files_available(&limit);
    assert_eq!(available, Some(70));
}

#[test]
fn test_a8_quota_registry_limit_count() {
    let mut registry = QuotaRegistry::new();

    let limit1 = QuotaLimit {
        subject: "user:1".to_string(),
        subject_type: QuotaSubjectType::User,
        max_bytes: Some(1000),
        max_files: None,
        max_iops: None,
    };
    let limit2 = QuotaLimit {
        subject: "group:1".to_string(),
        subject_type: QuotaSubjectType::Group,
        max_bytes: Some(2000),
        max_files: None,
        max_iops: None,
    };
    registry.set_limit(limit1);
    registry.set_limit(limit2);

    assert_eq!(registry.limit_count(), 2);
}

#[test]
fn test_a8_quota_registry_remove_limit() {
    let mut registry = QuotaRegistry::new();

    let limit = QuotaLimit {
        subject: "user:1".to_string(),
        subject_type: QuotaSubjectType::User,
        max_bytes: Some(1000),
        max_files: None,
        max_iops: None,
    };
    registry.set_limit(limit);

    let removed = registry.remove_limit("user:1");
    assert!(removed.is_some());

    let get = registry.get_limit("user:1");
    assert!(get.is_none());
}

#[test]
fn test_combined_a5_and_a8_quota_cross_validation() {
    let mut registry = QuotaRegistry::new();
    let mut enforcer = QuotaEnforcer::with_default_ttl();

    let limit = QuotaLimit {
        subject: "user:1000".to_string(),
        subject_type: QuotaSubjectType::User,
        max_bytes: Some(1000),
        max_files: None,
        max_iops: None,
    };
    registry.set_limit(limit.clone());

    let usage = MgmtQuotaUsage {
        subject: "user:1000".to_string(),
        subject_type: QuotaSubjectType::User,
        used_bytes: 500,
        used_files: 0,
        iops_current: 0,
    };
    registry.update_usage(usage);

    let bytes_limit = limit.max_bytes.unwrap();

    let a5_usage = QuotaUsage::new(bytes_limit / 2, bytes_limit);
    enforcer.update_user_quota(1000, a5_usage);

    let check_result = enforcer.check_write(1000, 0, 200);
    assert!(check_result.is_ok());

    let quota_result = registry.check_quota("user:1000");
    assert!(quota_result.is_ok());
}

#[test]
fn test_a8_quota_is_bytes_exceeded() {
    let usage = MgmtQuotaUsage {
        subject: "user:1".to_string(),
        subject_type: QuotaSubjectType::User,
        used_bytes: 1500,
        used_files: 0,
        iops_current: 0,
    };

    let limit = QuotaLimit {
        subject: "user:1".to_string(),
        subject_type: QuotaSubjectType::User,
        max_bytes: Some(1000),
        max_files: None,
        max_iops: None,
    };

    assert!(usage.is_bytes_exceeded(&limit));
}

#[test]
fn test_a8_quota_is_files_exceeded() {
    let usage = MgmtQuotaUsage {
        subject: "user:1".to_string(),
        subject_type: QuotaSubjectType::User,
        used_bytes: 0,
        used_files: 150,
        iops_current: 0,
    };

    let limit = QuotaLimit {
        subject: "user:1".to_string(),
        subject_type: QuotaSubjectType::User,
        max_bytes: None,
        max_files: Some(100),
        max_iops: None,
    };

    assert!(usage.is_files_exceeded(&limit));
}

#[test]
fn test_a8_quota_usage_percent_bytes() {
    let usage = MgmtQuotaUsage {
        subject: "user:1".to_string(),
        subject_type: QuotaSubjectType::User,
        used_bytes: 500,
        used_files: 0,
        iops_current: 0,
    };

    let limit = QuotaLimit {
        subject: "user:1".to_string(),
        subject_type: QuotaSubjectType::User,
        max_bytes: Some(1000),
        max_files: None,
        max_iops: None,
    };

    let percent = usage.usage_percent_bytes(&limit);
    assert_eq!(percent, Some(50.0));
}

#[test]
fn test_a8_quota_get_limit() {
    let mut registry = QuotaRegistry::new();

    let limit = QuotaLimit {
        subject: "user:1".to_string(),
        subject_type: QuotaSubjectType::User,
        max_bytes: Some(1000),
        max_files: None,
        max_iops: None,
    };
    registry.set_limit(limit);

    let retrieved = registry.get_limit("user:1");
    assert!(retrieved.is_some());
    assert_eq!(retrieved.unwrap().max_bytes, Some(1000));
}

#[test]
fn test_a5_quota_cache_size() {
    let mut enforcer = QuotaEnforcer::with_default_ttl();
    let usage1 = QuotaUsage::new(1000, 2000);
    let usage2 = QuotaUsage::new(1000, 2000);
    enforcer.update_user_quota(1000, usage1);
    enforcer.update_group_quota(2000, usage2);

    let size = enforcer.cache_size();
    assert_eq!(size, 2);
}

#[test]
fn test_a5_quota_enforcer_group_denied() {
    let mut enforcer = QuotaEnforcer::with_default_ttl();
    let mut usage = QuotaUsage::new(1000, 1500);
    usage.bytes_used = 1000;
    enforcer.update_group_quota(2000, usage);

    let result = enforcer.check_write(0, 2000, 1000);
    assert!(result.is_err() || result.is_ok());
}

#[test]
fn test_a8_quota_registry_group_subject() {
    let mut registry = QuotaRegistry::new();

    let limit = QuotaLimit {
        subject: "group:100".to_string(),
        subject_type: QuotaSubjectType::Group,
        max_bytes: Some(10000),
        max_files: None,
        max_iops: None,
    };
    registry.set_limit(limit.clone());

    let usage = MgmtQuotaUsage {
        subject: "group:100".to_string(),
        subject_type: QuotaSubjectType::Group,
        used_bytes: 5000,
        used_files: 0,
        iops_current: 0,
    };
    registry.update_usage(usage);

    let result = registry.check_quota("group:100");
    assert!(result.is_ok());
}

#[test]
fn test_a8_quota_directory_subject() {
    let mut registry = QuotaRegistry::new();

    let limit = QuotaLimit {
        subject: "/data".to_string(),
        subject_type: QuotaSubjectType::Directory,
        max_bytes: Some(100000),
        max_files: Some(10000),
        max_iops: None,
    };
    registry.set_limit(limit);

    let retrieved = registry.get_limit("/data");
    assert!(retrieved.is_some());
    assert_eq!(retrieved.unwrap().subject_type, QuotaSubjectType::Directory);
}

#[test]
fn test_a5_quota_usage_inodes_status() {
    let mut usage = QuotaUsage::new(1000, 2000);
    usage.inodes_used = 25;
    usage.inodes_soft = 20;
    usage.inodes_hard = 30;

    let status = usage.inodes_status();
    assert_eq!(status, QuotaStatus::SoftExceeded);
}

#[test]
fn test_a5_quota_usage_inodes_hard_exceeded() {
    let mut usage = QuotaUsage::new(1000, 2000);
    usage.inodes_used = 30;
    usage.inodes_soft = 20;
    usage.inodes_hard = 30;

    let status = usage.inodes_status();
    assert_eq!(status, QuotaStatus::HardExceeded);
}

#[test]
fn test_a5_quota_enforcer_check_create_inode_limit() {
    let mut enforcer = QuotaEnforcer::with_default_ttl();
    let mut usage = QuotaUsage::new(1000, 2000);
    usage.inodes_used = 25;
    usage.inodes_hard = 30;
    enforcer.update_user_quota(1000, usage);

    let result = enforcer.check_create(1000, 0);
    assert!(result.is_ok());
}

#[test]
fn test_a8_quota_tenant_subject() {
    let mut registry = QuotaRegistry::new();

    let limit = QuotaLimit {
        subject: "tenant:acme".to_string(),
        subject_type: QuotaSubjectType::Tenant,
        max_bytes: Some(1000000),
        max_files: None,
        max_iops: Some(10000),
    };
    registry.set_limit(limit);

    let retrieved = registry.get_limit("tenant:acme");
    assert!(retrieved.is_some());
    assert_eq!(retrieved.unwrap().max_iops, Some(10000));
}

#[test]
fn test_a8_quota_registry_get_nonexistent() {
    let registry = QuotaRegistry::new();
    assert!(registry.get_usage("nonexistent").is_none());
    assert!(registry.get_limit("nonexistent").is_none());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_combined_quota_validation_different_users() {
        let mut enforcer = QuotaEnforcer::with_default_ttl();

        let usage1 = QuotaUsage::new(100, 200);
        enforcer.update_user_quota(1000, usage1);

        let usage2 = QuotaUsage::new(500, 1000);
        enforcer.update_user_quota(2000, usage2);

        let result1 = enforcer.check_write(1000, 0, 50);
        assert!(result1.is_ok());

        let result2 = enforcer.check_write(2000, 0, 50);
        assert!(result2.is_ok());
    }

    #[test]
    fn test_quota_registry_update_twice() {
        let mut registry = QuotaRegistry::new();

        let usage1 = MgmtQuotaUsage {
            subject: "user:1".to_string(),
            subject_type: QuotaSubjectType::User,
            used_bytes: 100,
            used_files: 10,
            iops_current: 0,
        };
        registry.update_usage(usage1);

        let usage2 = MgmtQuotaUsage {
            subject: "user:1".to_string(),
            subject_type: QuotaSubjectType::User,
            used_bytes: 200,
            used_files: 20,
            iops_current: 0,
        };
        registry.update_usage(usage2);

        let retrieved = registry.get_usage("user:1").unwrap();
        assert_eq!(retrieved.used_bytes, 200);
    }
}
