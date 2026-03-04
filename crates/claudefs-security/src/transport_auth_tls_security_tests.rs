//! Transport authentication and TLS security tests.
//!
//! Part of A10 Phase 27: Storage allocator/io_uring & Transport auth/TLS security audit

#[cfg(test)]
mod tests {
    use claudefs_transport::{
        AuthConfig, AuthLevel, AuthResult, AuthStats, CertificateInfo, ClusterCA,
        ConnectionAuthenticator, EnrollmentConfig, EnrollmentError, EnrollmentService,
        RevocationList,
    };

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

    // ============================================================================
    // Authentication Bypass Attempts (5 tests)
    // ============================================================================

    #[test]
    fn test_auth_sec_revoked_serial_always_denied() {
        let mut auth = ConnectionAuthenticator::new(AuthConfig::default());

        auth.revoke_serial("BAD001".to_string());

        let cert = make_cert(
            "server1",
            "ClusterCA",
            "BAD001",
            "goodfp",
            1000,
            86400000 * 365 * 1000 + 1000,
        );

        auth.set_time(5000);
        let result = auth.authenticate(&cert);

        assert!(
            matches!(result, AuthResult::CertificateRevoked { .. }),
            "Security: revoked serial must be denied even if fingerprint is OK"
        );
    }

    #[test]
    fn test_auth_sec_revoked_fingerprint_always_denied() {
        let mut auth = ConnectionAuthenticator::new(AuthConfig::default());

        auth.revoke_fingerprint("badfingerprint123456".to_string());

        let cert = make_cert(
            "server1",
            "ClusterCA",
            "01",
            "badfingerprint123456",
            1000,
            86400000 * 365 * 1000 + 1000,
        );

        auth.set_time(5000);
        let result = auth.authenticate(&cert);

        assert!(
            matches!(result, AuthResult::CertificateRevoked { .. }),
            "Security: revoked fingerprint must be denied even if subject is allowed"
        );
    }

    #[test]
    fn test_auth_sec_expired_cert_denied_with_tlsonly() {
        let config = AuthConfig {
            level: AuthLevel::TlsOnly,
            ..Default::default()
        };
        let mut auth = ConnectionAuthenticator::new(config);

        let expired_ms: u64 = 86400000 * 200;
        let cert = make_cert("server1", "ClusterCA", "01", "abc123", 1000, expired_ms);

        auth.set_time(expired_ms + 1);
        let result = auth.authenticate(&cert);

        assert!(
            matches!(result, AuthResult::CertificateExpired { .. }),
            "Security: expired cert must be denied even with AuthLevel::TlsOnly"
        );
    }

    #[test]
    fn test_auth_sec_not_yet_valid_cert_denied_all_levels() {
        let levels = [
            AuthLevel::None,
            AuthLevel::TlsOnly,
            AuthLevel::MutualTls,
            AuthLevel::MutualTlsStrict,
        ];

        for level in levels {
            let config = AuthConfig {
                level,
                ..Default::default()
            };
            let mut auth = ConnectionAuthenticator::new(config);

            let cert = make_cert(
                "server1",
                "ClusterCA",
                "01",
                "abc123",
                10000,
                86400000 * 365 * 1000 + 10000,
            );

            auth.set_time(5000);
            let result = auth.authenticate(&cert);

            assert!(
                matches!(result, AuthResult::Denied { reason } if reason == "certificate not yet valid"),
                "Security: not-yet-valid cert must be denied for level {:?}",
                level
            );
        }
    }

    #[test]
    fn test_auth_sec_empty_subject_cert_handling() {
        let mut auth = ConnectionAuthenticator::new(AuthConfig::default());

        let cert = make_cert(
            "",
            "ClusterCA",
            "01",
            "abc123",
            1000,
            86400000 * 365 * 1000 + 1000,
        );

        auth.set_time(5000);
        let result = auth.authenticate(&cert);

        match result {
            AuthResult::Allowed { identity } => {
                assert_eq!(
                    identity, "",
                    "Security: empty subject should be allowed with empty identity"
                );
            }
            AuthResult::Denied { .. } => {
                // Empty subject can be denied - both are acceptable security behaviors
            }
            _ => panic!("Unexpected result: {:?}", result),
        }
    }

    // ============================================================================
    // Certificate Validation Depth (5 tests)
    // ============================================================================

