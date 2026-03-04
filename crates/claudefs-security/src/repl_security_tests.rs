// FILE: repl_security_tests.rs

#[cfg(test)]
mod tests {
    use claudefs_repl::{
        auth_ratelimit::{AuthRateLimiter, RateLimitConfig, RateLimitResult},
        batch_auth::{AuthResult, BatchAuthKey, BatchAuthenticator, BatchTag},
        conflict_resolver::{ConflictRecord, ConflictResolver, ConflictType, SiteId},
        journal::{JournalEntry, JournalPosition, JournalTailer, OpKind},
        site_registry::{SiteRecord, SiteRegistry, SiteRegistryError},
        split_brain::FencingToken,
        tls_policy::{
            validate_tls_config, TlsMode, TlsPolicyBuilder, TlsPolicyError, TlsValidator,
        },
        wal::{ReplicationCursor, ReplicationWal},
    };
    use std::collections::HashSet;

    macro_rules! finding {
        ($id:expr, $msg:expr) => {
            eprintln!("FINDING-{}: {}", $id, $msg)
        };
    }

    #[test]
    fn test_journal_crc_validation() {
        let entry = JournalEntry::new(1, 0, 100, 1000000, 42, OpKind::Create, b"payload".to_vec());

        let valid = entry.validate_crc();
        assert!(valid, "CRC should be valid for original entry");

        let mut tampered_entry = entry;
        if let Some(byte) = tampered_entry.payload.first_mut() {
            *byte ^= 0xFF;
        }

        let tampered_valid = tampered_entry.validate_crc();
        assert!(
            !tampered_valid,
            "CRC should fail after payload modification"
        );
    }

    #[test]
    fn test_journal_crc_weak_collision() {
        let entry_a =
            JournalEntry::new(1, 0, 100, 1000000, 42, OpKind::Create, b"payload".to_vec());
        let entry_b =
            JournalEntry::new(1, 0, 200, 1000000, 42, OpKind::Create, b"payload".to_vec());

        let crc_a = entry_a.compute_crc();
        let crc_b = entry_b.compute_crc();

        if crc_a == crc_b {
            finding!("REPL-01", "CRC32 may be insufficient for tampering detection: different site_ids produce same CRC");
        }

        assert_ne!(crc_a, crc_b, "Different entries should have different CRCs");
    }

    #[test]
    fn test_journal_entry_empty_payload() {
        let entry = JournalEntry::new(1, 0, 100, 1000000, 42, OpKind::Create, vec![]);

        let valid = entry.validate_crc();
        assert!(valid, "Empty payload entry should have valid CRC");
        assert_eq!(entry.compute_crc(), entry.crc32);
    }

    #[test]
    fn test_journal_tailer_position_tracking() {
        let entries = vec![
            JournalEntry::new(1, 0, 100, 1000000, 42, OpKind::Create, b"a".to_vec()),
            JournalEntry::new(2, 0, 100, 1000001, 42, OpKind::Write, b"b".to_vec()),
            JournalEntry::new(3, 0, 100, 1000002, 42, OpKind::Truncate, vec![]),
        ];

        let mut tailer = JournalTailer::new_in_memory(entries);

        let pos0 = tailer.position();
        assert!(pos0.is_some());

        let _ = tailer.next();
        let pos1 = tailer.position();
        assert!(pos1.is_some());

        let _ = tailer.next();
        let pos2 = tailer.position();
        assert!(pos2.is_some());

        let _ = tailer.next();
        let pos3 = tailer.position();
        assert!(
            pos3.is_some(),
            "Position returns last seq + 1 at end, not None"
        );
    }

    #[test]
    fn test_journal_filter_by_nonexistent_shard() {
        let entries = vec![
            JournalEntry::new(1, 0, 100, 1000000, 42, OpKind::Create, vec![]),
            JournalEntry::new(2, 1, 100, 1000001, 42, OpKind::Write, vec![]),
        ];

        let tailer = JournalTailer::new_in_memory(entries);
        let filtered = tailer.filter_by_shard(99);

        assert!(
            filtered.is_empty(),
            "Filtering nonexistent shard should return empty"
        );
    }

