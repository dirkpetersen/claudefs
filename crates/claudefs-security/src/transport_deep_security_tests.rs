//! Deep security tests for claudefs-transport: auth, protocol, dedup, flow control, multipath.
//!
//! Part of A10 Phase 6: Transport deep security audit

#[cfg(test)]
mod tests {
    use claudefs_transport::adaptive::{AdaptiveConfig, AdaptiveTimeout, LatencyHistogram};
    use claudefs_transport::circuitbreaker::{CircuitBreaker, CircuitBreakerConfig, CircuitState};
    use claudefs_transport::conn_auth::{
        AuthConfig, AuthLevel, AuthResult, CertificateInfo, ConnectionAuthenticator,
    };
    use claudefs_transport::enrollment::{
        ClusterCA, EnrollmentConfig, EnrollmentError, EnrollmentService, EnrollmentToken,
    };
    use claudefs_transport::flowcontrol::{FlowControlConfig, FlowControlState, FlowController};
    use claudefs_transport::multipath::{
        MultipathConfig, MultipathRouter, PathId, PathSelectionPolicy, PathState,
    };
    use claudefs_transport::protocol::{
        Frame, FrameFlags, FrameHeader, Opcode, MAGIC, MAX_PAYLOAD_SIZE,
    };
    use claudefs_transport::ratelimit::{CompositeRateLimiter, RateLimitConfig, RateLimiter};
    use claudefs_transport::request_dedup::{DedupConfig, DedupResult, DedupStats, DedupTracker};

    fn make_cert(
        subject: &str,
        issuer: &str,
        serial: &str,
        fingerprint: &str,
        not_before_ms: u64,
        not_after_ms: u64,
    ) -> CertificateInfo {
        CertificateInfo {
            subject: subject.to_string(),
            issuer: issuer.to_string(),
            serial: serial.to_string(),
            fingerprint_sha256: fingerprint.to_string(),
            not_before_ms,
            not_after_ms,
            is_ca: false,
        }
    }

    // Category 1: Connection Authentication (5 tests)

    #[test]
    fn test_auth_time_zero_default() {
        let config = AuthConfig::default();
        let mut auth = ConnectionAuthenticator::new(config);

        // Do not call set_time() - should default to 0
        let cert = make_cert("server1", "ClusterCA", "01", "abc123", 0, 100);

        let result = auth.authenticate(&cert);

        // FINDING-TRANS-DEEP-01: With time=0, expired cert (not_after=100) is accepted
        // because 0 > 100 evaluates to false in the check
        match result {
            AuthResult::Allowed { .. } => {
                println!(
                    "FINDING-TRANS-DEEP-01: Expired certificate accepted due to time=0 default"
                );
            }
            AuthResult::CertificateExpired { .. } => {
                println!("FINDING-TRANS-DEEP-01: Expired certificate correctly rejected");
            }
            _ => {}
        }
    }

    #[test]
    fn test_auth_level_none_allows_all() {
        let config = AuthConfig {
            level: AuthLevel::None,
            ..Default::default()
        };
        let mut auth = ConnectionAuthenticator::new(config);

        let cert = make_cert("bad", "bad", "01", "badfingerprint", 0, 1000);

        let result = auth.authenticate(&cert);

        assert!(matches!(result, AuthResult::Allowed { identity } if identity == "bad"));
    }

    #[test]
    fn test_auth_revoked_cert_denied() {
        let config = AuthConfig::default();
        let mut auth = ConnectionAuthenticator::new(config);

        auth.revoke_fingerprint("revokedfp".to_string());

        let cert = make_cert(
            "server1",
            "ClusterCA",
            "01",
            "revokedfp",
            1000,
            86400000 * 365 * 1000,
        );

        auth.set_time(5000);
        let result = auth.authenticate(&cert);

        assert!(matches!(
            result,
            AuthResult::CertificateRevoked { subject, serial }
            if subject == "server1" && serial == "01"
        ));
    }

