# Task: Write meta_fsck_security_tests.rs for claudefs-security crate

Write a comprehensive security test module for the `claudefs-meta` crate focusing on filesystem integrity checking (fsck), quota management, and multi-tenant namespace isolation.

## File location
`crates/claudefs-security/src/meta_fsck_security_tests.rs`

## Module structure
```rust
//! Meta fsck/quota/tenant security tests.
//!
//! Part of A10 Phase 13: Meta integrity & tenant security audit

#[cfg(test)]
mod tests {
    // imports and tests here
}
```

## Available types (verified from source)

```rust
use claudefs_meta::fsck::{
    FsckSeverity, FsckIssue, FsckRepairAction, FsckFinding, FsckConfig, FsckReport, suggest_repair,
};
use claudefs_meta::quota::{
    QuotaTarget, QuotaLimit, QuotaUsage, QuotaEntry, QuotaManager,
};
use claudefs_meta::tenant::{
    TenantId, TenantConfig, TenantUsage, TenantManager,
};
use claudefs_meta::{InodeId, NodeId};
use claudefs_meta::types::{FileType, Timestamp};
```

**IMPORTANT**: Not all may be public. If any import fails, remove it and skip those tests.

## Existing tests to AVOID duplicating
- `meta_security_tests.rs`: input validation, distributed locking, metadata ops, path cache
- `meta_deep_security_tests.rs`: transactions, locking, tenants (basic), quotas (basic), shard routing, journal
- `meta_consensus_security_tests.rs`: Raft, membership, leases, ReadIndex, follower reads

DO NOT duplicate these. Focus on fsck integrity, quota enforcement edge cases, and tenant isolation security.

## Test categories (25 tests total)

### Category 1: Fsck Integrity Checks (5 tests)

1. **test_fsck_config_defaults** — Create FsckConfig::default(). Verify all checks enabled (check_orphans, check_links, check_dangling, check_duplicates, check_connectivity). Verify repair is false by default. Verify max_errors == 100.

2. **test_fsck_report_clean** — Create FsckReport::default(). Verify is_clean() returns true. Verify errors == 0, warnings == 0, repaired == 0. Add a finding with severity Error. Verify is_clean() returns false.

3. **test_fsck_severity_is_error** — Verify FsckSeverity::Error.is_error() returns true. Verify FsckSeverity::Warning.is_error() returns false. Verify FsckSeverity::Info.is_error() returns false.

4. **test_fsck_suggest_repair_orphan** — Create FsckIssue::OrphanInode with inode 42. Call suggest_repair(&issue, true). Verify repair actions include RemoveInode { inode: 42 }. Call suggest_repair(&issue, false). Verify empty (repair disabled).

5. **test_fsck_suggest_repair_link_mismatch** — Create FsckIssue::LinkCountMismatch { inode: 10, expected: 3, actual: 1 }. Call suggest_repair(&issue, true). Verify returns UpdateLinkCount { inode: 10, nlink: 1 }. (FINDING: repair updates to actual count, not expected).

### Category 2: Quota Enforcement (5 tests)

6. **test_quota_limit_unlimited** — Create QuotaLimit::unlimited(). Verify max_bytes == 0 and max_inodes == 0. Verify has_byte_limit() returns false. Verify has_inode_limit() returns false.

7. **test_quota_entry_over_quota** — Create QuotaEntry with limit max_bytes=1000. Set usage bytes_used=1001. Verify is_over_quota() returns true. Set usage to 999. Verify is_over_quota() returns false.

8. **test_quota_manager_set_and_check** — Create QuotaManager::new(). Set quota for User(1000) with max_bytes=10000, max_inodes=100. Call check_quota(uid=1000, gid=0, bytes_delta=5000, inodes_delta=1). Verify Ok. Call check_quota with bytes_delta=20000 (exceeds). Verify error.

9. **test_quota_manager_update_usage** — Create manager. Set quota for User(1000). Update usage with bytes_delta=500, inodes_delta=2. Get usage for User(1000). Verify bytes_used==500 and inodes_used==2. Update with negative delta (-100 bytes). Verify bytes_used==400.

10. **test_quota_manager_over_quota_targets** — Create manager. Set quotas for User(1) with limit 100 bytes and User(2) with limit 1000 bytes. Update User(1) usage to 150 bytes (over). Update User(2) to 500 bytes (under). Call over_quota_targets(). Verify only User(1) returned.

### Category 3: Tenant Isolation (5 tests)

