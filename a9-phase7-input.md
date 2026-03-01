# A9 Phase 7: Production Readiness Test Suite

You are implementing Phase 7 of the A9 Test & Validation crate for **ClaudeFS**, a distributed POSIX filesystem in Rust. This is the **Production Readiness** phase (Project Phase 3).

## Context

The test crate `crates/claudefs-tests` currently has 834 passing tests across 29 modules. You must add **5 new test modules** targeting production readiness:

1. `security_integration.rs` — A6 Phase 7 security hardening validation
2. `quota_integration.rs` — Cross-crate quota enforcement
3. `mgmt_integration.rs` — A8 management API validation
4. `acl_integration.rs` — POSIX ACL + fallocate testing
5. `perf_regression.rs` — Performance regression framework

## Crate Location

All new files go in: `crates/claudefs-tests/src/`

The `Cargo.toml` for `claudefs-tests` already includes these dependencies:
- `claudefs-fuse`, `claudefs-repl`, `claudefs-gateway`, `claudefs-storage`, `claudefs-meta`, `claudefs-transport`, `claudefs-reduce`, `claudefs-mgmt`
- `bincode`, `proptest`, `anyhow`, `tokio`, `libc`

## CRITICAL: Public API Reference

You MUST use ONLY these exact public APIs (already verified against source). Do NOT invent types or methods.

### A6 Replication — `claudefs_repl`:

**`tls_policy` module:**
```rust
use claudefs_repl::tls_policy::{TlsMode, TlsValidator, TlsPolicyBuilder, TlsConfigRef, TlsPolicyError, validate_tls_config};
// TlsMode: Required | TestOnly | Disabled
// TlsValidator::new(mode: TlsMode) -> Self
// TlsValidator::mode(&self) -> &TlsMode
// TlsValidator::is_plaintext_allowed(&self) -> bool
// TlsValidator::validate_config(&self, tls: &Option<TlsConfigRef>) -> Result<(), TlsPolicyError>
// TlsPolicyBuilder::new() -> Self, .mode(TlsMode) -> Self, .build() -> TlsValidator
// validate_tls_config(cert_pem: &[u8], key_pem: &[u8], ca_pem: &[u8]) -> Result<(), TlsPolicyError>
// TlsConfigRef { cert_pem: Vec<u8>, key_pem: Vec<u8>, ca_pem: Vec<u8> }
```

**`site_registry` module:**
```rust
use claudefs_repl::site_registry::{SiteRecord, SiteRegistry, SiteRegistryError};
// SiteRecord::new(site_id: u64, display_name: &str) -> Self
// SiteRecord { site_id, display_name, tls_fingerprint: Option<[u8;32]>, addresses: Vec<String>, added_at_us, last_seen_us }
// SiteRegistry (struct with internal HashMap)
// SiteRegistry::new() -> Self
// SiteRegistry::register(&mut self, record: SiteRecord) -> Result<(), SiteRegistryError>
// SiteRegistry::unregister(&mut self, site_id: u64) -> Result<SiteRecord, SiteRegistryError>
// SiteRegistry::lookup(&self, site_id: u64) -> Option<&SiteRecord>
// SiteRegistry::verify_source_id(&self, site_id: u64, fingerprint: &[u8;32]) -> Result<(), SiteRegistryError>
// SiteRegistry::update_last_seen(&mut self, site_id: u64, now_us: u64) -> Result<(), SiteRegistryError>
// SiteRegistryError: AlreadyRegistered{site_id}, NotFound{site_id}, FingerprintMismatch{site_id}
```

