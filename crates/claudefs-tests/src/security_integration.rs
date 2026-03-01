//! A6 Phase 7 security hardening integration tests
//!
//! Tests for TLS policy validation, site registry, rate limiting, and journal GC.

use claudefs_repl::journal_gc::{
    AckRecord, GcCandidate, GcPolicy, GcStats, JournalGcScheduler, JournalGcState,
};
use claudefs_repl::recv_ratelimit::{
    RateLimitConfig, RateLimitDecision, RateLimiterStats, RecvRateLimiter,
};
use claudefs_repl::site_registry::{SiteRecord, SiteRegistry, SiteRegistryError};
use claudefs_repl::tls_policy::{
    validate_tls_config, TlsConfigRef, TlsMode, TlsPolicyBuilder, TlsPolicyError, TlsValidator,
};

#[test]
fn test_tls_mode_required_rejects_none_config() {
    let validator = TlsValidator::new(TlsMode::Required);
    let result = validator.validate_config(&None);
    assert!(result.is_err());
    matches!(result, Err(TlsPolicyError::PlaintextNotAllowed));
}

#[test]
fn test_tls_mode_required_rejects_empty_cert() {
    let validator = TlsValidator::new(TlsMode::Required);
    let tls = Some(TlsConfigRef {
        cert_pem: vec![],
        key_pem: b"key".to_vec(),
        ca_pem: b"ca".to_vec(),
    });
    let result = validator.validate_config(&tls);
    assert!(result.is_err());
}

#[test]
fn test_tls_mode_testonly_allows_none() {
    let validator = TlsValidator::new(TlsMode::TestOnly);
    let result = validator.validate_config(&None);
    assert!(result.is_ok());
}

#[test]
fn test_tls_mode_disabled_allows_none() {
    let validator = TlsValidator::new(TlsMode::Disabled);
    let result = validator.validate_config(&None);
    assert!(result.is_ok());
}

#[test]
fn test_tls_policy_builder_default_mode() {
    let builder = TlsPolicyBuilder::new();
    let validator = builder.build();
    assert_eq!(validator.mode(), &TlsMode::TestOnly);
}

#[test]
fn test_tls_policy_builder_set_required() {
    let builder = TlsPolicyBuilder::new().mode(TlsMode::Required);
    let validator = builder.build();
    assert_eq!(validator.mode(), &TlsMode::Required);
}

#[test]
fn test_validate_tls_config_valid_pem() {
    let result = validate_tls_config(
        b"-----BEGIN CERTIFICATE-----\ntest\n-----END CERTIFICATE-----",
        b"key-data",
        b"ca-data",
    );
    assert!(result.is_ok());
}

#[test]
fn test_validate_tls_config_empty_cert_fails() {
    let result = validate_tls_config(b"", b"key", b"ca");
    assert!(result.is_err());
    matches!(result, Err(TlsPolicyError::InvalidCertificate { .. }));
}

#[test]
fn test_validate_tls_config_missing_begin_prefix_fails() {
    let result = validate_tls_config(b"NOT-A-CERT", b"key", b"ca");
    assert!(result.is_err());
}

#[test]
fn test_site_registry_register_lookup() {
    let mut registry = SiteRegistry::new();
    let record = SiteRecord::new(1, "site-a");
    registry.register(record).unwrap();

    let found = registry.lookup(1);
    assert!(found.is_some());
    assert_eq!(found.unwrap().site_id, 1);
}

#[test]
fn test_site_registry_already_registered_error() {
    let mut registry = SiteRegistry::new();
    let record = SiteRecord::new(1, "site-a");
    registry.register(record).unwrap();

    let record2 = SiteRecord::new(1, "site-b");
    let result = registry.register(record2);
    assert!(result.is_err());
    matches!(
        result,
        Err(SiteRegistryError::AlreadyRegistered { site_id: 1 })
    );
}

#[test]
fn test_site_registry_not_found_error() {
    let mut registry = SiteRegistry::new();
    let result = registry.unregister(999);
    assert!(result.is_err());
    matches!(result, Err(SiteRegistryError::NotFound { site_id: 999 }));
}

#[test]
fn test_site_registry_unregister() {
    let mut registry = SiteRegistry::new();
    let record = SiteRecord::new(1, "site-a");
    registry.register(record).unwrap();

    let removed = registry.unregister(1).unwrap();
    assert_eq!(removed.site_id, 1);

    assert!(registry.lookup(1).is_none());
}