    #[test]
    fn test_batch_auth_sign_verify_roundtrip() {
        let key = BatchAuthKey::generate();
        let auth = BatchAuthenticator::new(key, 100);

        let entries = vec![JournalEntry::new(
            1,
            0,
            100,
            1000000,
            42,
            OpKind::Create,
            b"data".to_vec(),
        )];

        let tag = auth.sign_batch(100, 1, &entries);
        let result = auth.verify_batch(&tag, 100, 1, &entries);

        match result {
            AuthResult::Valid => {}
            AuthResult::Invalid { reason } => panic!("Valid batch should verify: {}", reason),
        }
    }

    #[test]
    fn test_batch_auth_tampered_entry() {
        let key = BatchAuthKey::generate();
        let auth = BatchAuthenticator::new(key, 100);

        let entries = vec![JournalEntry::new(
            1,
            0,
            100,
            1000000,
            42,
            OpKind::Create,
            b"original".to_vec(),
        )];

        let tag = auth.sign_batch(100, 1, &entries);

        let mut tampered_entries = entries.clone();
        tampered_entries[0].payload = b"tampered".to_vec();

        let result = auth.verify_batch(&tag, 100, 1, &tampered_entries);

        match result {
            AuthResult::Valid => panic!("Tampered entries should fail verification"),
            AuthResult::Invalid { .. } => {}
        }
    }

    #[test]
    fn test_batch_auth_replay_different_seq() {
        let key = BatchAuthKey::generate();
        let auth = BatchAuthenticator::new(key, 100);

        let entries = vec![JournalEntry::new(
            1,
            0,
            100,
            1000000,
            42,
            OpKind::Create,
            vec![],
        )];

        let tag = auth.sign_batch(100, 1, &entries);
        let result = auth.verify_batch(&tag, 100, 2, &entries);

        match result {
            AuthResult::Valid => finding!(
                "REPL-02",
                "Replay protection via batch_seq: same tag validates with different seq"
            ),
            AuthResult::Invalid { reason } => {
                eprintln!("Replay protection working: {}", reason);
            }
        }
    }

    #[test]
    fn test_batch_auth_wrong_key() {
        let key_a = BatchAuthKey::generate();
        let key_b = BatchAuthKey::generate();

        let auth = BatchAuthenticator::new(key_a, 100);

        let entries = vec![JournalEntry::new(
            1,
            0,
            100,
            1000000,
            42,
            OpKind::Create,
            vec![],
        )];

        let tag = auth.sign_batch(100, 1, &entries);

        let auth_b = BatchAuthenticator::new(key_b, 100);
        let result = auth_b.verify_batch(&tag, 100, 1, &entries);

        match result {
            AuthResult::Valid => panic!("Wrong key should fail verification"),
            AuthResult::Invalid { .. } => {}
        }
    }

    #[test]
    fn test_batch_auth_zero_tag() {
        let key = BatchAuthKey::generate();
        let auth = BatchAuthenticator::new(key, 100);

        let entries = vec![JournalEntry::new(
            1,
            0,
            100,
            1000000,
            42,
            OpKind::Create,
            vec![],
        )];

        let zero_tag = BatchTag::zero();
        let result = auth.verify_batch(&zero_tag, 100, 1, &entries);

        match result {
            AuthResult::Valid => finding!("REPL-03", "Zero tag rejection: zero tag was accepted"),
            AuthResult::Invalid { .. } => {}
        }
    }

    #[test]
    fn test_site_registry_fingerprint_mismatch() {
        let mut registry = SiteRegistry::new();

        let record = SiteRecord::new(1, "site1");
        let mut record_with_fp = record;
        record_with_fp.tls_fingerprint = Some([0xAAu8; 32]);

        registry.register(record_with_fp).unwrap();

        let result = registry.verify_source_id(1, Some(&[0xBBu8; 32]));

        match result {
            Err(SiteRegistryError::FingerprintMismatch { .. }) => {}
            Ok(_) => panic!("Fingerprint mismatch should fail verification"),
            Err(e) => panic!("Unexpected error: {:?}", e),
        }
    }

    #[test]
    fn test_site_registry_no_fingerprint_bypass() {
        let mut registry = SiteRegistry::new();

        let mut record = SiteRecord::new(1, "site1");
        record.tls_fingerprint = Some([0xAAu8; 32]);

        registry.register(record).unwrap();

        let result = registry.verify_source_id(1, None);

        match result {
            Ok(_) => finding!(
                "REPL-04",
                "Optional fingerprint bypass: fingerprint check skipped when None provided"
            ),
            Err(e) => eprintln!("Fingerprint bypass prevented: {:?}", e),
        }
    }