**`recv_ratelimit` module:**
```rust
use claudefs_repl::recv_ratelimit::{RateLimitConfig, RateLimitDecision, RecvRateLimiter, RateLimiterStats};
// RateLimitConfig::new(max_batches_per_sec: u64, max_entries_per_sec: u64) -> Self
// RateLimitConfig { max_batches_per_sec, max_entries_per_sec, burst_factor: f64, window_ms: u64 }
// RecvRateLimiter::new(config: RateLimitConfig) -> Self
// RecvRateLimiter::check_batch(&mut self, entry_count: usize, now_ms: u64) -> RateLimitDecision
// RecvRateLimiter::reset(&mut self)
// RecvRateLimiter::stats(&self) -> &RateLimiterStats
// RecvRateLimiter::config(&self) -> &RateLimitConfig
// RateLimitDecision: Allow | Throttle { delay_ms: u64 } | Reject { reason: String }
// RateLimiterStats { batches_allowed, batches_throttled, batches_rejected, entries_allowed, entries_rejected, windows_reset }
```

**`journal_gc` module:**
```rust
use claudefs_repl::journal_gc::{GcPolicy, AckRecord, GcCandidate, GcStats, JournalGcState, JournalGcScheduler};
// GcPolicy: RetainAll | RetainByAge { max_age_us: u64 } | RetainByCount { max_entries: usize } | RetainByAck
// AckRecord { site_id: u64, acked_through_seq: u64, acked_at_us: u64 }
// GcCandidate { shard_id: u32, seq: u64, timestamp_us: u64, size_bytes: usize }
// GcStats { entries_gc_collected, bytes_gc_collected, gc_runs, last_gc_us }
// JournalGcState::new(policy: GcPolicy) -> Self
// JournalGcState::record_ack(&mut self, site_id: u64, acked_through_seq: u64, timestamp_us: u64)
// JournalGcState::get_ack(&self, site_id: u64) -> Option<&AckRecord>
// JournalGcState::min_acked_seq(&self, site_ids: &[u64]) -> Option<u64>
// JournalGcState::all_sites_acked(&self, seq: u64, site_ids: &[u64]) -> bool
// JournalGcState::policy(&self) -> &GcPolicy
// JournalGcState::site_count(&self) -> usize
// JournalGcScheduler::new(policy: GcPolicy, known_sites: Vec<u64>) -> Self
// JournalGcScheduler::record_ack(&mut self, ack: AckRecord)
// JournalGcScheduler::should_gc_entry(&self, candidate: &GcCandidate, now_us: u64) -> bool
// JournalGcScheduler::run_gc(&mut self, candidates: &[GcCandidate], now_us: u64) -> Vec<GcCandidate>
// JournalGcScheduler::stats(&self) -> &GcStats
// JournalGcScheduler::total_gc_entries(&self) -> u64
```

### A5 FUSE — `claudefs_fuse`:

**`quota_enforce` module:**
```rust
use claudefs_fuse::quota_enforce::{QuotaEnforcer, QuotaUsage, QuotaStatus};
use std::time::Duration;
// QuotaUsage::new(bytes_soft: u64, bytes_hard: u64) -> Self
// QuotaUsage::unlimited() -> Self
// QuotaUsage { bytes_used, bytes_soft, bytes_hard, inodes_used, inodes_soft, inodes_hard }
// QuotaUsage::bytes_status(&self) -> QuotaStatus
// QuotaUsage::inodes_status(&self) -> QuotaStatus
// QuotaStatus: Ok | SoftExceeded | HardExceeded
// QuotaEnforcer::new(ttl: Duration) -> Self
// QuotaEnforcer::with_default_ttl() -> Self
// QuotaEnforcer::update_user_quota(&mut self, uid: u32, usage: QuotaUsage)
// QuotaEnforcer::update_group_quota(&mut self, gid: u32, usage: QuotaUsage)
// QuotaEnforcer::check_write(&mut self, uid: u32, gid: u32, write_size: u64) -> Result<QuotaStatus>
// QuotaEnforcer::check_create(&mut self, uid: u32, gid: u32) -> Result<QuotaStatus>
// QuotaEnforcer::invalidate_user(&mut self, uid: u32)
// QuotaEnforcer::invalidate_group(&mut self, gid: u32)
// QuotaEnforcer::cache_hits(&self) -> u64
// QuotaEnforcer::check_count(&self) -> u64
// QuotaEnforcer::denied_count(&self) -> u64
// QuotaEnforcer::cache_size(&self) -> usize
```

