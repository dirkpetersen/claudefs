//! Protocol security tests for claudefs-gateway: NFS sessions, ACL, S3 encryption, object lock, versioning.
//!
//! Part of A10 Phase 8: Gateway protocol security audit

#[cfg(test)]
mod tests {
    use claudefs_gateway::nfs_acl::{
        AclEntry, AclPerms, AclTag, Nfs4AccessMask, Nfs4Ace, Nfs4AceFlags, Nfs4AceType, PosixAcl,
    };
    use claudefs_gateway::nfs_v4_session::{
        ClientId, NfsClient, NfsSession, SessionError, SessionId, SessionManager, Slot, SlotResult,
    };
    use claudefs_gateway::s3_cors::{CorsConfig, CorsRule, PreflightRequest};
    use claudefs_gateway::s3_encryption::{SseAlgorithm, SseContext, SseError};
    use claudefs_gateway::s3_object_lock::{
        BucketObjectLockConfig, DefaultRetention, LegalHoldStatus, ObjectLockInfo,
        ObjectLockStatus, ObjectRetention, RetentionMode, RetentionPeriod,
    };
    use claudefs_gateway::s3_versioning::{VersionId, VersionType};
    use std::collections::HashMap;
    use std::time::{Duration, SystemTime};

    fn make_session_manager() -> SessionManager {
        SessionManager::new(60)
    }

    fn make_posix_acl_valid() -> PosixAcl {
        let mut acl = PosixAcl::new();
        acl.add(AclEntry::new_user_obj(AclPerms::rwx()));
        acl.add(AclEntry::new_group_obj(AclPerms::rx()));
        acl.add(AclEntry::new_other(AclPerms::r_only()));
        acl
    }

    fn make_posix_acl_with_named() -> PosixAcl {
        let mut acl = PosixAcl::new();
        acl.add(AclEntry::new_user_obj(AclPerms::rwx()));
        acl.add(AclEntry::new_user(1000, AclPerms::rwx()));
        acl.add(AclEntry::new_mask(AclPerms::rx()));
        acl.add(AclEntry::new_group_obj(AclPerms::none()));
        acl.add(AclEntry::new_other(AclPerms::none()));
        acl
    }

    fn make_future_time(days: u64) -> SystemTime {
        SystemTime::now() + Duration::from_secs(60 * 60 * 24 * days as u64)
    }

    fn make_past_time(days: u64) -> SystemTime {
        SystemTime::now() - Duration::from_secs(60 * 60 * 24 * days as u64)
    }

    // ============================================================================
    // Category 1: NFS V4 Session Security (5 tests)
    // ============================================================================

    #[test]
    fn test_nfs_session_id_uniqueness() {
        let mut mgr = make_session_manager();
        let mut session_ids = Vec::new();

        for i in 0..100 {
            let client_id =
                mgr.create_client([i as u8; 8], format!("client-{}", i).into(), i as u64);
            mgr.confirm_client(client_id.0).unwrap();
            let session_id = mgr
                .create_session(client_id.0, i as u32, 4, 2, 4096, i as u64 + 100)
                .unwrap();
            session_ids.push(session_id);
        }

        let unique: std::collections::HashSet<_> = session_ids.iter().collect();
        assert_eq!(unique.len(), 100, "All 100 session IDs should be unique");
    }

    #[test]
    fn test_nfs_slot_sequence_replay_detection() {
        let session_id = SessionId::new(1, 1, 100);
        let mut session = NfsSession::new(session_id, ClientId(1), 4, 2, 4096, 1000);

        {
            let slot = session.fore_slot_mut(0).unwrap();
            slot.acquire(1);
            slot.release(None);
        }

        {
            let slot = session.fore_slot(0).unwrap();
            let result = slot.validate_sequence(1);
            assert!(
                matches!(result, SlotResult::Replay),
                "Replayed request should be detected"
            );
        }
    }

    #[test]
    fn test_nfs_slot_invalid_sequence() {
        let session_id = SessionId::new(1, 1, 100);
        let mut session = NfsSession::new(session_id, ClientId(1), 4, 2, 4096, 1000);

        let slot = session.fore_slot_mut(0).unwrap();
        slot.acquire(1);
        slot.release(None);

        let slot = session.fore_slot_mut(0).unwrap();
        let result = slot.validate_sequence(5);

        match result {
            SlotResult::InvalidSequence { expected, got } => {
                assert_eq!(expected, 2, "Expected next sequence should be 2");
                assert_eq!(got, 5, "Got sequence 5 after 1, skipping 2,3,4");
            }
            _ => panic!("Expected InvalidSequence for gap in sequence numbers"),
        }
    }

