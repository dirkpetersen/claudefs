//! Replication split-brain detection and TLS policy security tests.
//!
//! Part of A10 Phase 24: Repl split-brain/TLS security audit

#[cfg(test)]
mod tests {
    use claudefs_repl::split_brain::{
        FencingToken, SplitBrainDetector, SplitBrainEvidence, SplitBrainState, SplitBrainStats,
    };
    use claudefs_repl::tls_policy::{
        validate_tls_config, TlsConfigRef, TlsMode, TlsPolicyBuilder, TlsPolicyError, TlsValidator,
    };

    fn make_evidence() -> SplitBrainEvidence {
        SplitBrainEvidence {
            site_a_last_seq: 100,
            site_b_last_seq: 99,
            site_a_diverge_seq: 50,
            detected_at_ns: 1000,
        }
    }

    fn make_valid_tls() -> Option<TlsConfigRef> {
        Some(TlsConfigRef {
            cert_pem: b"-----BEGIN CERTIFICATE-----\ntest\n-----END CERTIFICATE-----".to_vec(),
            key_pem: b"key-data".to_vec(),
            ca_pem: b"ca-data".to_vec(),
        })
    }

    // Category 1: Fencing Token (4 tests)

    #[test]
    fn test_fencing_token_new_and_value() {
        let token = FencingToken::new(42);
        assert_eq!(token.value(), 42);

        let token_zero = FencingToken::new(0);
        assert_eq!(token_zero.value(), 0);
    }

    #[test]
    fn test_fencing_token_next() {
        let token = FencingToken::new(5);
        let next = token.next();
        assert_eq!(next.value(), 6);
        assert_eq!(token.value(), 5);
    }

    #[test]
    fn test_fencing_token_ordering() {
        let t1 = FencingToken::new(1);
        let t2 = FencingToken::new(2);
        let t3 = FencingToken::new(3);
        assert!(t1 < t2);
        assert!(t2 < t3);
        assert!(t3 > t1);
    }

    #[test]
    fn test_fencing_token_equality() {
        assert_eq!(FencingToken::new(5), FencingToken::new(5));
        assert_ne!(FencingToken::new(5), FencingToken::new(6));
    }

    // Category 2: Split-Brain State Machine (6 tests)

    #[test]
    fn test_detector_initial_state() {
        let detector = SplitBrainDetector::new(1);
        assert!(matches!(detector.state(), SplitBrainState::Normal));
        assert_eq!(detector.current_token().value(), 1);
        let stats = detector.stats();
        assert_eq!(stats.partitions_detected, 0);
        assert_eq!(stats.split_brains_confirmed, 0);
        assert_eq!(stats.resolutions_completed, 0);
        assert_eq!(stats.fencing_tokens_issued, 0);
    }

    #[test]
    fn test_report_partition() {
        let mut detector = SplitBrainDetector::new(1);
        detector.report_partition(2, 1000);

        let state = detector.state();
        assert!(matches!(
            state,
            SplitBrainState::PartitionSuspected {
                since_ns: 1000,
                site_id: 2
            }
        ));
        assert_eq!(detector.stats().partitions_detected, 1);
    }

    #[test]
    fn test_confirm_split_brain_requires_partition() {
        let mut detector = SplitBrainDetector::new(1);
        let evidence = make_evidence();
        let state = detector.confirm_split_brain(evidence, 1, 2);

        assert!(matches!(state, SplitBrainState::Normal));
        assert_eq!(detector.stats().split_brains_confirmed, 0);
    }

    #[test]
    fn test_confirm_from_partition() {
        let mut detector = SplitBrainDetector::new(1);
        detector.report_partition(2, 1000);

        let evidence = make_evidence();
        let state = detector.confirm_split_brain(evidence, 1, 2);

        assert!(matches!(
            state,
            SplitBrainState::Confirmed {
                site_a: 1,
                site_b: 2,
                diverged_at_seq: 50
            }
        ));
        assert_eq!(detector.stats().split_brains_confirmed, 1);
    }