**`posix_acl` module:**
```rust
use claudefs_fuse::posix_acl::{AclTag, AclPerms, PosixAcl, XATTR_POSIX_ACL_ACCESS, XATTR_POSIX_ACL_DEFAULT};
// AclTag: UserObj | User(u32) | GroupObj | Group(u32) | Mask | Other
// AclPerms::from_bits(bits: u8) -> Self
// AclPerms::to_bits(&self) -> u8
// AclPerms::all() -> Self
// AclPerms::none() -> Self
// AclPerms::read_only() -> Self
// AclPerms { read: bool, write: bool, execute: bool }
// PosixAcl (struct with entries)
// PosixAcl::new() -> Self
// PosixAcl::add_entry(&mut self, tag: AclTag, perms: AclPerms)
// PosixAcl::check_access(&self, uid: u32, file_uid: u32, gid: u32, file_gid: u32, req: AclPerms) -> bool
// PosixAcl::effective_perms(&self, tag: &AclTag) -> AclPerms
// XATTR_POSIX_ACL_ACCESS: &str
// XATTR_POSIX_ACL_DEFAULT: &str
```

**`fallocate` module:**
```rust
use claudefs_fuse::fallocate::{FallocateOp, FallocateStats, FALLOC_FL_KEEP_SIZE, FALLOC_FL_PUNCH_HOLE, FALLOC_FL_ZERO_RANGE, FALLOC_FL_COLLAPSE_RANGE, FALLOC_FL_INSERT_RANGE};
// FallocateOp: Allocate | PunchHole | ZeroRange | CollapseRange | InsertRange
// FallocateOp::from_flags(flags: u32) -> Result<Self, ...>  (returns an error type)
// FallocateOp::is_space_saving(&self) -> bool
// FallocateOp::modifies_size(&self) -> bool
// FallocateOp::affected_range(&self, offset: u64, length: u64) -> (u64, u64)
// FallocateStats (struct) - default constructible, tracks counts
```

### A8 Management — `claudefs_mgmt`:

**`quota` module:**
```rust
use claudefs_mgmt::quota::{QuotaError, QuotaSubjectType, QuotaLimit, QuotaUsage as MgmtQuotaUsage, QuotaRegistry};
// QuotaSubjectType: User | Group | Directory | Tenant
// QuotaLimit { subject: String, subject_type: QuotaSubjectType, max_bytes: Option<u64>, max_files: Option<u64>, max_iops: Option<u64> }
// MgmtQuotaUsage { subject, subject_type, used_bytes, used_files, iops_current }
// MgmtQuotaUsage::bytes_available(&self, limit: &QuotaLimit) -> Option<u64>
// MgmtQuotaUsage::files_available(&self, limit: &QuotaLimit) -> Option<u64>
// MgmtQuotaUsage::is_bytes_exceeded(&self, limit: &QuotaLimit) -> bool
// MgmtQuotaUsage::is_files_exceeded(&self, limit: &QuotaLimit) -> bool
// MgmtQuotaUsage::usage_percent_bytes(&self, limit: &QuotaLimit) -> Option<f64>
// QuotaRegistry::new() -> Self
// QuotaRegistry::set_limit(&mut self, limit: QuotaLimit)
// QuotaRegistry::remove_limit(&mut self, subject: &str) -> Option<QuotaLimit>
// QuotaRegistry::get_limit(&self, subject: &str) -> Option<&QuotaLimit>
// QuotaRegistry::update_usage(&mut self, usage: MgmtQuotaUsage)
// QuotaRegistry::get_usage(&self, subject: &str) -> Option<&MgmtQuotaUsage>
// QuotaRegistry::check_quota(&self, subject: &str) -> Result<(), QuotaError>
// QuotaRegistry::over_quota_subjects(&self) -> Vec<(&QuotaLimit, &MgmtQuotaUsage)>
// QuotaRegistry::near_quota_subjects(&self, threshold: f64) -> Vec<(&QuotaLimit, &MgmtQuotaUsage)>
// QuotaRegistry::limit_count(&self) -> usize
```

