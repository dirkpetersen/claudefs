# Task: Write gateway_infra_security_tests.rs for claudefs-security crate

Write a comprehensive security test module for the `claudefs-gateway` crate focusing on TLS configuration validation, circuit breaker state machine, S3 lifecycle policy, connection pool management, and gateway quota enforcement.

## File location
`crates/claudefs-security/src/gateway_infra_security_tests.rs`

## Module structure
```rust
//! Gateway infrastructure security tests: TLS, circuit breaker, lifecycle, conn pool, quota.
//!
//! Part of A10 Phase 11: Gateway infrastructure security audit

#[cfg(test)]
mod tests {
    // imports and tests here
}
```

## Available types (verified from source)

```rust
use claudefs_gateway::gateway_tls::{
    TlsVersion, CipherPreference, CertSource, ClientCertMode, AlpnProtocol,
    TlsConfig, TlsConfigError, TlsConfigValidator, TlsEndpoint, TlsRegistry,
};
use claudefs_gateway::gateway_circuit_breaker::{
    CircuitState, CircuitBreakerError, CircuitBreakerConfig, CircuitBreakerMetrics,
    CircuitBreaker, CircuitBreakerRegistry,
};
use claudefs_gateway::s3_lifecycle::{
    StorageClass, RuleStatus, LifecycleError, LifecycleFilter, TransitionAction,
    ExpirationAction, LifecycleRule, LifecycleConfiguration, LifecycleRegistry,
};
use claudefs_gateway::gateway_conn_pool::{
    ConnState, ConnPoolError, BackendNode, PooledConn, ConnPoolConfig,
    NodeConnPool, GatewayConnPool,
};
use claudefs_gateway::quota::{
    QuotaSubject, QuotaViolation, QuotaLimits, QuotaUsage, QuotaManager,
};
```

**IMPORTANT**: Not all may be public. If any import fails, remove it and skip those tests.

## Existing tests to AVOID duplicating
- `gateway_auth_tests.rs`: token authentication, AUTH_SYS credentials
- `gateway_security_tests.rs`: S3 validation, pNFS layouts, NFS auth squashing
- `gateway_s3_security_tests.rs`: S3 bucket policy, presigned URLs, rate limiting, encryption, multipart
- `gateway_protocol_security_tests.rs`: NFS v4 sessions, ACLs, S3 encryption, object lock, versioning, CORS

DO NOT duplicate these. Focus on TLS config, circuit breaker, S3 lifecycle, connection pool, quota.

## Test categories (25 tests total, 5 per category)

### Category 1: TLS Configuration Security (5 tests)

1. **test_tls_config_defaults_are_modern** — Create TlsConfig::default(). Verify min_version is Tls13. Verify cipher_pref is Modern. Verify client_cert_mode is None. Verify session_cache_size > 0. Verify handshake_timeout_ms > 0. (FINDING: verify defaults are secure by default).

2. **test_tls_validate_empty_cert_path** — Create TlsConfig with CertSource::PemFiles with empty cert_path. Call TlsConfigValidator::validate(). Verify TlsConfigError::EmptyCertPath returned.

3. **test_tls_validate_empty_key_path** — Create TlsConfig with CertSource::PemFiles with valid cert_path but empty key_path. Call TlsConfigValidator::validate(). Verify TlsConfigError::EmptyKeyPath returned.

4. **test_tls_endpoint_bind_address** — Create TlsEndpoint::new("0.0.0.0", 443, config). Verify bind_address() returns "0.0.0.0:443". Disable endpoint. Verify enabled == false. Enable. Verify enabled == true.

5. **test_tls_registry_management** — Create TlsRegistry. Register 3 endpoints with different names. Verify enabled_count() == 3. Remove one. Verify enabled_count() == 2. Verify all_names() returns 2 names.

### Category 2: Circuit Breaker Security (5 tests)

6. **test_circuit_breaker_initial_closed** — Create CircuitBreaker::new() with default config. Verify state() == CircuitState::Closed. Verify failure_count() == 0 and success_count() == 0.

7. **test_circuit_breaker_opens_on_failures** — Create breaker with failure_threshold=3. Record 2 failures. Verify state still Closed. Record 1 more failure. Verify state == Open. (FINDING: circuit opens at exactly threshold).

8. **test_circuit_breaker_half_open_recovery** — Create breaker. Trip it (force Open). Record successes equal to success_threshold. Verify state returns to Closed. Verify metrics show state_changes.

9. **test_circuit_breaker_call_rejected_when_open** — Create breaker. Trip it open. Call call() with a function. Verify CircuitBreakerError::CircuitOpen returned. Verify rejected_calls metric incremented.

