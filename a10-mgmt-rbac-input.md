# Task: Write mgmt_rbac_security_tests.rs for claudefs-security crate

Write a comprehensive Rust test module `mgmt_rbac_security_tests.rs` for the claudefs-security crate that tests security properties of the claudefs-mgmt crate's RBAC, audit trail, compliance, live config, and security modules.

## File location
`crates/claudefs-security/src/mgmt_rbac_security_tests.rs`

## Structure
```rust
//! Security tests for management RBAC, audit, compliance, live config, and rate limiting
//!
//! Part of A10 Phase 3: Mgmt subsystem security audit

#[cfg(test)]
mod tests {
    // imports and tests here
}
```

## Available APIs (use these exact types)

### From claudefs_mgmt::rbac
```rust
pub enum RbacError { UserNotFound(String), RoleNotFound(String), PermissionDenied { user, permission, resource } }
pub enum Permission { ViewCluster, ViewNodes, DrainNodes, ManageTiering, ManageSnapshots, ViewQuotas, ManageQuotas, ViewReplication, QueryAnalytics, ManageWebhooks, Admin }
pub struct Role { pub name: String, pub description: String, pub permissions: HashSet<Permission> }
pub struct User { pub id: String, pub username: String, pub email: Option<String>, pub roles: Vec<String>, pub active: bool, pub created_at: u64 }
pub struct RbacRegistry { /* private */ }
// Role has: new(name, description), add_permission(perm), has_permission(perm), permission_count()
// Permission has: implies(other) -> bool
// User has: new(id, username)
// RbacRegistry has: new(), add_role(role), get_role(name), remove_role(name), add_user(user), get_user(id), get_user_by_name(name), remove_user(id), assign_role(user_id, role_name), revoke_role(user_id, role_name), check_permission(user_id, permission, resource), user_permissions(user_id), list_users(), list_roles()
// Free functions: admin_role(), operator_role(), viewer_role(), tenant_admin_role()
```

### From claudefs_mgmt::audit_trail
```rust
pub enum AuditEventKind { Login, Logout, TokenCreate, TokenRevoke, QuotaChange, RoleAssign, RoleRevoke, NodeDrain, SnapshotCreate, SnapshotDelete, MigrationStart, MigrationAbort, ConfigChange, AdminCommand }
pub struct AuditEvent { pub id: u64, pub timestamp: u64, pub user: String, pub ip: String, pub kind: AuditEventKind, pub resource: String, pub detail: String, pub success: bool }
pub struct AuditFilter { pub user: Option<String>, pub kind: Option<AuditEventKind>, pub since_ts: Option<u64>, pub until_ts: Option<u64>, pub success_only: bool }
pub struct AuditTrail { /* private */ }
// AuditEvent has: new(id, timestamp, user, ip, kind, resource, detail, success)
// AuditFilter has: new(), matches(event) -> bool
// AuditTrail has: new(), record(user, ip, kind, resource, detail, success) -> u64, query(filter) -> Vec<AuditEvent>, event_count()
```

### From claudefs_mgmt::compliance
```rust
pub enum RetentionStatus { Active, Expired, Locked }
pub enum ComplianceError { PolicyAlreadyExists(String), PolicyNotFound(String), RecordNotFound(String) }
pub struct RetentionPolicy { pub policy_id: String, pub name: String, pub retention_days: u32, pub worm_enabled: bool }
pub struct RetentionRecord { pub record_id: String, pub path: String, pub policy_id: String, pub created_at_ms: u64, pub expires_at_ms: u64, pub worm_enabled: bool }
pub struct ComplianceRegistry { /* private */ }
// RetentionRecord has: status(now_ms) -> RetentionStatus, days_remaining(now_ms) -> i64
// ComplianceRegistry has: new(), add_policy(policy), get_policy(id), register_file(path, policy_id, created_at_ms), get_record(id), active_records(now_ms), expired_records(now_ms), policy_count(), record_count()
```