    #[test]
    fn test_issue_fence_requires_confirmed() {
        let mut detector = SplitBrainDetector::new(1);
        let token = detector.issue_fence(2, 1);

        assert_eq!(token.value(), 1);
        assert_eq!(detector.stats().fencing_tokens_issued, 0);
    }

    #[test]
    fn test_full_lifecycle() {
        let mut detector = SplitBrainDetector::new(1);

        assert!(matches!(detector.state(), SplitBrainState::Normal));

        detector.report_partition(2, 1000);
        assert!(matches!(
            detector.state(),
            SplitBrainState::PartitionSuspected { .. }
        ));

        let evidence = make_evidence();
        detector.confirm_split_brain(evidence, 1, 2);
        assert!(matches!(
            detector.state(),
            SplitBrainState::Confirmed { .. }
        ));

        detector.issue_fence(2, 1);
        assert!(matches!(
            detector.state(),
            SplitBrainState::Resolving { .. }
        ));

        detector.mark_healed(5000);
        assert!(matches!(detector.state(), SplitBrainState::Healed { .. }));

        detector.mark_healed(6000);
        assert!(matches!(detector.state(), SplitBrainState::Normal));

        let stats = detector.stats();
        assert_eq!(stats.partitions_detected, 1);
        assert_eq!(stats.split_brains_confirmed, 1);
        assert_eq!(stats.fencing_tokens_issued, 1);
        assert_eq!(stats.resolutions_completed, 1);
    }

    // Category 3: Fencing & Validation (4 tests)

    #[test]
    fn test_issue_fence_increments_token() {
        let mut detector = SplitBrainDetector::new(1);
        detector.report_partition(2, 1000);

        let evidence = make_evidence();
        detector.confirm_split_brain(evidence, 1, 2);

        let token1 = detector.issue_fence(2, 1);
        assert_eq!(token1.value(), 2);

        detector.report_partition(3, 2000);
        let evidence2 = SplitBrainEvidence {
            site_a_last_seq: 200,
            site_b_last_seq: 199,
            site_a_diverge_seq: 150,
            detected_at_ns: 2000,
        };
        detector.confirm_split_brain(evidence2, 1, 3);

        let token2 = detector.issue_fence(3, 1);
        assert_eq!(token2.value(), 3);
    }

    #[test]
    fn test_validate_token_current_or_higher() {
        let mut detector = SplitBrainDetector::new(1);
        detector.report_partition(2, 1000);

        let evidence = make_evidence();
        detector.confirm_split_brain(evidence, 1, 2);
        let token = detector.issue_fence(2, 1);

        assert!(detector.validate_token(token));
        assert!(detector.validate_token(FencingToken::new(100)));

        assert!(!detector.validate_token(FencingToken::new(1)));
    }

    #[test]
    fn test_mark_healed_from_wrong_state() {
        let mut detector = SplitBrainDetector::new(1);
        let state = detector.mark_healed(5000);

        assert!(matches!(state, SplitBrainState::Normal));
        assert_eq!(detector.stats().resolutions_completed, 0);
    }

    #[test]
    fn test_mark_healed_two_step() {
        let mut detector = SplitBrainDetector::new(1);
        detector.report_partition(2, 1000);

        let evidence = make_evidence();
        detector.confirm_split_brain(evidence, 1, 2);
        detector.issue_fence(2, 1);

        let state = detector.mark_healed(5000);
        assert!(matches!(state, SplitBrainState::Healed { at_ns: 5000 }));

        let final_state = detector.mark_healed(6000);
        assert!(matches!(final_state, SplitBrainState::Normal));
    }

    // Category 4: TLS Policy Validation (6 tests)

    #[test]
    fn test_tls_required_rejects_none() {
        let validator = TlsValidator::new(TlsMode::Required);
        let result = validator.validate_config(&None);
        assert!(matches!(result, Err(TlsPolicyError::PlaintextNotAllowed)));
    }