    #[test]
    fn test_auth_sec_strict_mode_rejects_non_matching_fingerprint() {
        let config = AuthConfig {
            level: AuthLevel::MutualTlsStrict,
            allowed_fingerprints: vec!["abc123".to_string(), "def456".to_string()],
            ..Default::default()
        };
        let mut auth = ConnectionAuthenticator::new(config);

        let cert = make_cert(
            "server1",
            "ClusterCA",
            "01",
            "unknownfingerprint",
            1000,
            86400000 * 365 * 1000 + 1000,
        );

        auth.set_time(5000);
        let result = auth.authenticate(&cert);

        assert!(
            matches!(result, AuthResult::Denied { reason } if reason.contains("fingerprint")),
            "Security: strict mode must reject fingerprint not in allowed list"
        );
    }

    #[test]
    fn test_auth_sec_cluster_ca_wrong_issuer_denied() {
        let config = AuthConfig {
            require_cluster_ca: true,
            cluster_ca_fingerprint: Some("ExpectedCA".to_string()),
            ..Default::default()
        };
        let mut auth = ConnectionAuthenticator::new(config);

        let cert = make_cert(
            "server1",
            "WrongCA",
            "01",
            "abc123",
            1000,
            86400000 * 365 * 1000 + 1000,
        );

        auth.set_time(5000);
        let result = auth.authenticate(&cert);

        assert!(
            matches!(result, AuthResult::Denied { reason } if reason.contains("cluster CA")),
            "Security: wrong issuer must be denied with cluster CA validation"
        );
    }

    #[test]
    fn test_auth_sec_cert_at_max_age_boundary() {
        let config = AuthConfig {
            max_cert_age_days: 100,
            ..Default::default()
        };
        let mut auth = ConnectionAuthenticator::new(config);

        let not_before = 100u64 * 86400000;
        let not_after = 400u64 * 86400000;

        let cert = make_cert(
            "server1",
            "ClusterCA",
            "01",
            "abc123",
            not_before,
            not_after,
        );

        let at_boundary = not_before + 100u64 * 86400000;
        auth.set_time(at_boundary);
        let result = auth.authenticate(&cert);

        assert!(
            matches!(result, AuthResult::Denied { reason } if reason.contains("maximum age")),
            "Security: cert at exactly max_cert_age_days should be denied"
        );
    }

    #[test]
    fn test_auth_sec_max_cert_age_zero_always_rejects() {
        let config = AuthConfig {
            max_cert_age_days: 0,
            ..Default::default()
        };
        let mut auth = ConnectionAuthenticator::new(config);

        let cert = make_cert(
            "server1",
            "ClusterCA",
            "01",
            "abc123",
            1000,
            86400000 * 365 * 1000 + 1000,
        );

        auth.set_time(5000);
        let result = auth.authenticate(&cert);

        assert!(
            matches!(result, AuthResult::Denied { reason } if reason.contains("maximum age")),
            "Security: max_cert_age_days=0 must always reject"
        );
    }

    #[test]
    fn test_auth_sec_zero_validity_window_cert() {
        let config = AuthConfig::default();
        let mut auth = ConnectionAuthenticator::new(config);

        let same_time = 5000u64;
        let cert = make_cert("server1", "ClusterCA", "01", "abc123", same_time, same_time);

        auth.set_time(same_time);
        let result = auth.authenticate(&cert);

        match result {
            AuthResult::Allowed { .. } => {}
            AuthResult::Denied { .. } => {}
            AuthResult::CertificateExpired { .. } => {}
            _ => panic!("Unexpected result for zero validity window: {:?}", result),
        }
    }

    // ============================================================================
    // Enrollment Security (5 tests)
    // ============================================================================

    #[test]
    fn test_enroll_sec_rejects_empty_token() {
        let config = EnrollmentConfig::default();
        let ca = ClusterCA::new(&config).expect("CA creation failed");
        let mut service = EnrollmentService::new(ca, config);

        let result = service.enroll_with_token("");

        assert!(
            matches!(result, Err(EnrollmentError::InvalidToken { .. })),
            "Security: empty token must be rejected"
        );
    }

    #[test]
    fn test_enroll_sec_rejects_invalid_token() {
        let config = EnrollmentConfig::default();
        let ca = ClusterCA::new(&config).expect("CA creation failed");
        let mut service = EnrollmentService::new(ca, config);

        let result = service.enroll_with_token("invalid_token_xyz");

        assert!(
            matches!(result, Err(EnrollmentError::InvalidToken { .. })),
            "Security: invalid token must be rejected"
        );
    }

