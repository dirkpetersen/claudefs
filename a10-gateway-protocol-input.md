# Task: Write gateway_protocol_security_tests.rs for claudefs-security crate

Write a comprehensive security test module for the `claudefs-gateway` crate focusing on NFS session management, NFS ACL enforcement, S3 encryption, S3 object lock, and S3 versioning security.

## File location
`crates/claudefs-security/src/gateway_protocol_security_tests.rs`

## Module structure
```rust
//! Protocol security tests for claudefs-gateway: NFS sessions, ACL, S3 encryption, object lock, versioning.
//!
//! Part of A10 Phase 8: Gateway protocol security audit

#[cfg(test)]
mod tests {
    // imports and tests here
}
```

## Available types (verified from lib.rs and module exploration)

```rust
// NFS V4 Session
use claudefs_gateway::nfs_v4_session::{
    ClientId, NfsClient, NfsSession, SessionError, SessionId, SessionManager, SessionState, Slot, SlotResult,
};
// NFS ACL
use claudefs_gateway::nfs_acl::{
    AclEntry, AclPerms, AclTag, Nfs4Ace, Nfs4AceFlags, Nfs4AceType, Nfs4AccessMask, PosixAcl,
};
// NFS Write
use claudefs_gateway::nfs_write::{PendingWrite, WriteStability, WriteTracker};
// S3 Encryption
use claudefs_gateway::s3_encryption::{SseAlgorithm, SseContext, SseError};
// S3 Object Lock
use claudefs_gateway::s3_object_lock::{
    BucketObjectLockConfig, DefaultRetention, LegalHoldStatus, ObjectLockInfo, ObjectLockStatus,
    ObjectRetention, RetentionMode, RetentionPeriod,
};
// S3 CORS
use claudefs_gateway::s3_cors::{CorsConfig, CorsRule, PreflightRequest, PreflightResponse};
// S3 Versioning
use claudefs_gateway::s3_versioning::{VersionEntry, VersionId, VersionType, VersioningState};
// S3 Lifecycle
use claudefs_gateway::s3_lifecycle::{LifecycleFilter, StorageClass};
```

**IMPORTANT**: Not all may be public. If any import fails, remove it and skip those tests.

## Existing tests to AVOID duplicating

The existing gateway tests cover:
- `gateway_security_tests.rs`: S3 bucket name validation, object key traversal, null bytes, max length, ListObjects edge cases
- `gateway_s3_security_tests.rs`: Presigned URLs, bucket policies, rate limiting, token auth, multipart uploads
- `gateway_auth_tests.rs`: Token generation, AUTH_SYS UID spoofing, token expiry, max GIDs, root squashing

DO NOT duplicate any of these. Focus on NFS protocol, S3 encryption/object lock/versioning, and ACL enforcement.

## Test categories (25 tests total, 5 per category)

### Category 1: NFS V4 Session Security (5 tests)

1. **test_nfs_session_id_uniqueness** — Create SessionManager. Create 100 sessions. Verify all SessionIds are unique.

2. **test_nfs_slot_sequence_replay_detection** — Create NfsSession. Get fore_slot_mut(). Call validate_sequence with seq=1 → NewRequest. Call validate_sequence with seq=1 again → Replay. Verify replay is detected.

3. **test_nfs_slot_invalid_sequence** — Create NfsSession. Get fore_slot_mut(). Validate sequence 1. Then try sequence 5 (skipping 2,3,4). Verify InvalidSequence returned.

4. **test_nfs_session_expire_stale** — Create SessionManager. Create client, create session. Set lease_expiry in the past. Call expire_stale_clients(). Verify client is expired/removed.

5. **test_nfs_session_unconfirmed_client** — Create SessionManager. Create client but don't confirm. Try create_session for unconfirmed client. Verify SessionError::ClientNotConfirmed.

### Category 2: NFS ACL Enforcement (5 tests)

6. **test_acl_missing_required_entries** — Create PosixAcl with only a User entry (no user_obj, group_obj, other). Call is_valid(). Verify returns false.