11. **test_tenant_create_and_list** — Create TenantManager::new(). Create tenant "acme" with root inode 100. Verify get_tenant("acme") returns correct config. Verify tenant_count() == 1. Verify list_tenants() includes "acme".

12. **test_tenant_authorization** — Create manager. Create tenant "acme" with allowed_uids=[1000, 1001] and allowed_gids=[100]. Verify is_authorized("acme", uid=1000, gid=0) returns true. Verify is_authorized("acme", uid=9999, gid=0) returns false. Verify is_authorized("acme", uid=0, gid=100) returns true (GID match).

13. **test_tenant_quota_check** — Create manager. Create tenant "acme" with max_inodes=10, max_bytes=1000. Update usage with inode_delta=5, bytes_delta=500. Verify check_tenant_quota(additional_inodes=5, additional_bytes=500) returns true (exactly at limit). Verify check_tenant_quota(additional_inodes=6, additional_bytes=0) returns false (exceeds).

14. **test_tenant_inode_assignment** — Create manager. Create tenant "acme" with root inode 100. Assign inode 200 to "acme". Verify tenant_for_inode(200) returns Some("acme"). Release inode 200. Verify tenant_for_inode(200) returns None.

15. **test_tenant_removal** — Create manager. Create tenant "acme". Assign inodes. Remove tenant "acme". Verify get_tenant("acme") returns None. Verify tenant_count() == 0. (FINDING: verify inode assignments cleaned up on removal).

### Category 4: Fsck Issues & Repair (5 tests)

16. **test_fsck_dangling_entry_repair** — Create FsckIssue::DanglingEntry { parent: InodeId::new(1), name: "ghost".to_string(), child: InodeId::new(99) }. Call suggest_repair(true). Verify returns RemoveEntry { parent: 1, name: "ghost" }.

17. **test_fsck_duplicate_entry_repair** — Create FsckIssue::DuplicateEntry { parent: InodeId::new(1), name: "dup".to_string(), inode1: InodeId::new(10), inode2: InodeId::new(20) }. Call suggest_repair(true). Document what repair is suggested for duplicates.

18. **test_fsck_disconnected_subtree_repair** — Create FsckIssue::DisconnectedSubtree { root: InodeId::new(50) }. Call suggest_repair(true). Document repair action.

19. **test_fsck_finding_display** — Create FsckFinding with severity Error and OrphanInode issue. Convert to string with Display. Verify output contains severity and inode info.

20. **test_fsck_report_accumulation** — Create FsckReport. Add 2 Error findings, 1 Warning finding. Verify errors == 2, warnings == 1. Verify is_clean() returns false. Mark one finding as repaired. Verify repaired count.

### Category 5: Quota & Tenant Edge Cases (5 tests)

21. **test_quota_usage_saturating_add** — Create QuotaUsage::new(). Call add with bytes_delta=i64::MAX. Then add again. Verify no overflow panic (should saturate).

22. **test_quota_remove_and_recheck** — Create manager. Set quota for User(1). Remove quota for User(1). Verify remove returns true. Verify get_quota(User(1)) returns None. Check quota for User(1) — should succeed (no quota = no limit).

23. **test_tenant_duplicate_create** — Create manager. Create tenant "acme". Try to create tenant "acme" again. Verify error (duplicate tenant ID).

24. **test_tenant_usage_tracking** — Create manager. Create tenant "acme". Update usage(inode_delta=3, bytes_delta=300). Get usage. Verify inode_count==3, bytes_used==300. Update with negative delta. Verify usage decreases.

25. **test_quota_group_enforcement** — Create manager. Set quota for Group(100) with max_bytes=5000. Update Group(100) usage to 4000 bytes. Check quota with gid=100, bytes_delta=2000. Verify error (would exceed group quota).

## Implementation notes
- Use `fn make_xxx()` helper functions
- Mark findings with `// FINDING-META-FSCK-XX: description`
- If a type is not public, skip that test and add an alternative
- DO NOT use any async code — all tests are synchronous
- Use `assert!`, `assert_eq!`, `matches!`
- For TenantId: TenantId::new("acme")
- For TenantConfig: TenantConfig::new(TenantId::new("acme"), InodeId::new(100), 1000, 100000, vec![1000], vec![100])
- For InodeId: InodeId::new(1) or InodeId(1)
- For QuotaTarget: QuotaTarget::User(1000) or QuotaTarget::Group(100)
- For QuotaLimit: QuotaLimit::new(max_bytes, max_inodes)

## Output format
Output ONLY the complete Rust source file. No explanations, no markdown fences.