### From claudefs_mgmt::live_config
```rust
pub enum LiveConfigError { NotFound(String), ValidationFailed(String), ReloadInProgress, Serialize(String) }
pub enum ReloadStatus { Success { keys_updated, keys_unchanged }, PartialFailure { keys_updated, errors }, NoChanges }
pub struct LiveConfigEntry { pub key: String, pub value: String, pub version: u64, pub last_updated: u64, pub description: String }
pub struct LiveConfigStore { /* private */ }
// LiveConfigStore has: new(), set(key, value, description), get(key), keys(), version(), remove(key), reload(new_entries: HashMap<String,(String,String)>), watch(keys) -> mpsc::UnboundedReceiver<Vec<String>>, watcher_count()
// Free functions: validate_json(value), parse_entry<T>(entry)
```

### From claudefs_mgmt::security
```rust
pub fn constant_time_eq(a: &str, b: &str) -> bool;
pub struct AuthRateLimiter { /* private */ }
// AuthRateLimiter has: new(), record_failure(ip) -> bool, is_rate_limited(ip) -> bool, get_failure_count(ip) -> u32, prune()
```

## Security findings to test (25 tests total)

### A. RBAC Security (7 tests)
1. `test_rbac_admin_implies_all_permissions` — Admin permission implies() returns true for all other permissions
2. `test_rbac_non_admin_does_not_imply_admin` — ViewCluster.implies(Admin) should be false
3. `test_rbac_inactive_user_denied` — Inactive user should be denied permission check
4. `test_rbac_assign_role_to_nonexistent_user` — assign_role for missing user returns error
5. `test_rbac_assign_nonexistent_role` — assign_role with missing role returns error
6. `test_rbac_removed_user_not_accessible` — After remove_user, get_user returns None
7. `test_rbac_duplicate_role_assignment` — Assigning same role twice should not duplicate

### B. Audit Trail Security (5 tests)
8. `test_audit_record_returns_incrementing_ids` — Multiple records get incrementing IDs
9. `test_audit_filter_by_user` — Filter matches only specified user
10. `test_audit_filter_by_kind` — Filter matches only specified event kind
11. `test_audit_empty_filter_returns_all` — Empty filter returns all events
12. `test_audit_success_only_filter` — success_only=true filters out failed events

### C. Compliance Security (5 tests)
13. `test_compliance_worm_record_status_active` — New WORM record is Active before expiry
14. `test_compliance_expired_record_status` — Record past expiry is Expired
15. `test_compliance_duplicate_policy_rejected` — Adding same policy_id twice fails
16. `test_compliance_register_file_unknown_policy` — Registering with non-existent policy fails
17. `test_compliance_days_remaining_calculation` — Verify days_remaining math is correct

### D. Live Config Security (5 tests)
18. `test_live_config_set_and_get_roundtrip` — Set a key, get it back, values match
19. `test_live_config_get_nonexistent_key_error` — Getting unknown key returns NotFound
20. `test_live_config_remove_key` — After remove, get returns NotFound
21. `test_live_config_version_increments` — Each set() increments version
22. `test_live_config_reload_updates_existing` — reload() with new values updates entries

### E. Rate Limiter Security (3 tests)
23. `test_rate_limiter_locks_after_threshold` — After 5 failures, is_rate_limited returns true
24. `test_rate_limiter_different_ips_independent` — Rate limit on one IP doesn't affect another
25. `test_rate_limiter_constant_time_eq_works` — constant_time_eq("abc","abc") is true, ("abc","xyz") is false

## Important rules
- Use `#[cfg(test)]` on the outer module
- Put all tests in a `mod tests { }` block
- Import from claudefs_mgmt, not from internal paths
- For tokio async tests (live_config::watch), use `#[tokio::test]`
- Do NOT use unsafe code
- Every test should have a comment explaining the security finding it validates
- Use assert!, assert_eq!, assert_ne! for assertions
- Use `matches!()` macro for enum variant matching where needed
- Keep tests simple and focused — test one security property per test
- For HashMap in reload(), use: `use std::collections::HashMap;`

## Output format
Output ONLY the complete Rust source file contents. No markdown, no explanation, no code fences — just the raw .rs file content.