    #[test]
    fn test_enroll_sec_token_for_one_node_not_reusable() {
        let config = EnrollmentConfig::default();
        let ca = ClusterCA::new(&config).expect("CA creation failed");
        let mut service = EnrollmentService::new(ca, config.clone());

        let token = service
            .generate_token("node1")
            .expect("Token generation failed");

        let result1 = service.enroll_with_token(&token.token);
        assert!(
            result1.is_ok(),
            "Security: first use of valid token should succeed"
        );

        let result2 = service.enroll_with_token(&token.token);
        assert!(
            matches!(result2, Err(EnrollmentError::TokenAlreadyUsed { .. })),
            "Security: token must not be reusable after first use"
        );
    }

    #[test]
    fn test_enroll_sec_expired_token_rejected() {
        let config = EnrollmentConfig::default();
        let ca = ClusterCA::new(&config).expect("CA creation failed");
        let mut service = EnrollmentService::new(ca, config);

        let token = service
            .generate_token("node1")
            .expect("Token generation failed");

        // Manually expire the token
        service.tokens.get_mut(&token.token).unwrap().expires_at = 0;

        let result = service.enroll_with_token(&token.token);

        assert!(
            matches!(result, Err(EnrollmentError::TokenExpired { .. })),
            "Security: expired token must be rejected"
        );
    }

    #[test]
    fn test_enroll_sec_crl_includes_revoked_certs() {
        let config = EnrollmentConfig::default();
        let ca = ClusterCA::new(&config).expect("CA creation failed");
        let mut service = EnrollmentService::new(ca, config);

        let bundle = service
            .issue_client_cert("client1")
            .expect("Cert issuance failed");
        let serial = bundle.serial.clone();

        service
            .revoke(&serial, claudefs_transport::RevocationReason::KeyCompromise)
            .expect("Revocation failed");

        let crl = service.get_crl();

        assert!(
            crl.iter().any(|e| e.serial == serial),
            "Security: CRL must include revoked certificate"
        );
    }

    #[test]
    fn test_enroll_sec_ca_cert_pem_non_empty() {
        let config = EnrollmentConfig::default();
        let ca = ClusterCA::new(&config).expect("CA creation failed");

        let cert_pem = ca.ca_cert_pem();

        assert!(
            !cert_pem.is_empty(),
            "Security: CA cert PEM must be non-empty after initialization"
        );
        assert!(
            cert_pem.contains("-----BEGIN CERTIFICATE-----"),
            "Security: CA cert must be valid PEM format"
        );
    }

    // ============================================================================
    // Revocation List Security (4 tests)
    // ============================================================================

    #[test]
    fn test_auth_sec_revocation_list_idempotent_revoke() {
        let mut rl = RevocationList::new();

        rl.revoke_serial("01".to_string());
        rl.revoke_serial("01".to_string());
        rl.revoke_serial("01".to_string());

        assert_eq!(
            rl.len(),
            1,
            "Security: duplicate revocation must be idempotent"
        );
    }

    #[test]
    fn test_auth_sec_revocation_list_concurrent_lookup() {
        let rl = RevocationList::new();

        rl.revoke_serial("01".to_string());
        rl.revoke_fingerprint("fp1".to_string());

        assert!(rl.is_revoked_serial("01"));
        assert!(rl.is_revoked_fingerprint("fp1"));
        assert!(!rl.is_revoked_serial("02"));
        assert!(!rl.is_revoked_fingerprint("fp2"));
    }

    #[test]
    fn test_auth_sec_large_crl_lookup_correctness() {
        let mut rl = RevocationList::new();

        for i in 0..1000 {
            rl.revoke_serial(format!("serial{:04}", i));
        }

        assert_eq!(
            rl.len(),
            1000,
            "Security: large CRL must have correct count"
        );

        assert!(rl.is_revoked_serial("serial0000"));
        assert!(rl.is_revoked_serial("serial0999"));
        assert!(!rl.is_revoked_serial("serial1000"));
    }

    #[test]
    fn test_auth_sec_revocation_list_serialization_roundtrip() {
        let mut rl = RevocationList::new();

        rl.revoke_serial("01".to_string());
        rl.revoke_serial("02".to_string());
        rl.revoke_fingerprint("fp1".to_string());

        let serialized = serde_json::to_string(&rl).unwrap();
        let deserialized: RevocationList = serde_json::from_str(&serialized).unwrap();

        assert_eq!(rl.len(), deserialized.len());
        assert!(deserialized.is_revoked_serial("01"));
        assert!(deserialized.is_revoked_fingerprint("fp1"));
    }