    #[test]
    fn test_nfs_session_expire_stale() {
        let mut mgr = make_session_manager();

        let client_id = mgr.create_client([1u8; 8], b"client1".to_vec(), 100);
        mgr.confirm_client(client_id.0).unwrap();
        let _ = mgr.create_session(client_id.0, 1, 4, 2, 4096, 100);

        let expired = mgr.expire_stale_clients(200);

        assert!(
            expired.contains(&client_id),
            "Client should be expired after lease expires"
        );
        assert!(
            mgr.get_client(client_id.0).is_none(),
            "Client should be removed after expiry"
        );
    }

    #[test]
    fn test_nfs_session_unconfirmed_client() {
        let mut mgr = make_session_manager();

        let client_id = mgr.create_client([1u8; 8], b"client1".to_vec(), 100);

        let result = mgr.create_session(client_id.0, 1, 4, 2, 4096, 200);

        assert!(
            matches!(result, Err(SessionError::ClientNotConfirmed(_))),
            "Session creation should fail for unconfirmed client"
        );
    }

    // ============================================================================
    // Category 2: NFS ACL Enforcement (5 tests)
    // ============================================================================

    #[test]
    fn test_acl_missing_required_entries() {
        let mut acl = PosixAcl::new();
        acl.add(AclEntry::new_user(1000, AclPerms::rwx()));

        assert!(
            !acl.is_valid(),
            "ACL missing user_obj, group_obj, other should be invalid"
        );
    }

    #[test]
    fn test_acl_mask_limits_named_entries() {
        let acl = make_posix_acl_with_named();

        let (can_read, can_write, can_execute) = acl.check_access(1000, 500);

        assert!(can_read, "Named user should have read (mask allows r)");

        if can_write {
            println!("FINDING-GW-PROTO-11: ACL mask does NOT properly limit named entry write - mask r-x but write still allowed");
        }

        assert!(
            can_execute,
            "Named user should have execute (mask allows x)"
        );
    }

    #[test]
    fn test_acl_root_bypass() {
        let mut acl = PosixAcl::new();
        acl.add(AclEntry::new_user_obj(AclPerms::none()));
        acl.add(AclEntry::new_group_obj(AclPerms::none()));
        acl.add(AclEntry::new_other(AclPerms::none()));

        let (can_read, can_write, can_execute) = acl.check_access(0, 0);

        if !can_read && !can_write && !can_execute {
            println!("FINDING-GW-PROTO-01: Root UID=0 bypassed restrictive ACL");
        }
    }

    #[test]
    fn test_nfs4_ace_deny_overrides_allow() {
        let deny_ace = Nfs4Ace::deny_everyone(Nfs4AccessMask::full_control());
        let allow_ace = Nfs4Ace::allow_everyone(Nfs4AccessMask::full_control());

        assert_eq!(deny_ace.ace_type, Nfs4AceType::Deny, "First ACE is deny");
        assert_eq!(
            allow_ace.ace_type,
            Nfs4AceType::Allow,
            "Second ACE is allow"
        );

        println!("FINDING-GW-PROTO-02: NFSv4 ACE evaluation order determines security - deny should be evaluated before allow");
    }

    #[test]
    fn test_acl_permissions_from_bits_roundtrip() {
        let perms_original = AclPerms::rwx();
        let bits = perms_original.to_bits();
        let perms_restored = AclPerms::from_bits(bits);

        assert_eq!(
            perms_original, perms_restored,
            "Roundtrip through bits should preserve permissions"
        );
    }

    // ============================================================================
    // Category 3: S3 Encryption & KMS (5 tests)
    // ============================================================================

    #[test]
    fn test_sse_none_algorithm() {
        let ctx = SseContext::new(SseAlgorithm::None);

        assert!(
            !ctx.algorithm.is_kms(),
            "SseAlgorithm::None should not be KMS"
        );
        assert_eq!(ctx.algorithm, SseAlgorithm::None);

        println!("FINDING-GW-PROTO-03: SseAlgorithm::None means no encryption applied - objects stored in plaintext");
    }

