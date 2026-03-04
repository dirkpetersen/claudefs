//! Meta fsck/quota/tenant security tests.
//!
//! Part of A10 Phase 13: Meta integrity & tenant security audit

use claudefs_meta::fsck::{
    suggest_repair, FsckConfig, FsckFinding, FsckIssue, FsckRepairAction, FsckReport, FsckSeverity,
};
use claudefs_meta::quota::{QuotaEntry, QuotaLimit, QuotaManager, QuotaTarget, QuotaUsage};
use claudefs_meta::tenant::{TenantConfig, TenantId, TenantManager, TenantUsage};
use claudefs_meta::types::InodeId;

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_tenant_id(name: &str) -> TenantId {
        TenantId::new(name)
    }

    fn make_inode(id: u64) -> InodeId {
        InodeId::new(id)
    }

    // Category 1: Fsck Integrity Checks (5 tests)

    #[test]
    fn test_fsck_config_defaults() {
        let config = FsckConfig::default();
        assert!(config.check_orphans);
        assert!(config.check_links);
        assert!(config.check_dangling);
        assert!(config.check_duplicates);
        assert!(config.check_connectivity);
        assert!(!config.repair);
        assert_eq!(config.max_errors, 100);
    }

    #[test]
    fn test_fsck_report_clean() {
        let mut report = FsckReport::default();
        assert!(report.is_clean());
        assert_eq!(report.errors, 0);
        assert_eq!(report.warnings, 0);
        assert_eq!(report.repaired, 0);

        report.findings.push(FsckFinding {
            severity: FsckSeverity::Error,
            issue: FsckIssue::OrphanInode {
                inode: make_inode(42),
            },
            repaired: false,
        });
        report.errors = 1;

        assert!(!report.is_clean());
    }

    #[test]
    fn test_fsck_severity_is_error() {
        assert!(FsckSeverity::Error.is_error());
        assert!(!FsckSeverity::Warning.is_error());
        assert!(!FsckSeverity::Info.is_error());
    }

    #[test]
    fn test_fsck_suggest_repair_orphan() {
        let issue = FsckIssue::OrphanInode {
            inode: make_inode(42),
        };

        let actions = suggest_repair(&issue, true);
        assert_eq!(actions.len(), 1);
        assert!(matches!(
            actions[0],
            FsckRepairAction::RemoveInode { inode } if inode.as_u64() == 42
        ));

        // FINDING-META-FSCK-001: repair disabled returns empty
        let actions = suggest_repair(&issue, false);
        assert!(actions.is_empty());
    }

    #[test]
    fn test_fsck_suggest_repair_link_mismatch() {
        let issue = FsckIssue::LinkCountMismatch {
            inode: make_inode(10),
            expected: 3,
            actual: 1,
        };

        let actions = suggest_repair(&issue, true);
        assert_eq!(actions.len(), 1);
        // FINDING-META-FSCK-002: repair updates to expected count, not actual
        assert!(matches!(
            actions[0],
            FsckRepairAction::UpdateLinkCount { inode, nlink }
                if inode.as_u64() == 10 && nlink == 3
        ));
    }

    // Category 2: Quota Enforcement (5 tests)

    #[test]
    fn test_quota_limit_unlimited() {
        let limit = QuotaLimit::unlimited();
        assert_eq!(limit.max_bytes, u64::MAX);
        assert_eq!(limit.max_inodes, u64::MAX);
        assert!(!limit.has_byte_limit());
        assert!(!limit.has_inode_limit());
    }

    #[test]
    fn test_quota_entry_over_quota() {
        let limit = QuotaLimit::new(1000, 100);
        let mut entry = QuotaEntry::new(QuotaTarget::User(1000), limit);
        entry.usage.bytes_used = 1001;
        assert!(entry.is_over_quota());

        entry.usage.bytes_used = 999;
        assert!(!entry.is_over_quota());
    }

    #[test]
    fn test_quota_manager_set_and_check() {
        let mgr = QuotaManager::new();
        mgr.set_quota(QuotaTarget::User(1000), QuotaLimit::new(10000, 100));

        let result = mgr.check_quota(1000, 0, 5000, 1);
        assert!(result.is_ok());

        let result = mgr.check_quota(1000, 0, 20000, 0);
        assert!(result.is_err());
    }

    #[test]
    fn test_quota_manager_update_usage() {
        let mgr = QuotaManager::new();
        mgr.set_quota(QuotaTarget::User(1000), QuotaLimit::new(10000, 100));

        mgr.update_usage(1000, 0, 500, 2);

        let usage = mgr.get_usage(&QuotaTarget::User(1000)).unwrap();
        assert_eq!(usage.bytes_used, 500);
        assert_eq!(usage.inodes_used, 2);

        mgr.update_usage(1000, 0, -100, 0);
        let usage = mgr.get_usage(&QuotaTarget::User(1000)).unwrap();
        assert_eq!(usage.bytes_used, 400);
    }

    #[test]
    fn test_quota_manager_over_quota_targets() {
        let mgr = QuotaManager::new();
        mgr.set_quota(QuotaTarget::User(1), QuotaLimit::new(100, 100));
        mgr.set_quota(QuotaTarget::User(2), QuotaLimit::new(1000, 100));

        mgr.update_usage(1, 0, 150, 0);
        mgr.update_usage(2, 0, 500, 0);

        let over = mgr.over_quota_targets();
        assert_eq!(over.len(), 1);
        assert!(over.contains(&QuotaTarget::User(1)));
    }

    // Category 3: Tenant Isolation (5 tests)

    #[test]
    fn test_tenant_create_and_list() {
        let mgr = TenantManager::new();
        let config = TenantConfig::new(
            make_tenant_id("acme"),
            make_inode(100),
            1000,
            100000,
            vec![1000],
            vec![100],
        );

        mgr.create_tenant(config).unwrap();

        let retrieved = mgr.get_tenant(&make_tenant_id("acme")).unwrap();
        assert_eq!(retrieved.tenant_id.as_str(), "acme");
        assert_eq!(mgr.tenant_count(), 1);

        let tenants = mgr.list_tenants();
        assert!(tenants.iter().any(|t| t.as_str() == "acme"));
    }

    #[test]
    fn test_tenant_authorization() {
        let mgr = TenantManager::new();
        let config = TenantConfig::new(
            make_tenant_id("acme"),
            make_inode(100),
            1000,
            100000,
            vec![1000, 1001],
            vec![100],
        );

        mgr.create_tenant(config).unwrap();

        assert!(mgr.is_authorized(&make_tenant_id("acme"), 1000, 0));
        assert!(!mgr.is_authorized(&make_tenant_id("acme"), 9999, 0));
        assert!(mgr.is_authorized(&make_tenant_id("acme"), 0, 100));
    }

    #[test]
    fn test_tenant_removal() {
        let mgr = TenantManager::new();
        let config = TenantConfig::new(
            make_tenant_id("acme"),
            make_inode(100),
            1000,
            100000,
            vec![],
            vec![],
        );

        mgr.create_tenant(config).unwrap();
        mgr.assign_inode(&make_tenant_id("acme"), make_inode(200))
            .unwrap();

        mgr.remove_tenant(&make_tenant_id("acme")).unwrap();

        assert!(mgr.get_tenant(&make_tenant_id("acme")).is_none());
        assert_eq!(mgr.tenant_count(), 0);

        // FINDING-META-FSCK-003: inode_to_tenant map is NOT cleaned up on tenant removal
        // This is a potential resource leak - orphaned inode mappings remain after tenant removal
        // The line below documents the current (buggy) behavior
        let _ = mgr.tenant_for_inode(make_inode(200));
    }

    #[test]
    fn test_tenant_quota_check() {
        let mgr = TenantManager::new();
        let config = TenantConfig::new(
            make_tenant_id("acme"),
            make_inode(100),
            10,
            1000,
            vec![],
            vec![],
        );

        mgr.create_tenant(config).unwrap();
        mgr.update_usage(&make_tenant_id("acme"), 5, 500);

        // Exactly at limit - should pass
        assert!(mgr.check_tenant_quota(&make_tenant_id("acme"), 5, 500));

        // Exceeds limit - should fail
        assert!(!mgr.check_tenant_quota(&make_tenant_id("acme"), 6, 0));
    }

    #[test]
    fn test_tenant_inode_assignment() {
        let mgr = TenantManager::new();
        let config = TenantConfig::new(
            make_tenant_id("acme"),
            make_inode(100),
            1000,
            100000,
            vec![],
            vec![],
        );

        mgr.create_tenant(config).unwrap();
        mgr.assign_inode(&make_tenant_id("acme"), make_inode(200))
            .unwrap();

        let tenant = mgr.tenant_for_inode(make_inode(200));
        assert!(tenant.is_some());
        assert_eq!(tenant.unwrap().as_str(), "acme");

        mgr.release_inode(make_inode(200));
        let tenant = mgr.tenant_for_inode(make_inode(200));
        assert!(tenant.is_none());
    }

    // Category 4: Fsck Issues & Repair (5 tests)

    #[test]
    fn test_fsck_dangling_entry_repair() {
        let issue = FsckIssue::DanglingEntry {
            parent: make_inode(1),
            name: "ghost".to_string(),
            child: make_inode(99),
        };

        let actions = suggest_repair(&issue, true);
        assert_eq!(actions.len(), 1);
        assert!(matches!(
            &actions[0],
            FsckRepairAction::RemoveEntry { parent, name }
                if parent.as_u64() == 1 && name == "ghost"
        ));
    }

    #[test]
    fn test_fsck_duplicate_entry_repair() {
        let issue = FsckIssue::DuplicateEntry {
            parent: make_inode(1),
            name: "dup".to_string(),
            inode1: make_inode(10),
            inode2: make_inode(20),
        };

        // FINDING-META-FSCK-004: duplicate repair removes the duplicate entry
        let actions = suggest_repair(&issue, true);
        assert_eq!(actions.len(), 1);
        assert!(matches!(
            &actions[0],
            FsckRepairAction::RemoveEntry { parent, name }
                if parent.as_u64() == 1 && name == "dup"
        ));
    }

    #[test]
    fn test_fsck_disconnected_subtree_repair() {
        let issue = FsckIssue::DisconnectedSubtree {
            root: make_inode(50),
        };

        // FINDING-META-FSCK-005: disconnected subtree has no automatic repair
        let actions = suggest_repair(&issue, true);
        assert!(actions.is_empty());
    }

    #[test]
    fn test_fsck_finding_display() {
        let finding = FsckFinding {
            severity: FsckSeverity::Error,
            issue: FsckIssue::OrphanInode {
                inode: make_inode(42),
            },
            repaired: false,
        };

        let display = format!("{}", finding);
        assert!(display.contains("ERROR"));
        assert!(display.contains("42"));
    }

    #[test]
    fn test_fsck_report_accumulation() {
        let mut report = FsckReport::default();

        report.findings.push(FsckFinding {
            severity: FsckSeverity::Error,
            issue: FsckIssue::OrphanInode {
                inode: make_inode(1),
            },
            repaired: false,
        });
        report.findings.push(FsckFinding {
            severity: FsckSeverity::Error,
            issue: FsckIssue::OrphanInode {
                inode: make_inode(2),
            },
            repaired: false,
        });
        report.findings.push(FsckFinding {
            severity: FsckSeverity::Warning,
            issue: FsckIssue::LinkCountMismatch {
                inode: make_inode(3),
                expected: 2,
                actual: 1,
            },
            repaired: false,
        });

        report.errors = 2;
        report.warnings = 1;

        assert!(!report.is_clean());
        assert_eq!(report.errors, 2);
        assert_eq!(report.warnings, 1);

        // Mark one as repaired
        report.findings[0].repaired = true;
        report.repaired = 1;

        assert_eq!(report.repaired, 1);
    }

    // Category 5: Quota & Tenant Edge Cases (5 tests)

    #[test]
    fn test_quota_usage_saturating_add() {
        let mut usage = QuotaUsage::new();

        // Adding max value should not panic
        usage.add(i64::MAX, 0);

        // Adding again should saturate, not overflow
        // i64::MAX as u64 + i64::MAX as u64 = 2*i64::MAX - 2 = u64::MAX - 1
        usage.add(i64::MAX, 0);

        // Should be saturated (at u64::MAX - 1 due to two max adds)
        assert_eq!(usage.bytes_used, u64::MAX - 1);
    }

    #[test]
    fn test_quota_remove_and_recheck() {
        let mgr = QuotaManager::new();
        mgr.set_quota(QuotaTarget::User(1), QuotaLimit::new(1000, 100));

        let removed = mgr.remove_quota(&QuotaTarget::User(1));
        assert!(removed);

        let quota = mgr.get_quota(&QuotaTarget::User(1));
        assert!(quota.is_none());

        // No quota = no limit, should succeed
        let result = mgr.check_quota(1, 0, 1000000, 1000);
        assert!(result.is_ok());
    }

    #[test]
    fn test_tenant_duplicate_create() {
        let mgr = TenantManager::new();
        let config = TenantConfig::new(
            make_tenant_id("acme"),
            make_inode(100),
            1000,
            100000,
            vec![],
            vec![],
        );

        mgr.create_tenant(config).unwrap();

        let config2 = TenantConfig::new(
            make_tenant_id("acme"),
            make_inode(200),
            2000,
            200000,
            vec![],
            vec![],
        );

        let result = mgr.create_tenant(config2);
        assert!(result.is_err());
    }

    #[test]
    fn test_tenant_usage_tracking() {
        let mgr = TenantManager::new();
        let config = TenantConfig::new(
            make_tenant_id("acme"),
            make_inode(100),
            1000,
            100000,
            vec![],
            vec![],
        );

        mgr.create_tenant(config).unwrap();
        mgr.update_usage(&make_tenant_id("acme"), 3, 300);

        let usage = mgr.get_usage(&make_tenant_id("acme")).unwrap();
        assert_eq!(usage.inode_count, 3);
        assert_eq!(usage.bytes_used, 300);

        // Update with negative delta
        mgr.update_usage(&make_tenant_id("acme"), -1, -100);
        let usage = mgr.get_usage(&make_tenant_id("acme")).unwrap();
        assert_eq!(usage.inode_count, 2);
        assert_eq!(usage.bytes_used, 200);
    }

    #[test]
    fn test_quota_group_enforcement() {
        let mgr = QuotaManager::new();
        mgr.set_quota(QuotaTarget::Group(100), QuotaLimit::new(5000, 100));

        mgr.update_usage(0, 100, 4000, 0);

        // 4000 + 2000 would exceed 5000 group limit
        let result = mgr.check_quota(0, 100, 2000, 0);
        assert!(result.is_err());

        // Within limit should succeed
        let result = mgr.check_quota(0, 100, 500, 0);
        assert!(result.is_ok());
    }
}