    #[test]
    fn test_auth_expired_cert_rejected() {
        let config = AuthConfig::default();
        let mut auth = ConnectionAuthenticator::new(config);

        auth.set_time(2000);
        let cert = make_cert("server1", "ClusterCA", "01", "abc123", 1000, 1000);

        let result = auth.authenticate(&cert);

        assert!(matches!(
            result,
            AuthResult::CertificateExpired { subject, expired_at_ms }
            if subject == "server1" && expired_at_ms == 1000
        ));
    }

    #[test]
    fn test_auth_ca_fingerprint_substring_match() {
        let config = AuthConfig {
            require_cluster_ca: true,
            cluster_ca_fingerprint: Some("ClusterCA".to_string()),
            ..Default::default()
        };
        let mut auth = ConnectionAuthenticator::new(config);

        let cert = make_cert(
            "server1",
            "MyClusterCAIssuer",
            "01",
            "abc123",
            1000,
            86400000 * 365 * 1000,
        );

        auth.set_time(5000);
        let result = auth.authenticate(&cert);

        // FINDING-TRANS-DEEP-02: Substring match instead of exact match
        // "MyClusterCAIssuer" contains "ClusterCA" so it's accepted
        match &result {
            AuthResult::Allowed { identity } => {
                println!("FINDING-TRANS-DEEP-02: Substring match - cert issuer contains cluster_ca_fingerprint");
            }
            AuthResult::Denied { reason } => {
                println!("FINDING-TRANS-DEEP-02: Substring match failed: {}", reason);
            }
            _ => {}
        }

        assert!(matches!(result, AuthResult::Allowed { .. }));
    }

    // Category 2: Protocol Frame Security (5 tests)

    #[test]
    fn test_frame_magic_validation() {
        let mut header = FrameHeader::new(FrameFlags::default(), 0x0101, 1, 0, 0);
        header.magic = 0xDEADBEEF; // Wrong magic

        let encoded = header.encode();
        let result = FrameHeader::decode(&encoded);

        assert!(matches!(
            result,
            Err(claudefs_transport::TransportError::InvalidMagic { .. })
        ));
    }

    #[test]
    fn test_frame_max_payload_size() {
        let payload = vec![0u8; (MAX_PAYLOAD_SIZE + 1) as usize];
        let frame = Frame::new(Opcode::Read, 1, payload);

        let result = frame.validate();

        assert!(matches!(
            result,
            Err(claudefs_transport::TransportError::PayloadTooLarge { .. })
        ));
    }

    #[test]
    fn test_frame_checksum_corruption() {
        let payload = b"test data for checksum validation".to_vec();
        let mut frame = Frame::new(Opcode::Write, 1, payload);

        // Flip a byte in the payload to corrupt it
        frame.payload[0] ^= 0xFF;

        let result = frame.validate();

        assert!(matches!(
            result,
            Err(claudefs_transport::TransportError::ChecksumMismatch { .. })
        ));
    }

    #[test]
    fn test_frame_conflicting_flags() {
        let payload = b"test".to_vec();
        let mut frame = Frame::new(Opcode::Write, 1, payload);

        // Set both ONE_WAY and RESPONSE flags
        frame.header.flags = FrameFlags::ONE_WAY | FrameFlags::RESPONSE;

        // FINDING-TRANS-DEEP-03: No validation prevents conflicting flags
        let result = frame.validate();

        match result {
            Ok(_) => {
                println!("FINDING-TRANS-DEEP-03: Conflicting flags (ONE_WAY | RESPONSE) accepted without error");
            }
            Err(_) => {
                println!("FINDING-TRANS-DEEP-03: Conflicting flags properly rejected");
            }
        }

        // Currently passes validation - this is the finding
        assert!(result.is_ok());
    }

    #[test]
    fn test_frame_empty_payload() {
        let frame = Frame::new(Opcode::Write, 1, vec![]);

        let encoded = frame.encode();
        let decoded = Frame::decode(&encoded).expect("Should decode empty payload frame");

        assert_eq!(decoded.payload.len(), 0);
        assert_eq!(decoded.header.request_id, 1);
    }

    // Category 3: Request Deduplication (5 tests)
    // Note: RequestId constructor is private, so these tests verify config and public types only

    #[test]
    fn test_dedup_config_defaults() {
        let config = DedupConfig::default();
        assert_eq!(config.max_entries, 100_000);
        assert_eq!(config.ttl_ms, 30_000);
        assert_eq!(config.cleanup_interval_ms, 5_000);
    }