    #[test]
    fn test_tls_required_rejects_plaintext() {
        let validator = TlsValidator::new(TlsMode::Required);

        let result = validator.validate_config(&None);

        match result {
            Err(TlsPolicyError::PlaintextNotAllowed) => {}
            Ok(_) => panic!("Required TLS should reject plaintext"),
            Err(e) => panic!("Unexpected error: {:?}", e),
        }
    }

    #[test]
    fn test_tls_testonly_allows_plaintext() {
        let validator = TlsValidator::new(TlsMode::TestOnly);

        let result = validator.validate_config(&None);

        match result {
            Ok(_) => finding!(
                "REPL-05",
                "TestOnly allows plaintext: TLS not enforced in test mode"
            ),
            Err(e) => eprintln!("TestOnly rejected plaintext: {:?}", e),
        }
    }

    #[test]
    fn test_tls_validate_empty_certs() {
        let result = validate_tls_config(b"", b"", b"");

        match result {
            Ok(_) => finding!(
                "REPL-06",
                "Empty cert validation: empty certs passed validation"
            ),
            Err(e) => eprintln!("Empty certs rejected: {:?}", e),
        }
    }

    #[test]
    fn test_conflict_lww_resolution() {
        let mut resolver = ConflictResolver::new();

        let record = resolver.resolve(42, SiteId(1), 100, 1000000, SiteId(2), 200, 1000001);

        assert_eq!(record.winner, SiteId(2), "Later timestamp should win");
        assert_eq!(record.conflict_type, ConflictType::LwwResolved);
    }

    #[test]
    fn test_conflict_equal_timestamps() {
        let mut resolver = ConflictResolver::new();

        let record = resolver.resolve(42, SiteId(1), 100, 1000000, SiteId(2), 200, 1000000);

        if record.winner == SiteId(1) || record.winner == SiteId(2) {
            finding!(
                "REPL-07",
                "Tie-breaking policy: equal timestamps resolved to one site deterministically"
            );
        }
    }

    #[test]
    fn test_fencing_token_monotonic() {
        let token1 = FencingToken::new(1);
        let token2 = token1.next();
        let token3 = token2.next();

        assert!(
            token2.value() > token1.value(),
            "Tokens should be strictly increasing"
        );
        assert!(
            token3.value() > token2.value(),
            "Tokens should be strictly increasing"
        );
        assert_eq!(
            token3.value(),
            token1.value() + 2,
            "Tokens should increment by 1"
        );
    }

    #[test]
    fn test_wal_reset_loses_cursor() {
        let mut wal = ReplicationWal::new();

        wal.advance(1, 0, 100, 1000000, 10);
        wal.advance(1, 0, 200, 1000001, 20);

        let cursor_before = wal.cursor(1, 0);
        assert_eq!(cursor_before.last_seq, 200);

        wal.reset(1, 0);

        let cursor_after = wal.cursor(1, 0);
        assert_eq!(cursor_after.last_seq, 0, "Cursor should be reset to 0");
    }

    #[test]
    fn test_rate_limiter_lockout() {
        let config = RateLimitConfig {
            max_auth_attempts_per_minute: 5,
            max_batches_per_second: 100,
            max_global_bytes_per_second: 1000000,
            lockout_duration_secs: 60,
        };

        let mut limiter = AuthRateLimiter::new(config);
        let now_us = 1000000u64;

        for i in 0..5 {
            let result = limiter.check_auth_attempt(1, now_us + (i as u64 * 1000));
            match result {
                RateLimitResult::Allowed => {}
                RateLimitResult::Throttled { .. } => {}
                RateLimitResult::Blocked { .. } => {
                    finding!(
                        "REPL-08",
                        "Rate limiting enforcement: early block before limit"
                    );
                }
            }
        }

        let sixth_result = limiter.check_auth_attempt(1, now_us + 6000);
        match sixth_result {
            RateLimitResult::Blocked { .. } => {}
            _ => finding!(
                "REPL-08",
                "Rate limiting enforcement: exceeded attempts not blocked"
            ),
        }
    }
}