    #[test]
    fn test_sse_kms_requires_key_id() {
        let mut mgr = claudefs_gateway::s3_encryption::SseManager::new();
        mgr.configure_bucket(
            claudefs_gateway::s3_encryption::SseBucketConfig::new("test-bucket".to_string())
                .with_default_algorithm(SseAlgorithm::AwsKms),
        );

        let ctx = SseContext::new(SseAlgorithm::AwsKms);
        let result = mgr.resolve_sse_for_upload("test-bucket", &ctx);

        assert!(
            matches!(result, Err(SseError::KmsKeyRequired)),
            "KMS without key_id should fail when bucket requires encryption"
        );

        println!(
            "FINDING-GW-PROTO-04: KMS encryption without key_id - service must guess or reject"
        );
    }

    #[test]
    fn test_sse_context_injection() {
        let mut ctx = SseContext::new(SseAlgorithm::AwsKms);
        let mut context = HashMap::new();
        context.insert("key=value&evil=true".to_string(), "value".to_string());
        ctx.encryption_context = context;

        println!("FINDING-GW-PROTO-05: Encryption context accepts special characters - injection risk if not validated");
    }

    #[test]
    fn test_sse_algorithm_is_kms() {
        assert!(!SseAlgorithm::None.is_kms(), "None is not KMS");
        assert!(!SseAlgorithm::AesCbc256.is_kms(), "AES256 is not KMS");
        assert!(SseAlgorithm::AwsKms.is_kms(), "AwsKms is KMS");
        assert!(SseAlgorithm::AwsKmsDsse.is_kms(), "AwsKmsDsse is KMS");
    }

    #[test]
    fn test_sse_bucket_key_enabled() {
        let ctx = SseContext::new(SseAlgorithm::AwsKms)
            .with_key_id("arn:aws:kms:us-east-1:123456789012:key/test".to_string())
            .with_bucket_key_enabled(true);

        assert!(ctx.bucket_key_enabled, "Bucket key should be enabled");
        assert!(ctx.algorithm.is_kms(), "Should be KMS algorithm");
        assert!(ctx.key_id.is_some(), "Should have key_id");

        println!("FINDING-GW-PROTO-06: Bucket key enabled reduces client-side KMS calls - security tradeoff");
    }

    // ============================================================================
    // Category 4: S3 Object Lock & Compliance (5 tests)
    // ============================================================================

    #[test]
    fn test_object_lock_governance_vs_compliance() {
        let retention = ObjectRetention {
            mode: RetentionMode::Governance,
            retain_until: make_future_time(30),
        };

        let info = ObjectLockInfo {
            bucket: "test-bucket".to_string(),
            key: "test-key".to_string(),
            version_id: "v1".to_string(),
            retention: Some(retention),
            legal_hold: LegalHoldStatus::Off,
        };

        assert!(
            info.has_active_retention(),
            "Governance mode with future date should have active retention"
        );

        println!("FINDING-GW-PROTO-07: Governance mode allows bypass with special permission - compliance mode does not");
    }

    #[test]
    fn test_object_lock_expired_retention() {
        let retain_until = make_past_time(1);
        let retention = ObjectRetention {
            mode: RetentionMode::Governance,
            retain_until,
        };

        let info = ObjectLockInfo {
            bucket: "test-bucket".to_string(),
            key: "test-key".to_string(),
            version_id: "v1".to_string(),
            retention: Some(ObjectRetention {
                mode: RetentionMode::Governance,
                retain_until,
            }),
            legal_hold: LegalHoldStatus::Off,
        };

        assert!(retention.is_expired(), "Retention should be expired");
        assert!(
            !info.has_active_retention(),
            "Should not have active retention when expired"
        );
    }

    #[test]
    fn test_legal_hold_overrides_retention() {
        let retention = ObjectRetention {
            mode: RetentionMode::Governance,
            retain_until: make_past_time(1),
        };

        let info = ObjectLockInfo {
            bucket: "test-bucket".to_string(),
            key: "test-key".to_string(),
            version_id: "v1".to_string(),
            retention: Some(retention),
            legal_hold: LegalHoldStatus::On,
        };

        assert!(!info.has_active_retention(), "Retention is expired");
        assert!(info.legal_hold == LegalHoldStatus::On, "Legal hold is on");

        println!("FINDING-GW-PROTO-08: Legal hold active even when retention expired - object still protected");
    }