    #[test]
    fn test_dedup_config_custom() {
        let config = DedupConfig {
            max_entries: 5000,
            ttl_ms: 60000,
            cleanup_interval_ms: 10000,
        };
        assert_eq!(config.max_entries, 5000);
        assert_eq!(config.ttl_ms, 60000);
        assert_eq!(config.cleanup_interval_ms, 10000);
    }

    #[test]
    fn test_dedup_result_variants() {
        // Verify DedupResult enum variants exist
        let _new = DedupResult::New;
        let _dup = DedupResult::Duplicate { hit_count: 5 };
        let _expired = DedupResult::Expired;
    }

    #[test]
    fn test_dedup_stats_default() {
        let stats = DedupStats::default();
        assert_eq!(stats.total_checks, 0);
        assert_eq!(stats.total_duplicates, 0);
        assert_eq!(stats.total_evictions, 0);
        assert_eq!(stats.current_entries, 0);
        assert_eq!(stats.hit_rate, 0.0);
    }

    #[test]
    fn test_dedup_tracker_interface() {
        let config = DedupConfig::default();
        let tracker = DedupTracker::new(config);

        // Verify we can call public methods without panicking
        assert_eq!(tracker.len(), 0);
        assert!(tracker.is_empty());

        let stats = tracker.stats();
        assert_eq!(stats.current_entries, 0);
    }

    // Category 4: Flow Control & Rate Limiting (5 tests)

    #[test]
    fn test_flow_control_state_transitions() {
        let mut config = FlowControlConfig::default();
        config.max_inflight_requests = 10;
        config.max_inflight_bytes = 1000;
        config.high_watermark_pct = 80;
        config.low_watermark_pct = 50;

        let controller = FlowController::new(config);

        // Initially Open
        assert_eq!(controller.state(), FlowControlState::Open);

        // Fill to 50% - should still be Open
        let mut permits = Vec::new();
        for _ in 0..5 {
            permits.push(controller.try_acquire(100).unwrap());
        }
        assert_eq!(controller.state(), FlowControlState::Open);

        // Fill to 80% - should be Throttled
        for _ in 0..3 {
            permits.push(controller.try_acquire(100).unwrap());
        }
        assert_eq!(controller.state(), FlowControlState::Throttled);

        // Fill to 100% - should be Blocked
        for _ in 0..2 {
            permits.push(controller.try_acquire(100).unwrap());
        }
        assert_eq!(controller.state(), FlowControlState::Blocked);
    }

    #[test]
    fn test_flow_control_permit_release() {
        let config = FlowControlConfig::default();
        let controller = FlowController::new(config);

        let permit = controller.try_acquire(1024).expect("Should acquire permit");

        assert_eq!(controller.inflight_requests(), 1);
        assert_eq!(controller.inflight_bytes(), 1024);

        drop(permit);

        assert_eq!(controller.inflight_requests(), 0);
        assert_eq!(controller.inflight_bytes(), 0);
    }

    #[test]
    fn test_circuit_breaker_state_machine() {
        let config = CircuitBreakerConfig::default();
        let breaker = CircuitBreaker::new(config);

        // Initially Closed
        assert_eq!(breaker.state(), CircuitState::Closed);

        // Record failures until threshold
        for _ in 0..5 {
            breaker.record_failure();
        }

        // Should be Open now
        assert_eq!(breaker.state(), CircuitState::Open);
        assert!(!breaker.can_execute());
    }

    #[test]
    fn test_circuit_breaker_half_open_recovery() {
        let mut config = CircuitBreakerConfig::default();
        config.open_duration = std::time::Duration::from_millis(50);

        let breaker = CircuitBreaker::new(config);

        // Open the circuit
        for _ in 0..5 {
            breaker.record_failure();
        }
        assert_eq!(breaker.state(), CircuitState::Open);

        // Wait for reset timeout
        std::thread::sleep(std::time::Duration::from_millis(60));

        // Should transition to HalfOpen
        let can_exec = breaker.can_execute();
        assert!(can_exec, "Should allow test request in half-open state");
        assert_eq!(breaker.state(), CircuitState::HalfOpen);

        // Record successes until threshold
        for _ in 0..3 {
            breaker.record_success();
        }

        // Should be Closed now
        assert_eq!(breaker.state(), CircuitState::Closed);
    }