7. **test_acl_mask_limits_named_entries** — Create valid PosixAcl with user_obj(rwx), named user(rwx), mask(r-x), group_obj(---), other(---). Call check_access for the named user. Verify write is denied by mask even though named entry has write.

8. **test_acl_root_bypass** — Create restrictive PosixAcl (other=none). Call check_access(uid=0, gid=0). Document whether root UID=0 bypasses ACL. (FINDING: if root bypasses ACL, this is a policy concern).

9. **test_nfs4_ace_deny_overrides_allow** — Create Nfs4Ace deny for everyone, then allow for everyone. Document evaluation order behavior (FINDING: if allow evaluated before deny, security bypass).

10. **test_acl_permissions_from_bits_roundtrip** — Create AclPerms::rwx(). Call to_bits(). Call from_bits(). Verify equal to original.

### Category 3: S3 Encryption & KMS (5 tests)

11. **test_sse_none_algorithm** — Create SseContext with SseAlgorithm::None. Verify is_kms() returns false. Document that no encryption is applied.

12. **test_sse_kms_requires_key_id** — Create SseContext with SseAlgorithm::AwsKms but no key_id. Document whether this is rejected or allowed without a key. (FINDING: KMS encryption without key_id means service must guess).

13. **test_sse_context_injection** — Create SseContext. Add encryption_context with key containing special characters ("key=value&evil=true"). Document whether context is validated.

14. **test_sse_algorithm_is_kms** — Verify is_kms() returns true for AwsKms and AwsKmsDsse, false for None and AesCbc256.

15. **test_sse_bucket_key_enabled** — Create SseContext with AwsKms and bucket_key_enabled=true. Verify fields are set correctly. Document the security implications.

### Category 4: S3 Object Lock & Compliance (5 tests)

16. **test_object_lock_governance_vs_compliance** — Create ObjectRetention with Governance mode and future retain_until. Verify has_active_retention() returns true. Document that Governance mode allows bypass with special permission.

17. **test_object_lock_expired_retention** — Create ObjectRetention with retain_until in the past. Verify is_expired() returns true and has_active_retention() returns false.

18. **test_legal_hold_overrides_retention** — Create ObjectLockInfo with expired retention but legal_hold=On. Verify the object is still protected (legal hold active even though retention expired).

19. **test_retention_period_days_to_duration** — Create RetentionPeriod::Days(365). Call to_duration(). Verify it equals approximately 365 days in seconds.

20. **test_object_lock_disabled_bucket** — Create BucketObjectLockConfig with status=Disabled. Try to set retention on an object. Document whether enforcement is skipped.

### Category 5: S3 Versioning & CORS (5 tests)

21. **test_version_id_generation_uniqueness** — Generate 1000 VersionIds with same timestamp but different random suffixes. Verify all are unique.

22. **test_version_id_null_special** — Create VersionId::null(). Verify is_null() returns true. Create regular VersionId. Verify is_null() returns false.

23. **test_cors_wildcard_origin** — Create CorsRule::allow_all(). Verify matches_origin("evil.example.com") returns true. (FINDING: wildcard CORS allows any origin — credential theft risk).

24. **test_cors_no_matching_rule** — Create CorsConfig with rule for "https://example.com" only. Try matching_rule for "https://evil.com". Verify returns None.

25. **test_cors_rule_valid_requires_origin_and_method** — Create CorsRule with empty allowed_origins or empty allowed_methods. Call is_valid(). Verify returns false.

## Implementation notes
- Use `fn make_xxx()` helper functions for creating test objects
- Mark security findings with `// FINDING-GW-PROTO-XX: description`
- If a type is not public, skip that test and add an alternative
- Each test focuses on one property
- Use `assert!`, `assert_eq!`, `matches!`
- DO NOT use any async code — all tests are synchronous
- For NFS session creation, use SessionManager::new() and then create_client/create_session
- For time-based tests, use std::time::SystemTime or Duration

## Output format
Output ONLY the complete Rust source file. No explanations, no markdown fences.