    #[test]
    fn test_tls_required_accepts_valid() {
        let validator = TlsValidator::new(TlsMode::Required);
        let result = validator.validate_config(&make_valid_tls());
        assert!(result.is_ok());
    }

    #[test]
    fn test_tls_required_rejects_empty_cert() {
        let validator = TlsValidator::new(TlsMode::Required);
        let result = validator.validate_config(&Some(TlsConfigRef {
            cert_pem: vec![],
            key_pem: b"key".to_vec(),
            ca_pem: b"ca".to_vec(),
        }));
        assert!(matches!(
            result,
            Err(TlsPolicyError::InvalidCertificate { reason })
            if reason.contains("cert_pem")
        ));
    }

    #[test]
    fn test_tls_required_rejects_empty_key() {
        let validator = TlsValidator::new(TlsMode::Required);
        let result = validator.validate_config(&Some(TlsConfigRef {
            cert_pem: b"cert".to_vec(),
            key_pem: vec![],
            ca_pem: b"ca".to_vec(),
        }));
        assert!(matches!(
            result,
            Err(TlsPolicyError::InvalidCertificate { reason })
            if reason.contains("key_pem")
        ));
    }

    #[test]
    fn test_tls_required_rejects_bad_pem_format() {
        let validator = TlsValidator::new(TlsMode::Required);
        let result = validator.validate_config(&Some(TlsConfigRef {
            cert_pem: b"NOT A CERTIFICATE".to_vec(),
            key_pem: b"key".to_vec(),
            ca_pem: b"ca".to_vec(),
        }));
        assert!(matches!(
            result,
            Err(TlsPolicyError::InvalidCertificate { .. })
        ));
    }

    #[test]
    fn test_tls_test_only_allows_none() {
        let validator = TlsValidator::new(TlsMode::TestOnly);
        let result = validator.validate_config(&None);
        assert!(result.is_ok());

        let result2 = validator.validate_config(&make_valid_tls());
        assert!(result2.is_ok());
    }

    // Category 5: TLS Builder & Edge Cases (5 tests)

    #[test]
    fn test_tls_disabled_allows_all() {
        let validator = TlsValidator::new(TlsMode::Disabled);
        let result = validator.validate_config(&None);
        assert!(result.is_ok());

        let result2 = validator.validate_config(&make_valid_tls());
        assert!(result2.is_ok());
    }

    #[test]
    fn test_tls_builder_default() {
        let builder = TlsPolicyBuilder::new();
        let validator = builder.build();
        assert!(matches!(validator.mode(), TlsMode::TestOnly));
    }

    #[test]
    fn test_tls_builder_set_mode() {
        let builder = TlsPolicyBuilder::new().mode(TlsMode::Required);
        let validator = builder.build();
        assert!(matches!(validator.mode(), TlsMode::Required));
    }

    #[test]
    fn test_plaintext_allowed_modes() {
        let required = TlsValidator::new(TlsMode::Required);
        assert!(!required.is_plaintext_allowed());

        let test_only = TlsValidator::new(TlsMode::TestOnly);
        assert!(test_only.is_plaintext_allowed());

        let disabled = TlsValidator::new(TlsMode::Disabled);
        assert!(disabled.is_plaintext_allowed());
    }

    #[test]
    fn test_validate_tls_config_standalone() {
        let result = validate_tls_config(
            b"-----BEGIN CERTIFICATE-----\ntest\n-----END CERTIFICATE-----",
            b"key-data",
            b"ca-data",
        );
        assert!(result.is_ok());

        let result_empty = validate_tls_config(b"", b"key", b"ca");
        assert!(result_empty.is_err());

        let result_bad_pem = validate_tls_config(b"NOT A CERT", b"key", b"ca");
        assert!(result_bad_pem.is_err());
    }
}