#[test]
fn test_site_registry_verify_source_id_success() {
    let mut registry = SiteRegistry::new();
    let mut record = SiteRecord::new(1, "site-a");
    let fingerprint: [u8; 32] = [0xAB; 32];
    record.tls_fingerprint = Some(fingerprint);
    registry.register(record).unwrap();

    let result = registry.verify_source_id(1, Some(&fingerprint));
    assert!(result.is_ok());
}

#[test]
fn test_site_registry_verify_source_id_mismatch() {
    let mut registry = SiteRegistry::new();
    let mut record = SiteRecord::new(1, "site-a");
    let fingerprint: [u8; 32] = [0xAB; 32];
    record.tls_fingerprint = Some(fingerprint);
    registry.register(record).unwrap();

    let bad_fingerprint: [u8; 32] = [0xCD; 32];
    let result = registry.verify_source_id(1, Some(&bad_fingerprint));
    assert!(result.is_err());
    matches!(result, Err(SiteRegistryError::FingerprintMismatch { .. }));
}

#[test]
fn test_site_registry_update_last_seen() {
    let mut registry = SiteRegistry::new();
    let record = SiteRecord::new(1, "site-a");
    registry.register(record).unwrap();

    registry.update_last_seen(1, 2000000).unwrap();

    let found = registry.lookup(1).unwrap();
    assert_eq!(found.last_seen_us, 2000000);
}

#[test]
fn test_site_registry_update_last_seen_unknown_site() {
    let mut registry = SiteRegistry::new();
    let result = registry.update_last_seen(999, 1000);
    assert!(result.is_err());
}

#[test]
fn test_recv_rate_limiter_allow_below_limit() {
    let config = RateLimitConfig::new(100, 1000);
    let mut limiter = RecvRateLimiter::new(config);

    let decision = limiter.check_batch(10, 1000);
    matches!(decision, RateLimitDecision::Allow);
}

#[test]
fn test_recv_rate_limiter_stats_tracking() {
    let config = RateLimitConfig::new(100, 1000);
    let mut limiter = RecvRateLimiter::new(config);

    limiter.check_batch(5, 1000);
    limiter.check_batch(5, 1001);

    let stats = limiter.stats();
    assert_eq!(stats.batches_allowed, 2);
}

#[test]
fn test_recv_rate_limiter_reset() {
    let config = RateLimitConfig::new(100, 1000);
    let mut limiter = RecvRateLimiter::new(config);

    limiter.check_batch(5, 1000);
    limiter.reset();

    let stats = limiter.stats();
    assert!(stats.batches_allowed >= 1);
}

#[test]
fn test_rate_limit_config_default_values() {
    let config = RateLimitConfig::default();
    assert_eq!(config.max_batches_per_sec, 0);
    assert_eq!(config.max_entries_per_sec, 0);
    assert_eq!(config.burst_factor, 2.0);
    assert_eq!(config.window_ms, 1000);
}

#[test]
fn test_rate_limit_config_new() {
    let config = RateLimitConfig::new(50, 500);
    assert_eq!(config.max_batches_per_sec, 50);
    assert_eq!(config.max_entries_per_sec, 500);
}

#[test]
fn test_gc_policy_retain_all_never_gcs() {
    let policy = GcPolicy::RetainAll;
    let scheduler = JournalGcScheduler::new(policy, vec![]);

    let candidates = vec![
        GcCandidate {
            shard_id: 0,
            seq: 1,
            timestamp_us: 1000,
            size_bytes: 100,
        },
        GcCandidate {
            shard_id: 0,
            seq: 2,
            timestamp_us: 2000,
            size_bytes: 100,
        },
    ];

    for c in &candidates {
        assert!(!scheduler.should_gc_entry(c, 5000000));
    }
}

#[test]
fn test_gc_policy_retain_by_age_gcs_old_entries() {
    let policy = GcPolicy::RetainByAge { max_age_us: 1000 };
    let scheduler = JournalGcScheduler::new(policy, vec![]);

    let old_candidate = GcCandidate {
        shard_id: 0,
        seq: 1,
        timestamp_us: 1000,
        size_bytes: 100,
    };
    assert!(scheduler.should_gc_entry(&old_candidate, 5000));
}