**`rbac` module:**
```rust
use claudefs_mgmt::rbac::{RbacError, Permission, Role, User, RbacRegistry, admin_role, operator_role, viewer_role, tenant_admin_role};
// Permission: ReadData | WriteData | DeleteData | ManageQuota | ManageUsers | ViewMetrics | ManageConfig | Admin
// Permission::implies(&self, other: &Permission) -> bool
// Role { name, description, permissions: HashSet<Permission> }
// Role::new(name: String, description: String) -> Self
// Role::add_permission(&mut self, perm: Permission)
// Role::has_permission(&self, perm: &Permission) -> bool
// Role::permission_count(&self) -> usize
// User { id, username, email: Option<String>, roles: Vec<String>, active, created_at }
// User::new(id: String, username: String) -> Self
// RbacRegistry::new() -> Self
// RbacRegistry::add_role(&mut self, role: Role)
// RbacRegistry::get_role(&self, name: &str) -> Option<&Role>
// RbacRegistry::remove_role(&mut self, name: &str) -> Option<Role>
// RbacRegistry::add_user(&mut self, user: User)
// RbacRegistry::get_user(&self, user_id: &str) -> Option<&User>
// admin_role() -> Role, operator_role() -> Role, viewer_role() -> Role, tenant_admin_role() -> Role
```

**`sla` module:**
```rust
use claudefs_mgmt::sla::{SlaMetricKind, SlaTarget, LatencySample, PercentileResult, SlaViolation, SlaCheckResult, compute_percentiles};
// SlaMetricKind: ReadLatency | WriteLatency | MetadataLatency | ThroughputMbps | Iops
// SlaMetricKind::name(&self) -> &'static str
// SlaTarget::new(kind: SlaMetricKind, p50: f64, p95: f64, p99: f64, description: String) -> Self
// SlaTarget { kind, p50_threshold, p95_threshold, p99_threshold, description }
// LatencySample::new(value_us: u64, timestamp: u64) -> Self
// compute_percentiles(samples: &[u64]) -> Option<PercentileResult>
// PercentileResult { p50, p95, p99, p999, min, max, mean, sample_count }
// SlaViolation: P50Exceeded{..} | P95Exceeded{..} | P99Exceeded{..}
// SlaCheckResult { target, percentiles, violations, compliant, checked_at }
```

**`alerting` module:**
```rust
use claudefs_mgmt::alerting::{AlertSeverity, AlertState, AlertRule, Comparison, Alert, AlertManager, AlertError, default_alert_rules};
// AlertSeverity: Info | Warning | Critical
// AlertState: Pending | Firing | Resolved
// Comparison: GreaterThan | LessThan | GreaterThanOrEqual | LessThanOrEqual
// Comparison::evaluate(&self, metric_value: f64) -> bool  -- note: takes &self only? Check again
// AlertRule { name, description, severity, metric, threshold, comparison, for_secs }
// Alert { rule, state, value, firing_since, resolved_at, message, labels }
// Alert::new(rule: AlertRule, value: f64) -> Self
// Alert::is_firing(&self) -> bool
// Alert::is_resolved(&self) -> bool
// Alert::age_secs(&self) -> u64
// AlertManager::new(rules: Vec<AlertRule>) -> Self
// AlertManager::with_default_rules() -> Self
// AlertManager::evaluate(&mut self, metrics: &HashMap<String, f64>) -> Vec<Alert>
// default_alert_rules() -> Vec<AlertRule>
```

## Files to Create

### File 1: `crates/claudefs-tests/src/security_integration.rs`

**Purpose:** Integration tests for A6 Phase 7 security hardening features.

