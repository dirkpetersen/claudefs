# Task: Write transport_deep_security_tests.rs for claudefs-security crate

Write a comprehensive security test module for the `claudefs-transport` crate focusing on authentication, protocol parsing, rate limiting, dedup, flow control, and multipath security.

## File location
`crates/claudefs-security/src/transport_deep_security_tests.rs`

## Module structure
```rust
//! Deep security tests for claudefs-transport: auth, protocol, dedup, flow control, multipath.
//!
//! Part of A10 Phase 6: Transport deep security audit

#[cfg(test)]
mod tests {
    // imports and tests here
}
```

## Available types (verified from lib.rs pub use statements)

```rust
use claudefs_transport::{
    // Protocol
    Frame, FrameHeader, Opcode, FrameFlags,
    // Rate limiting
    RateLimitConfig, // plus whatever RateLimiter, CompositeRateLimiter types exist
    // Request dedup
    DedupConfig, DedupEntry, DedupResult, DedupStats, DedupTracker, RequestId,
    // Flow control
    FlowControlConfig, FlowControlState, FlowController,
    // Error
    TransportError,
    // Server
    ServerConfig, ServerStats,
    // Circuit breaker
    CircuitBreaker, CircuitBreakerConfig, CircuitState,
    // Adaptive timeout
    AdaptiveTimeoutConfig, LatencyHistogram,
    // Multipath
    // (may need module path: claudefs_transport::multipath::*)
};
// Module-path imports that may be needed:
use claudefs_transport::conn_auth::{AuthLevel, AuthConfig, ConnectionAuthenticator, AuthResult, CertificateInfo};
use claudefs_transport::multipath::{MultipathRouter, PathId, PathState, PathMetrics, PathSelectionPolicy};
use claudefs_transport::ratelimit::{RateLimiter, CompositeRateLimiter, RateLimitResult};
use claudefs_transport::enrollment::{EnrollmentConfig, EnrollmentToken, EnrollmentService, ClusterCA};
use claudefs_transport::protocol::{MAGIC, PROTOCOL_VERSION, FRAME_HEADER_SIZE, MAX_PAYLOAD_SIZE};
```

**IMPORTANT**: Not all may be public. If any import fails, remove it and skip those tests.

## Test categories (25 tests total, 5 per category)

### Category 1: Connection Authentication (5 tests)

1. **test_auth_time_zero_default** — Create ConnectionAuthenticator without calling set_time(). Verify time defaults to 0. Create a CertificateInfo with not_after in the past (say 100). Authenticate. Document if expired cert accepted due to time=0 (FINDING).

2. **test_auth_level_none_allows_all** — Create AuthConfig with AuthLevel::None. Authenticate any CertificateInfo. Verify AuthResult::Allowed (no cert checks at all).

3. **test_auth_revoked_cert_denied** — Create ConnectionAuthenticator. Add a certificate fingerprint to revocation list. Try to authenticate a CertificateInfo with that fingerprint. Verify AuthResult::CertificateRevoked.

4. **test_auth_expired_cert_rejected** — Create ConnectionAuthenticator, call set_time(2000). Create CertificateInfo with not_after=1000. Authenticate. Verify AuthResult::CertificateExpired.

5. **test_auth_ca_fingerprint_substring_match** — Create AuthConfig requiring cluster CA with fingerprint "CA". Create CertificateInfo with issuer containing "MyCertificationAuthority". Verify whether it's accepted due to substring match (FINDING: substring match instead of exact).

### Category 2: Protocol Frame Security (5 tests)

6. **test_frame_magic_validation** — Create a frame header with wrong magic (0xDEADBEEF). Try to decode. Verify error.

7. **test_frame_max_payload_size** — Create a FrameHeader with payload_length = MAX_PAYLOAD_SIZE + 1. Validate. Verify it's rejected.

8. **test_frame_checksum_corruption** — Create a valid Frame, encode it, flip a byte in payload, try to validate. Verify checksum mismatch detected.

9. **test_frame_conflicting_flags** — Create a Frame with both ONE_WAY and RESPONSE flags set. Document whether this is allowed (FINDING if accepted without error).

10. **test_frame_empty_payload** — Create a Frame with empty payload. Verify encode/decode roundtrip works.

### Category 3: Request Deduplication (5 tests)

11. **test_dedup_duplicate_detection** — Create DedupTracker. Check request_id 1 (should be New). Record it. Check again (should be Duplicate).

12. **test_dedup_expired_entry** — Create DedupTracker with ttl_ms=100. Record request at time 0. Advance time to 200. Check same request. Verify Expired.

13. **test_dedup_max_entries_eviction** — Create DedupTracker with max_entries=3. Record 4 requests. Verify oldest is evicted.

14. **test_dedup_hit_count_increments** — Check same request_id multiple times after recording. Verify hit_count increases.

15. **test_dedup_stats_tracking** — Record several requests, check duplicates. Verify stats reflect total checks, hits, misses.

### Category 4: Flow Control & Rate Limiting (5 tests)

16. **test_flow_control_state_transitions** — Create FlowController. Consume capacity until state transitions from Open to Throttled to Blocked.

17. **test_flow_control_permit_release** — Acquire a permit, verify capacity decremented. Release permit (drop), verify capacity restored.

18. **test_circuit_breaker_state_machine** — Create CircuitBreaker in Closed state. Record failures until it opens. Verify state is Open.

19. **test_circuit_breaker_half_open_recovery** — Open circuit breaker, wait for reset timeout, verify HalfOpen. Record success, verify returns to Closed.

20. **test_rate_limit_burst_enforcement** — Create RateLimiter with burst_size=5. Acquire 5 tokens (all allowed). Acquire 6th (denied).

### Category 5: Enrollment & Multipath (5 tests)

21. **test_enrollment_token_generation** — Generate enrollment token. Verify token is non-empty, has valid expiry.

22. **test_enrollment_token_reuse_fails** — Generate token, enroll with it. Try to enroll again with same token. Verify failure (token already used).

23. **test_multipath_all_paths_failed** — Create MultipathRouter with 2 paths. Mark both as Failed. Try select_path. Verify None returned.

24. **test_multipath_failover_on_error** — Create MultipathRouter with Failover policy. Record multiple failures on primary. Verify secondary becomes selected.

25. **test_adaptive_timeout_latency_tracking** — Create LatencyHistogram, record several latencies. Verify percentile(0.99) returns reasonable value.

## Implementation notes
- Use `fn make_xxx()` helper functions
- Mark security findings with `// FINDING-TRANS-DEEP-XX: description`
- If a type is not public, skip that test and add an alternative
- Each test should focus on one property
- Use `assert!`, `assert_eq!`, `matches!`

## Output format
Output ONLY the complete Rust source file. No explanations, no markdown fences.