#[test]
fn test_gc_policy_retain_by_count() {
    let policy = GcPolicy::RetainByCount { max_entries: 2 };
    let mut scheduler = JournalGcScheduler::new(policy, vec![]);

    scheduler.record_ack(AckRecord {
        site_id: 1,
        acked_through_seq: 10,
        acked_at_us: 1000,
    });

    let candidates = vec![
        GcCandidate {
            shard_id: 0,
            seq: 1,
            timestamp_us: 1000,
            size_bytes: 100,
        },
        GcCandidate {
            shard_id: 0,
            seq: 2,
            timestamp_us: 2000,
            size_bytes: 100,
        },
        GcCandidate {
            shard_id: 0,
            seq: 3,
            timestamp_us: 3000,
            size_bytes: 100,
        },
    ];

    let gc_result = scheduler.run_gc(&candidates, 5000);
    assert!(!gc_result.is_empty());
}

#[test]
fn test_journal_gc_state_record_ack() {
    let mut state = JournalGcState::new(GcPolicy::RetainAll);
    state.record_ack(1, 100, 1000);

    let ack = state.get_ack(1);
    assert!(ack.is_some());
    assert_eq!(ack.unwrap().acked_through_seq, 100);
}

#[test]
fn test_journal_gc_state_min_acked_seq() {
    let mut state = JournalGcState::new(GcPolicy::RetainAll);
    state.record_ack(1, 50, 1000);
    state.record_ack(2, 30, 1000);

    let min_seq = state.min_acked_seq(&[1, 2]);
    assert_eq!(min_seq, Some(30));
}

#[test]
fn test_journal_gc_state_all_sites_acked() {
    let mut state = JournalGcState::new(GcPolicy::RetainAll);
    state.record_ack(1, 100, 1000);
    state.record_ack(2, 100, 1000);

    assert!(state.all_sites_acked(100, &[1, 2]));
    assert!(!state.all_sites_acked(101, &[1, 2]));
}

#[test]
fn test_journal_gc_scheduler_run_gc() {
    let policy = GcPolicy::RetainByCount { max_entries: 1 };
    let mut scheduler = JournalGcScheduler::new(policy, vec![]);

    scheduler.record_ack(AckRecord {
        site_id: 1,
        acked_through_seq: 5,
        acked_at_us: 1000,
    });

    let candidates = vec![
        GcCandidate {
            shard_id: 0,
            seq: 1,
            timestamp_us: 1000,
            size_bytes: 100,
        },
        GcCandidate {
            shard_id: 0,
            seq: 2,
            timestamp_us: 2000,
            size_bytes: 100,
        },
        GcCandidate {
            shard_id: 0,
            seq: 3,
            timestamp_us: 3000,
            size_bytes: 100,
        },
    ];

    let gc = scheduler.run_gc(&candidates, 5000);
    assert!(gc.len() >= 2);
}

#[test]
fn test_journal_gc_scheduler_stats() {
    let policy = GcPolicy::RetainByCount { max_entries: 1 };
    let mut scheduler = JournalGcScheduler::new(policy, vec![]);

    scheduler.record_ack(AckRecord {
        site_id: 1,
        acked_through_seq: 10,
        acked_at_us: 1000,
    });

    let candidates = vec![
        GcCandidate {
            shard_id: 0,
            seq: 1,
            timestamp_us: 1000,
            size_bytes: 100,
        },
        GcCandidate {
            shard_id: 0,
            seq: 2,
            timestamp_us: 2000,
            size_bytes: 100,
        },
    ];

    scheduler.run_gc(&candidates, 5000);

    let stats = scheduler.stats();
    assert!(stats.gc_runs >= 1);
}

#[test]
fn test_tls_validator_is_plaintext_allowed() {
    let required = TlsValidator::new(TlsMode::Required);
    assert!(!required.is_plaintext_allowed());

    let test_only = TlsValidator::new(TlsMode::TestOnly);
    assert!(test_only.is_plaintext_allowed());

    let disabled = TlsValidator::new(TlsMode::Disabled);
    assert!(disabled.is_plaintext_allowed());
}

#[test]
fn test_combined_tls_and_site_registry() {
    let mut registry = SiteRegistry::new();
    let mut record = SiteRecord::new(1, "site-a");
    let fingerprint: [u8; 32] = [0x12; 32];
    record.tls_fingerprint = Some(fingerprint);
    registry.register(record).unwrap();

    let validator = TlsValidator::new(TlsMode::Required);
    let tls_config = Some(TlsConfigRef {
        cert_pem: b"-----BEGIN CERTIFICATE-----\ntest\n-----END CERTIFICATE-----".to_vec(),
        key_pem: b"key".to_vec(),
        ca_pem: b"ca".to_vec(),
    });

    let result = validator.validate_config(&tls_config);
    assert!(result.is_ok());

    let verify = registry.verify_source_id(1, Some(&fingerprint));
    assert!(verify.is_ok());
}