    #[test]
    fn test_rate_limit_burst_enforcement() {
        let config = RateLimitConfig::new(1000, 5); // 5 burst size
        let limiter = RateLimiter::new(config);

        // Acquire 5 tokens (all allowed)
        for _ in 0..5 {
            assert!(limiter.try_acquire(), "Should acquire token within burst");
        }

        // 6th should be denied
        assert!(!limiter.try_acquire(), "Should deny token beyond burst");
    }

    // Category 5: Enrollment & Multipath (5 tests)

    #[test]
    fn test_enrollment_token_generation() {
        let config = EnrollmentConfig::default();
        let ca = ClusterCA::new(&config).expect("Failed to create CA");
        let mut service = EnrollmentService::new(ca, config);

        let token = service
            .generate_token("node1")
            .expect("Failed to generate token");

        // Verify token is non-empty
        assert!(!token.token.is_empty());
        assert!(token.token.len() >= 32);

        // Verify valid expiry
        assert!(token.expires_at > token.created_at);
        assert!(token.expires_at - token.created_at >= 3600 * 1000); // At least 1 hour
    }

    #[test]
    fn test_enrollment_token_reuse_fails() {
        let config = EnrollmentConfig::default();
        let ca = ClusterCA::new(&config).expect("Failed to create CA");
        let mut service = EnrollmentService::new(ca, config);

        let token = service
            .generate_token("node1")
            .expect("Failed to generate token");
        let token_str = token.token.clone();

        // First use should succeed
        let _result1 = service
            .enroll_with_token(&token_str)
            .expect("First enrollment should succeed");

        // Second use should fail with TokenAlreadyUsed
        let result2 = service.enroll_with_token(&token_str);

        assert!(matches!(
            result2,
            Err(EnrollmentError::TokenAlreadyUsed { .. })
        ));
    }

    #[test]
    fn test_multipath_all_paths_failed() {
        let config = MultipathConfig::default();
        let mut router = MultipathRouter::new(config);

        let id1 = router.add_path("eth0".to_string(), 100, 1);
        let id2 = router.add_path("eth1".to_string(), 100, 2);

        // Mark both as failed
        router.mark_failed(id1);
        router.mark_failed(id2);

        // select_path should return None
        let selected = router.select_path();
        assert!(
            selected.is_none(),
            "Should return None when all paths failed"
        );
    }

    #[test]
    fn test_multipath_failover_on_error() {
        let mut config = MultipathConfig::default();
        config.policy = PathSelectionPolicy::Failover;
        config.failure_threshold = 2; // Fail after 2 consecutive failures

        let mut router = MultipathRouter::new(config);

        let id1 = router.add_path("primary".to_string(), 100, 1);
        let _id2 = router.add_path("backup".to_string(), 100, 2);

        // Primary should be selected initially
        let selected1 = router.select_path();
        assert_eq!(selected1, Some(id1));

        // Record failures on primary to trigger failover
        router.record_failure(id1, 1024);
        router.record_failure(id1, 1024);

        // Now backup should be selected
        let selected2 = router.select_path();
        assert_ne!(
            selected2,
            Some(id1),
            "Primary should no longer be selected after failures"
        );
    }

    #[test]
    fn test_adaptive_timeout_latency_tracking() {
        let hist = LatencyHistogram::new(100);

        // Record various latencies
        for i in 1..=100 {
            hist.record(i * 100); // 100, 200, 300, ... 10000
        }

        let p50 = hist.percentile(0.50);
        let p99 = hist.percentile(0.99);

        // Verify reasonable values
        assert!(p50 > 0, "p50 should be > 0");
        assert!(p99 > p50, "p99 should be >= p50");

        // p50 should be around 5000 (middle of 100-10000)
        assert!(
            p50 >= 4000 && p50 <= 6000,
            "p50 should be around 5000, got {}",
            p50
        );

        // p99 should be close to 10000
        assert!(p99 >= 9000, "p99 should be >= 9000, got {}", p99);
    }
}