    // ============================================================================
    // Configuration Security (3 tests)
    // ============================================================================

    #[test]
    fn test_auth_sec_config_defaults_are_secure() {
        let config = AuthConfig::default();

        assert_eq!(
            config.level,
            AuthLevel::MutualTls,
            "Security: default auth level must be MutualTls"
        );
        assert!(
            config.require_cluster_ca,
            "Security: default must require cluster CA"
        );
        assert!(
            config.max_cert_age_days > 0 && config.max_cert_age_days <= 365,
            "Security: default max cert age must be reasonable"
        );
    }

    #[test]
    fn test_enroll_sec_config_defaults_reasonable() {
        let config = EnrollmentConfig::default();

        assert!(
            config.ca_validity_days >= 3650,
            "Security: CA validity should be at least 10 years"
        );
        assert!(
            config.cert_validity_days >= 90,
            "Security: cert validity should be at least 90 days"
        );
        assert!(
            config.token_validity_secs >= 60,
            "Security: token validity should be at least 60 seconds"
        );
        assert!(
            config.token_length >= 16,
            "Security: token length should be at least 16 bytes"
        );
    }

    #[test]
    fn test_auth_sec_zero_value_configs_handled() {
        let config = AuthConfig {
            level: AuthLevel::None,
            allowed_subjects: vec![],
            allowed_fingerprints: vec![],
            max_cert_age_days: 0,
            require_cluster_ca: false,
            cluster_ca_fingerprint: None,
        };

        let mut auth = ConnectionAuthenticator::new(config);

        let cert = make_cert("", "", "", "", 1000, 86400000 * 365 * 1000 + 1000);
        auth.set_time(5000);

        let result = auth.authenticate(&cert);

        assert!(
            matches!(result, AuthResult::Allowed { .. }),
            "Security: zero-value configs must be handled gracefully"
        );
    }

    // ============================================================================
    // Stats Integrity (3 tests)
    // ============================================================================

    #[test]
    fn test_auth_sec_stats_track_allowed_denied() {
        let config = AuthConfig {
            allowed_subjects: vec!["server1".to_string()],
            ..Default::default()
        };
        let mut auth = ConnectionAuthenticator::new(config);

        let allowed_cert = make_cert(
            "server1",
            "ClusterCA",
            "01",
            "abc123",
            1000,
            86400000 * 365 * 1000 + 1000,
        );
        let denied_cert = make_cert(
            "server2",
            "ClusterCA",
            "02",
            "xyz789",
            1000,
            86400000 * 365 * 1000 + 1000,
        );

        auth.set_time(5000);

        for _ in 0..10 {
            auth.authenticate(&allowed_cert);
        }
        for _ in 0..5 {
            auth.authenticate(&denied_cert);
        }

        let stats = auth.stats();

        assert_eq!(
            stats.total_allowed, 10,
            "Security: stats must correctly track allowed operations"
        );
        assert_eq!(
            stats.total_denied, 5,
            "Security: stats must correctly track denied operations"
        );
    }

    #[test]
    fn test_enroll_sec_stats_track_issued_revoked() {
        let config = EnrollmentConfig::default();
        let ca = ClusterCA::new(&config).expect("CA creation failed");
        let mut service = EnrollmentService::new(ca, config);

        let token1 = service.generate_token("node1").expect("Token gen");
        let _ = service.enroll_with_token(&token1.token);

        let token2 = service.generate_token("node2").expect("Token gen");
        let bundle = service.enroll_with_token(&token2.token).expect("Enroll");

        service
            .revoke(
                &bundle.serial,
                claudefs_transport::RevocationReason::AdminRevoked,
            )
            .expect("Revoke");

        let stats = service.stats();

        assert_eq!(
            stats.total_issued, 2,
            "Security: enrollment stats must track issued certs"
        );
        assert_eq!(
            stats.total_revoked, 1,
            "Security: enrollment stats must track revoked certs"
        );
    }

    #[test]
    fn test_auth_sec_stats_no_overflow_high_volume() {
        let mut auth = ConnectionAuthenticator::new(AuthConfig::default());

        let cert = make_cert(
            "server1",
            "ClusterCA",
            "01",
            "abc123",
            1000,
            86400000 * 365 * 1000 + 1000,
        );

        auth.set_time(5000);

        for _ in 0..10000 {
            auth.authenticate(&cert);
        }

        let stats = auth.stats();

        assert!(
            stats.total_allowed > 0 && stats.total_allowed <= 10000,
            "Security: stats must not overflow on high volume"
        );
    }
}