10. **test_circuit_breaker_registry_reset_all** — Create registry. Add 3 breakers. Trip all of them. Call reset_all(). Verify all 3 are back to Closed state.

### Category 3: S3 Lifecycle Policy Security (5 tests)

11. **test_lifecycle_rule_validation** — Create LifecycleConfiguration. Try add_rule with no transitions or expiration. Verify LifecycleError::NoActions. Try add_rule with days=0 transition. Verify LifecycleError::InvalidDays.

12. **test_lifecycle_duplicate_rule_id** — Create config. Add rule with id="rule1". Try add another rule with same id="rule1". Verify LifecycleError::DuplicateRuleId.

13. **test_lifecycle_max_rules** — Create config with 1000 rules (max allowed). Try to add 1001st rule. Verify LifecycleError::TooManyRules. (FINDING: verify hard limit on rule count prevents DoS).

14. **test_lifecycle_filter_matching** — Create LifecycleFilter::with_prefix("data/"). Verify matches("data/file.txt", 100, &[]) returns true. Verify matches("other/file.txt", 100, &[]) returns false. Create filter with size constraints. Test boundary matching.

15. **test_lifecycle_expiration_evaluation** — Create config. Add rule with ExpirationAction::new(30). Test is_object_expired with days_old=29 (false). Test with days_old=30 (true). Test with days_old=31 (true). (FINDING: verify boundary handling at exactly expiration day).

### Category 4: Connection Pool Security (5 tests)

16. **test_conn_pool_config_defaults** — Create ConnPoolConfig::default(). Verify min_per_node >= 1. Verify max_per_node > min_per_node. Verify max_idle_ms > 0. Verify connect_timeout_ms > 0.

17. **test_conn_pool_checkout_checkin** — Create GatewayConnPool with 2 backend nodes. Checkout. Verify returns (node_id, conn_id). Checkin. Checkout again. Verify round-robin behavior (different node).

18. **test_conn_pool_exhaustion** — Create NodeConnPool with max_per_node=2. Checkout 2 connections. Try checkout 3rd. Verify returns None (pool exhausted). Checkin one. Checkout again. Verify succeeds. (FINDING: pool exhaustion handled gracefully).

19. **test_conn_pool_unhealthy_marking** — Create pool with backend node. Checkout connection. Mark unhealthy. Verify the connection is_healthy() returns false. Verify healthy_count() decreased.

20. **test_conn_pool_node_removal** — Create GatewayConnPool with 3 nodes. Verify node_count() == 3. Remove one node. Verify node_count() == 2. Checkout should only go to remaining 2 nodes.

### Category 5: Gateway Quota Enforcement (5 tests)

21. **test_quota_write_hard_limit** — Create QuotaManager. Set limits for User(1000) with bytes_hard=1000. Record 500 byte write. Verify QuotaViolation::None. Record 600 byte write (total=1100 > 1000). Verify QuotaViolation::HardLimitExceeded. Verify usage not updated on hard limit exceeded.

22. **test_quota_soft_limit_warning** — Create QuotaManager. Set limits with bytes_soft=500, bytes_hard=1000. Record 600 byte write (exceeds soft but not hard). Verify QuotaViolation::SoftLimitExceeded. Verify write WAS recorded (soft limit is warning only).

23. **test_quota_inode_enforcement** — Create QuotaManager. Set limits with inodes_hard=5. Call record_create() 5 times. Verify all return None violation. Call record_create() 6th time. Verify HardLimitExceeded.

24. **test_quota_delete_reclaims** — Create QuotaManager. Set limits. Record 500 byte write. Verify usage bytes_used == 500. Record delete of 200 bytes. Verify usage bytes_used == 300.

25. **test_quota_check_without_recording** — Create QuotaManager. Set limits with bytes_hard=100. Record 50 byte write. Call check_write(60) (would exceed). Verify HardLimitExceeded. Verify usage still at 50 (check didn't record).

## Implementation notes
- Use `fn make_xxx()` helper functions for creating test objects
- Mark findings with `// FINDING-GW-INFRA-XX: description`
- If a type is not public, skip that test and add an alternative
- Each test focuses on one property
- Use `assert!`, `assert_eq!`, `matches!`
- DO NOT use any async code — all tests are synchronous
- For BackendNode: `BackendNode::new("node1", "10.0.0.1:9000")`
- For CircuitBreaker: `CircuitBreaker::new("test".to_string(), config)`
- For QuotaManager: `QuotaManager::new()` then `.set_limits()` and `.record_write()`
- For ConnPoolConfig: `ConnPoolConfig::default()` or `ConnPoolConfig::new(min, max, idle_ms)`
- For LifecycleRule: `LifecycleRule::new("rule-1")` then add transitions/expiration

## Output format
Output ONLY the complete Rust source file. No explanations, no markdown fences.