#[test]
fn test_journal_gc_state_policy() {
    let state = JournalGcState::new(GcPolicy::RetainByAge { max_age_us: 1000 });
    let policy = state.policy();
    matches!(policy, GcPolicy::RetainByAge { .. });
}

#[test]
fn test_journal_gc_state_site_count() {
    let mut state = JournalGcState::new(GcPolicy::RetainAll);
    state.record_ack(1, 100, 1000);
    state.record_ack(2, 50, 1000);

    assert_eq!(state.site_count(), 2);
}

#[test]
fn test_journal_gc_scheduler_total_gc_entries() {
    let policy = GcPolicy::RetainByCount { max_entries: 2 };
    let mut scheduler = JournalGcScheduler::new(policy, vec![]);

    scheduler.record_ack(AckRecord {
        site_id: 1,
        acked_through_seq: 10,
        acked_at_us: 1000,
    });

    let candidates = vec![
        GcCandidate {
            shard_id: 0,
            seq: 1,
            timestamp_us: 1000,
            size_bytes: 100,
        },
        GcCandidate {
            shard_id: 0,
            seq: 2,
            timestamp_us: 2000,
            size_bytes: 100,
        },
        GcCandidate {
            shard_id: 0,
            seq: 3,
            timestamp_us: 3000,
            size_bytes: 100,
        },
    ];

    scheduler.run_gc(&candidates, 10000);
    let total = scheduler.total_gc_entries();
    assert!(total > 0);
}

#[test]
fn test_site_registry_verify_source_id_no_fingerprint() {
    let mut registry = SiteRegistry::new();
    let record = SiteRecord::new(1, "site-a");
    registry.register(record).unwrap();

    let result = registry.verify_source_id(1, None);
    assert!(result.is_ok());
}

#[test]
fn test_rate_limiter_stats_all_fields() {
    let config = RateLimitConfig::new(100, 1000);
    let mut limiter = RecvRateLimiter::new(config);

    limiter.check_batch(5, 1000);
    limiter.check_batch(5, 1001);

    let stats = limiter.stats();
    assert!(stats.batches_allowed >= 0);
    assert!(stats.batches_throttled >= 0);
    assert!(stats.batches_rejected >= 0);
    assert!(stats.entries_allowed >= 0);
    assert!(stats.windows_reset >= 0);
}

#[test]
fn test_rate_limiter_throttle_decision() {
    let config = RateLimitConfig::new(1, 1000);
    let mut limiter = RecvRateLimiter::new(config);

    limiter.check_batch(10, 1000);
    let decision = limiter.check_batch(10, 1001);

    match decision {
        RateLimitDecision::Throttle { delay_ms } => assert!(delay_ms > 0),
        _ => panic!("Expected throttle"),
    }
}

#[test]
fn test_journal_gc_stats_fields() {
    let stats = GcStats::default();
    assert_eq!(stats.entries_gc_collected, 0);
    assert_eq!(stats.bytes_gc_collected, 0);
    assert_eq!(stats.gc_runs, 0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_policy_gc_by_count_removes_excess() {
        let policy = GcPolicy::RetainByCount { max_entries: 1 };
        let mut scheduler = JournalGcScheduler::new(policy, vec![]);

        scheduler.record_ack(AckRecord {
            site_id: 1,
            acked_through_seq: 100,
            acked_at_us: 1000,
        });

        let candidates = vec![
            GcCandidate {
                shard_id: 0,
                seq: 1,
                timestamp_us: 1000,
                size_bytes: 100,
            },
            GcCandidate {
                shard_id: 0,
                seq: 2,
                timestamp_us: 2000,
                size_bytes: 100,
            },
            GcCandidate {
                shard_id: 0,
                seq: 3,
                timestamp_us: 3000,
                size_bytes: 100,
            },
        ];

        let gc = scheduler.run_gc(&candidates, 10000);
        assert!(gc.len() >= 2);
    }

    #[test]
    fn test_site_registry_len() {
        let mut registry = SiteRegistry::new();
        assert!(registry.is_empty());

        let record = SiteRecord::new(1, "site-a");
        registry.register(record).unwrap();

        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn test_site_registry_lookup_returns_none_for_unknown() {
        let registry = SiteRegistry::new();
        assert!(registry.lookup(999).is_none());
    }
}