**~28 tests covering:**
- TLS policy validation: Required mode rejects None config and empty certs; TestOnly allows None; Disabled allows None
- TlsPolicyBuilder: default mode, setting Required, building validator
- validate_tls_config: valid PEM-like data passes, empty cert/key fails, missing "-----BEGIN" prefix fails
- Site registry: register/lookup/unregister lifecycle; AlreadyRegistered error on duplicate; NotFound error
- Site registry fingerprint: verify_source_id passes with matching fingerprint; fails with mismatched fingerprint
- Site registry update_last_seen: updates timestamp, errors on unknown site
- Recv rate limiter: check_batch allows below limit; stats track allowed batches; reset clears state
- Recv rate limiter: RateLimitConfig default values, burst factor
- Journal GC: GcPolicy RetainAll never GCs; RetainByAge GCs old entries; RetainByCount GCs excess
- Journal GC: JournalGcState record_ack, min_acked_seq, all_sites_acked
- Journal GC: JournalGcScheduler run_gc returns candidates; stats after GC run
- Combined: TLS policy + site registry — register site with fingerprint, validate connection

Include a `#[cfg(test)] mod tests { }` block at the end with 3-4 additional tests.

### File 2: `crates/claudefs-tests/src/quota_integration.rs`

**Purpose:** Integration tests for quota enforcement in both A5 FUSE layer and A8 management layer.

**~27 tests covering:**
- A5 QuotaEnforcer: basic check_write allowed under limit; denied when hard limit exceeded
- A5 QuotaEnforcer: update_user_quota then check_write reflects new limit
- A5 QuotaEnforcer: update_group_quota; check_create under inode limit
- A5 QuotaEnforcer: TTL cache — cache_hits, check_count, denied_count stats
- A5 QuotaEnforcer: invalidate_user clears cache; with_default_ttl constructor
- A5 QuotaUsage: bytes_status returns Ok/SoftExceeded/HardExceeded correctly
- A5 QuotaUsage: unlimited() has no limits; new() sets both limits
- A8 QuotaRegistry: set_limit and check_quota passes; bytes exceeded returns error
- A8 QuotaRegistry: update_usage and get_usage lifecycle
- A8 QuotaRegistry: over_quota_subjects returns correct entries
- A8 QuotaRegistry: near_quota_subjects at threshold
- A8 QuotaRegistry: bytes_available, files_available calculations
- A8 QuotaRegistry: limit_count, remove_limit
- Combined: A5 quota enforce + A8 quota registry cross-validation (same subject enforced at both layers)

Include a `#[cfg(test)] mod tests { }` block at the end with 3-4 additional tests.

### File 3: `crates/claudefs-tests/src/mgmt_integration.rs`

**Purpose:** Integration tests for A8 management API components.

**~24 tests covering:**
- RBAC: admin_role has all permissions; viewer_role has limited permissions
- RBAC: operator_role has write but not ManageUsers; tenant_admin_role has ManageQuota
- RBAC: Role::add_permission, has_permission, permission_count
- RBAC: RbacRegistry add_role/get_role/remove_role lifecycle
- RBAC: RbacRegistry add_user/get_user; User::new() fields
- RBAC: Permission::implies — Admin implies ReadData; ReadData does not imply WriteData
- SLA: compute_percentiles returns None for empty; correct values for uniform samples
- SLA: SlaTarget fields; SlaMetricKind::name returns non-empty string
- SLA: LatencySample construction
- SLA: PercentileResult fields have valid values for sorted input
- Alerting: AlertManager::with_default_rules creates non-empty rule set
- Alerting: Comparison::evaluate — GreaterThan(100.0).evaluate(150.0) = true
- Alerting: Alert::new, is_firing, is_resolved, age_secs
- Alerting: AlertManager::evaluate with no metrics returns no alerts
- Combined: RBAC check + quota check — viewer cannot ManageQuota

Include a `#[cfg(test)] mod tests { }` block at the end with 3-4 additional tests.

### File 4: `crates/claudefs-tests/src/acl_integration.rs`

**Purpose:** POSIX ACL enforcement and fallocate mode tests.