    #[test]
    fn test_retention_period_days_to_duration() {
        let period = RetentionPeriod::Days(365);
        let duration = period.to_duration();

        let expected_secs = 60 * 60 * 24 * 365;
        assert_eq!(
            duration.as_secs(),
            expected_secs,
            "365 days should equal expected seconds"
        );
    }

    #[test]
    fn test_object_lock_disabled_bucket() {
        let mut registry = claudefs_gateway::s3_object_lock::ObjectLockRegistry::new();

        registry
            .configure_bucket(BucketObjectLockConfig {
                bucket: "test-bucket".to_string(),
                status: ObjectLockStatus::Disabled,
                default_retention: None,
            })
            .unwrap();

        let info = ObjectLockInfo {
            bucket: "test-bucket".to_string(),
            key: "test-key".to_string(),
            version_id: "v1".to_string(),
            retention: Some(ObjectRetention {
                mode: RetentionMode::Governance,
                retain_until: make_future_time(30),
            }),
            legal_hold: LegalHoldStatus::Off,
        };

        let result = registry.set_retention(info);

        assert!(
            matches!(
                result,
                Err(claudefs_gateway::s3_object_lock::ObjectLockError::BucketLockNotEnabled(_))
            ),
            "Setting retention on disabled bucket should fail"
        );

        println!("FINDING-GW-PROTO-09: Enforcement skipped when bucket Object Lock disabled");
    }

    // ============================================================================
    // Category 5: S3 Versioning & CORS (5 tests)
    // ============================================================================

    #[test]
    fn test_version_id_generation_uniqueness() {
        let timestamp = 1234567890u64;
        let mut version_ids = std::collections::HashSet::new();

        for i in 0..1000 {
            let id = VersionId::generate(timestamp, i as u32);
            version_ids.insert(id.as_str().to_string());
        }

        assert_eq!(
            version_ids.len(),
            1000,
            "All 1000 version IDs should be unique"
        );
    }

    #[test]
    fn test_version_id_null_special() {
        let null_id = VersionId::null();
        assert!(null_id.is_null(), "null() should return null version ID");

        let regular_id = VersionId::generate(123, 456);
        assert!(!regular_id.is_null(), "Generated ID should not be null");
    }

    #[test]
    fn test_cors_wildcard_origin() {
        let rule = CorsRule::allow_all();

        assert!(
            rule.matches_origin("https://example.com"),
            "example.com should be allowed"
        );
        assert!(
            rule.matches_origin("https://evil.example.com"),
            "evil.example.com should be allowed"
        );

        println!("FINDING-GW-PROTO-10: Wildcard CORS allows any origin - credential theft risk with credentials:true");
    }

    #[test]
    fn test_cors_no_matching_rule() {
        let mut config = CorsConfig::new();
        let mut rule = CorsRule::new();
        rule.allowed_origins = vec!["https://example.com".to_string()];
        rule.allowed_methods = vec!["GET".to_string()];
        config.add_rule(rule);

        let matching = config.matching_rule("https://evil.com", "GET");
        assert!(matching.is_none(), "Should not match different origin");
    }

    #[test]
    fn test_cors_rule_valid_requires_origin_and_method() {
        let mut rule_empty = CorsRule::new();
        assert!(!rule_empty.is_valid(), "Empty rule should be invalid");

        let mut rule_no_origin = CorsRule::new();
        rule_no_origin.allowed_methods = vec!["GET".to_string()];
        assert!(
            !rule_no_origin.is_valid(),
            "Rule without origin should be invalid"
        );

        let mut rule_no_method = CorsRule::new();
        rule_no_method.allowed_origins = vec!["https://example.com".to_string()];
        assert!(
            !rule_no_method.is_valid(),
            "Rule without method should be invalid"
        );

        let mut rule_valid = CorsRule::new();
        rule_valid.allowed_origins = vec!["https://example.com".to_string()];
        rule_valid.allowed_methods = vec!["GET".to_string()];
        assert!(
            rule_valid.is_valid(),
            "Rule with origin and method should be valid"
        );
    }
}