**~25 tests covering:**
- AclPerms: from_bits/to_bits round-trip for 0..7; all() has all bits; none() has no bits; read_only() has read only
- AclTag: UserObj, User(uid), GroupObj, Group(gid), Mask, Other variants
- PosixAcl: new() creates empty ACL; add_entry adds entries
- PosixAcl check_access: UserObj grants access to file owner
- PosixAcl check_access: Other tag for non-owner, non-group access
- PosixAcl check_access: Group tag for group members
- PosixAcl check_access: Mask restricts effective permissions
- PosixAcl: effective_perms with Mask limits User permissions
- PosixAcl: XATTR constants are non-empty strings
- FallocateOp: from_flags(0) returns Allocate; from_flags(KEEP_SIZE) returns... varies
- FallocateOp: from_flags(PUNCH_HOLE | KEEP_SIZE) returns PunchHole
- FallocateOp: is_space_saving true for PunchHole and ZeroRange; false for Allocate
- FallocateOp: modifies_size false for PunchHole (KEEP_SIZE required), true for Allocate
- FallocateOp: affected_range returns (offset, length)
- FallocateStats: default construction
- Combined: ACL check followed by fallocate validation

Include a `#[cfg(test)] mod tests { }` block at the end with 3-4 additional tests.

**Note on FallocateOp::from_flags:** The exact error type is not known. Use `unwrap_or_else(|_| FallocateOp::Allocate)` for error cases, or just test `is_ok()`/`is_err()` on the result. For `from_flags(FALLOC_FL_PUNCH_HOLE | FALLOC_FL_KEEP_SIZE)` — test with `.is_ok()`. For `from_flags(0)` — test with `.is_ok()`.

### File 5: `crates/claudefs-tests/src/perf_regression.rs`

**Purpose:** Performance regression test framework using the bench infrastructure.

**~21 tests covering:**
- FioConfig construction: default rwmix, block size, jobs, runtime
- FioRwMode variants: Read, Write, RandRead, RandWrite, ReadWrite, RandRW
- FioResult: throughput_mb_per_sec, iops calculations
- FioRunner: detect_fio_binary when fio is absent returns None-like result (test the detection logic)
- Parse FIO JSON: parse_fio_json with minimal valid JSON; with empty JSON returns error
- Regression framework: comparing two FioResult instances for regression
- Performance baseline: verify that FioResult fields are accessible
- Bench report aggregation using report::ReportBuilder
- TestCaseResult with status Passed/Failed/Skipped
- TestSuiteReport with multiple results

Use types from `claudefs_tests::bench::*` (re-exported in lib.rs):
- `FioConfig`, `FioResult`, `FioRunner`, `FioRwMode`, `detect_fio_binary`, `parse_fio_json`

Also use:
- `claudefs_tests::report::{ReportBuilder, TestCaseResult, TestStatus, TestSuiteReport, AggregateReport}`

Include a `#[cfg(test)] mod tests { }` block at the end with 3-4 additional tests.

## Code Style Requirements

1. Use the exact import paths shown above — do NOT rename types unless there's a naming conflict
2. For naming conflicts (e.g., two `QuotaUsage` types), use `as` aliasing: `use claudefs_mgmt::quota::QuotaUsage as MgmtQuotaUsage`
3. Each file starts with a doc comment `//! Brief description`
4. Tests use `#[test]` attribute, no async (all types are sync)
5. Follow the pattern from existing files: plain `#[test]` functions at module level, then `#[cfg(test)] mod tests { use super::*; ... }` at the end
6. Keep each test focused on ONE thing — no omnibus tests
7. Use `assert!`, `assert_eq!`, `assert_ne!`, `matches!` — no `unwrap()` on expected failures
8. For `is_ok()`/`is_err()` use `.is_ok()` directly on the `Result`

## Output Format

For each file, output the complete Rust source code in a fenced code block with the filename as a comment at the top:

```rust
// FILE: crates/claudefs-tests/src/security_integration.rs
// (full file contents here)
```

Output all 5 files completely and sequentially.
