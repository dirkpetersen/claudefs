//! Replication Phase 2 integration tests
//! 
//! Tests for Phase 2 modules: journal, batch_auth, active_active, failover

use claudefs_repl::journal::{JournalEntry, OpKind};
use claudefs_repl::batch_auth::{AuthResult, BatchAuthKey, BatchTag, BatchAuthenticator};
use claudefs_repl::active_active::{
    ActiveActiveController, ActiveActiveStats, ForwardedWrite, LinkStatus, SiteRole, WriteConflict,
};
use claudefs_repl::failover::{
    FailoverConfig, FailoverEvent, FailoverManager, SiteFailoverState, SiteMode,
};
use proptest::prelude::*;

fn make_journal_entry(seq: u64, op: OpKind, payload: Vec<u8>) -> JournalEntry {
    JournalEntry::new(seq, 0, 1, 1_000_000, 42, op, payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // Section 1: JournalEntry (10 tests)
    // =========================================================================

    #[test]
    fn test_journal_entry_new_sets_crc() {
        let entry = make_journal_entry(1, OpKind::Create, vec![1, 2, 3]);
        assert_ne!(entry.crc32, 0, "CRC should be computed on creation");
    }

    #[test]
    fn test_journal_entry_validate_crc_true() {
        let entry = make_journal_entry(1, OpKind::Create, vec![1, 2, 3]);
        assert!(entry.validate_crc(), "New entry should validate successfully");
    }

    #[test]
    fn test_journal_entry_validate_crc_false_after_tamper() {
        let entry = make_journal_entry(1, OpKind::Create, vec![1, 2, 3]);
        let mut tampered = entry.clone();
        tampered.payload.push(99);
        assert!(
            !tampered.validate_crc(),
            "Tampered entry should fail CRC validation"
        );
    }

    #[test]
    fn test_journal_entry_all_op_kinds() {
        let op_kinds = vec![
            OpKind::Create,
            OpKind::Unlink,
            OpKind::Rename,
            OpKind::Write,
            OpKind::Truncate,
            OpKind::SetAttr,
            OpKind::Link,
            OpKind::Symlink,
            OpKind::MkDir,
            OpKind::SetXattr,
            OpKind::RemoveXattr,
        ];
        for (i, op) in op_kinds.into_iter().enumerate() {
            let entry = make_journal_entry(i as u64, op, vec![]);
            assert_eq!(entry.op, op);
            assert!(entry.validate_crc());
        }
    }

    #[test]
    fn test_journal_entry_zero_payload() {
        let entry = make_journal_entry(1, OpKind::Create, vec![]);
        assert!(entry.validate_crc());
        assert!(entry.payload.is_empty());
    }

    #[test]
    fn test_journal_entry_serde_roundtrip() {
        let entry = make_journal_entry(42, OpKind::Write, vec![1, 2, 3, 4, 5]);
        let serialized = serde_json::to_string(&entry).unwrap();
        let deserialized: JournalEntry = serde_json::from_str(&serialized).unwrap();
        assert_eq!(entry.seq, deserialized.seq);
        assert_eq!(entry.op, deserialized.op);
        assert_eq!(entry.payload, deserialized.payload);
        assert_eq!(entry.crc32, deserialized.crc32);
    }

    #[test]
    fn test_op_kind_serde() {
        let op_kinds = vec![
            OpKind::Create,
            OpKind::Unlink,
            OpKind::Rename,
            OpKind::Write,
            OpKind::Truncate,
            OpKind::SetAttr,
            OpKind::Link,
            OpKind::Symlink,
            OpKind::MkDir,
            OpKind::SetXattr,
            OpKind::RemoveXattr,
        ];
        for op in op_kinds {
            let serialized = serde_json::to_string(&op).unwrap();
            let deserialized: OpKind = serde_json::from_str(&serialized).unwrap();
            assert_eq!(op, deserialized);
        }
    }

    #[test]
    fn test_journal_entry_compute_crc_deterministic() {
        let entry1 = make_journal_entry(1, OpKind::Create, vec![1, 2, 3]);
        let entry2 = make_journal_entry(1, OpKind::Create, vec![1, 2, 3]);
        assert_eq!(entry1.crc32, entry2.crc32, "Same entry should have same CRC");
    }

    #[test]
    fn test_journal_entry_crc_changes_with_payload() {
        let entry1 = make_journal_entry(1, OpKind::Create, vec![1, 2, 3]);
        let entry2 = make_journal_entry(1, OpKind::Create, vec![1, 2, 4]);
        assert_ne!(entry1.crc32, entry2.crc32, "Different payload should have different CRC");
    }

    // =========================================================================
    // Section 2: BatchAuthentication (15 tests)
    // =========================================================================

    #[test]
    fn test_batch_key_generate_is_32_bytes() {
        let key = BatchAuthKey::generate();
        assert_eq!(key.as_bytes().len(), 32);
    }

    #[test]
    fn test_batch_key_from_bytes() {
        let bytes = [0x42; 32];
        let key = BatchAuthKey::from_bytes(bytes);
        assert_eq!(*key.as_bytes(), bytes);
    }

    #[test]
    fn test_batch_key_two_generates_differ() {
        let key1 = BatchAuthKey::generate();
        let key2 = BatchAuthKey::generate();
        assert_ne!(
            *key1.as_bytes(),
            *key2.as_bytes(),
            "Two generated keys should differ with high probability"
        );
    }

    #[test]
    fn test_batch_tag_zero() {
        let tag = BatchTag::zero();
        assert_eq!(tag.bytes, [0u8; 32]);
    }

    #[test]
    fn test_batch_tag_new() {
        let bytes = [0xAB; 32];
        let tag = BatchTag::new(bytes);
        assert_eq!(tag.bytes, bytes);
    }

    #[test]
    fn test_batch_tag_equality() {
        let tag1 = BatchTag::new([0x11; 32]);
        let tag2 = BatchTag::new([0x11; 32]);
        let tag3 = BatchTag::new([0x22; 32]);
        assert_eq!(tag1, tag2);
        assert_ne!(tag1, tag3);
    }

    #[test]
    fn test_authenticator_local_site_id() {
        let key = BatchAuthKey::from_bytes([0xaa; 32]);
        let auth = BatchAuthenticator::new(key, 42);
        assert_eq!(auth.local_site_id(), 42);
    }

    #[test]
    fn test_sign_batch_empty_entries() {
        let key = BatchAuthKey::from_bytes([0xaa; 32]);
        let auth = BatchAuthenticator::new(key, 1);
        let entries: Vec<JournalEntry> = vec![];
        let tag = auth.sign_batch(1, 1, &entries);
        assert_eq!(tag.bytes.len(), 32);
    }

    #[test]
    fn test_sign_batch_nonempty_entries() {
        let key = BatchAuthKey::from_bytes([0xaa; 32]);
        let auth = BatchAuthenticator::new(key, 1);
        let entries = vec![
            make_journal_entry(1, OpKind::Create, vec![1, 2]),
            make_journal_entry(2, OpKind::Write, vec![3, 4]),
        ];
        let tag = auth.sign_batch(1, 1, &entries);
        assert_eq!(tag.bytes.len(), 32);
    }

    #[test]
    fn test_verify_batch_valid() {
        let key = BatchAuthKey::from_bytes([0xaa; 32]);
        let auth = BatchAuthenticator::new(key, 1);
        let entries = vec![make_journal_entry(1, OpKind::Create, vec![1, 2, 3])];
        let tag = auth.sign_batch(1, 1, &entries);
        let result = auth.verify_batch(&tag, 1, 1, &entries);
        match result {
            AuthResult::Valid => (),
            _ => panic!("Expected Valid"),
        }
    }

    #[test]
    fn test_verify_batch_wrong_key() {
        let key1 = BatchAuthKey::from_bytes([0xaa; 32]);
        let key2 = BatchAuthKey::from_bytes([0xbb; 32]);
        let auth1 = BatchAuthenticator::new(key1, 1);
        let auth2 = BatchAuthenticator::new(key2, 1);
        let entries = vec![make_journal_entry(1, OpKind::Create, vec![1, 2, 3])];
        let tag = auth1.sign_batch(1, 1, &entries);
        let result = auth2.verify_batch(&tag, 1, 1, &entries);
        match result {
            AuthResult::Invalid { .. } => (),
            _ => panic!("Expected Invalid"),
        }
    }

    #[test]
    fn test_verify_batch_tampered_payload() {
        let key = BatchAuthKey::from_bytes([0xaa; 32]);
        let auth = BatchAuthenticator::new(key, 1);
        let entries = vec![make_journal_entry(1, OpKind::Create, vec![1, 2, 3])];
        let tag = auth.sign_batch(1, 1, &entries);
        let tampered_entries = vec![make_journal_entry(1, OpKind::Create, vec![1, 2, 99])];
        let result = auth.verify_batch(&tag, 1, 1, &tampered_entries);
        match result {
            AuthResult::Invalid { .. } => (),
            _ => panic!("Expected Invalid"),
        }
    }

    #[test]
    fn test_verify_batch_wrong_source_site() {
        let key = BatchAuthKey::from_bytes([0xaa; 32]);
        let auth = BatchAuthenticator::new(key, 1);
        let entries = vec![make_journal_entry(1, OpKind::Create, vec![1, 2, 3])];
        let tag = auth.sign_batch(1, 1, &entries);
        let result = auth.verify_batch(&tag, 2, 1, &entries);
        match result {
            AuthResult::Invalid { .. } => (),
            _ => panic!("Expected Invalid"),
        }
    }

    #[test]
    fn test_verify_batch_wrong_seq() {
        let key = BatchAuthKey::from_bytes([0xaa; 32]);
        let auth = BatchAuthenticator::new(key, 1);
        let entries = vec![make_journal_entry(1, OpKind::Create, vec![1, 2, 3])];
        let tag = auth.sign_batch(1, 1, &entries);
        let result = auth.verify_batch(&tag, 1, 2, &entries);
        match result {
            AuthResult::Invalid { .. } => (),
            _ => panic!("Expected Invalid"),
        }
    }

    // =========================================================================
    // Section 3: ActiveActiveController (15 tests)
    // =========================================================================

    #[test]
    fn test_controller_new_initial_state() {
        let controller = ActiveActiveController::new("site-a".to_string(), SiteRole::Primary);
        assert_eq!(controller.site_id, "site-a");
        assert_eq!(controller.role, SiteRole::Primary);
        assert_eq!(controller.link_status, LinkStatus::Down);
    }

    #[test]
    fn test_controller_link_starts_down() {
        let controller = ActiveActiveController::new("site-a".to_string(), SiteRole::Primary);
        assert_eq!(controller.link_status, LinkStatus::Down);
    }

    #[test]
    fn test_local_write_increments_logical_time() {
        let mut controller = ActiveActiveController::new("site-a".to_string(), SiteRole::Primary);
        assert_eq!(controller.stats().writes_forwarded, 0);
        
        controller.local_write(b"key".to_vec(), b"value".to_vec());
        assert_eq!(controller.stats().writes_forwarded, 1);
        
        controller.local_write(b"key2".to_vec(), b"value2".to_vec());
        assert_eq!(controller.stats().writes_forwarded, 2);
    }

    #[test]
    fn test_local_write_returns_forwarded_write() {
        let mut controller = ActiveActiveController::new("site-a".to_string(), SiteRole::Primary);
        let fw = controller.local_write(b"testkey".to_vec(), b"testvalue".to_vec());
        assert_eq!(fw.key, b"testkey");
        assert_eq!(fw.value, b"testvalue");
        assert_eq!(fw.origin_site_id, "site-a");
    }

    #[test]
    fn test_local_write_forwards_origin_site_id() {
        let mut controller = ActiveActiveController::new("site-a".to_string(), SiteRole::Primary);
        let fw = controller.local_write(b"key".to_vec(), b"value".to_vec());
        assert_eq!(fw.origin_site_id, "site-a");
    }

    #[test]
    fn test_stats_writes_forwarded_after_write() {
        let mut controller = ActiveActiveController::new("site-a".to_string(), SiteRole::Primary);
        assert_eq!(controller.stats().writes_forwarded, 0);
        controller.local_write(b"key".to_vec(), b"value".to_vec());
        assert_eq!(controller.stats().writes_forwarded, 1);
    }

    #[test]
    fn test_drain_pending_clears_queue() {
        let mut controller = ActiveActiveController::new("site-a".to_string(), SiteRole::Primary);
        controller.local_write(b"key1".to_vec(), b"value1".to_vec());
        controller.local_write(b"key2".to_vec(), b"value2".to_vec());
        
        let drained = controller.drain_pending();
        assert_eq!(drained.len(), 2);
        
        let drained_again = controller.drain_pending();
        assert!(drained_again.is_empty(), "Queue should be cleared after drain");
    }

    #[test]
    fn test_drain_pending_empty_initially() {
        let mut controller = ActiveActiveController::new("site-a".to_string(), SiteRole::Primary);
        let drained = controller.drain_pending();
        assert!(drained.is_empty(), "Initially no pending writes");
    }

    #[test]
    fn test_apply_remote_write_no_conflict() {
        let mut controller = ActiveActiveController::new("site-a".to_string(), SiteRole::Primary);
        controller.local_write(b"key".to_vec(), b"value".to_vec());
        
        let fw = ForwardedWrite {
            origin_site_id: "site-b".to_string(),
            logical_time: 100,
            key: b"key2".to_vec(),
            value: b"value2".to_vec(),
        };
        let conflict = controller.apply_remote_write(fw);
        assert!(conflict.is_none(), "No conflict when remote time > local time");
    }

    #[test]
    fn test_apply_remote_write_conflict_same_timestamp() {
        let mut controller = ActiveActiveController::new("site-a".to_string(), SiteRole::Primary);
        controller.local_write(b"key".to_vec(), b"value".to_vec());
        
        let fw = ForwardedWrite {
            origin_site_id: "site-b".to_string(),
            logical_time: 1,
            key: b"key".to_vec(),
            value: b"value".to_vec(),
        };
        let conflict = controller.apply_remote_write(fw);
        assert!(conflict.is_some(), "Conflict when timestamps match");
    }

    #[test]
    fn test_conflict_winner_primary_site_id_lower() {
        let mut controller = ActiveActiveController::new("site-b".to_string(), SiteRole::Secondary);
        controller.local_write(b"key".to_vec(), b"value".to_vec());
        
        let fw = ForwardedWrite {
            origin_site_id: "site-a".to_string(),
            logical_time: 1,
            key: b"key".to_vec(),
            value: b"value".to_vec(),
        };
        let conflict = controller.apply_remote_write(fw);
        assert!(conflict.is_some());
        assert_eq!(conflict.unwrap().winner, SiteRole::Primary);
    }

    #[test]
    fn test_set_link_status_up_increments_flaps() {
        let mut controller = ActiveActiveController::new("site-a".to_string(), SiteRole::Primary);
        assert_eq!(controller.stats().link_flaps, 0);
        
        controller.set_link_status(LinkStatus::Up);
        assert_eq!(controller.stats().link_flaps, 1, "First Up should increment flaps");
        
        controller.set_link_status(LinkStatus::Down);
        assert_eq!(controller.stats().link_flaps, 1, "Down does not increment flaps");
        
        controller.set_link_status(LinkStatus::Up);
        assert_eq!(controller.stats().link_flaps, 2, "Second Up should increment flaps");
    }

    #[test]
    fn test_set_link_status_down_no_flap() {
        let mut controller = ActiveActiveController::new("site-a".to_string(), SiteRole::Primary);
        controller.set_link_status(LinkStatus::Down);
        assert_eq!(controller.stats().link_flaps, 0);
        
        controller.set_link_status(LinkStatus::Degraded);
        assert_eq!(controller.stats().link_flaps, 0);
    }

    #[test]
    fn test_stats_conflicts_resolved_after_conflict() {
        let mut controller = ActiveActiveController::new("site-a".to_string(), SiteRole::Primary);
        controller.local_write(b"key".to_vec(), b"value".to_vec());
        
        let fw = ForwardedWrite {
            origin_site_id: "site-b".to_string(),
            logical_time: 1,
            key: b"key".to_vec(),
            value: b"value".to_vec(),
        };
        controller.apply_remote_write(fw);
        
        assert_eq!(controller.stats().conflicts_resolved, 1);
    }

    #[test]
    fn test_forwarded_write_serde() {
        let fw = ForwardedWrite {
            origin_site_id: "site-a".to_string(),
            logical_time: 42,
            key: b"key1".to_vec(),
            value: b"value1".to_vec(),
        };
        let serialized = serde_json::to_string(&fw).unwrap();
        let deserialized: ForwardedWrite = serde_json::from_str(&serialized).unwrap();
        assert_eq!(fw.origin_site_id, deserialized.origin_site_id);
        assert_eq!(fw.logical_time, deserialized.logical_time);
        assert_eq!(fw.key, deserialized.key);
        assert_eq!(fw.value, deserialized.value);
    }

    // =========================================================================
    // Section 4: FailoverManager (15 tests)
    // =========================================================================

    #[test]
    fn test_failover_config_defaults() {
        let config = FailoverConfig::default();
        assert_eq!(config.failure_threshold, 3);
        assert_eq!(config.recovery_threshold, 2);
        assert_eq!(config.check_interval_ms, 5000);
        assert!(config.active_active);
    }

    #[test]
    fn test_site_failover_state_new() {
        let state = SiteFailoverState::new(42);
        assert_eq!(state.site_id, 42);
        assert_eq!(state.mode, SiteMode::ActiveReadWrite);
        assert_eq!(state.consecutive_failures, 0);
        assert_eq!(state.consecutive_successes, 0);
    }

    #[test]
    fn test_site_readable_active() {
        let state = SiteFailoverState::new(1);
        assert!(state.is_readable(), "Active site should be readable");
    }

    #[test]
    fn test_site_writable_active() {
        let state = SiteFailoverState::new(1);
        assert!(state.is_writable(), "Active site should be writable");
    }

    #[test]
    fn test_site_offline_not_readable() {
        let state = SiteFailoverState {
            site_id: 1,
            mode: SiteMode::Offline,
            consecutive_failures: 0,
            consecutive_successes: 0,
            last_check_us: 0,
            failover_count: 0,
        };
        assert!(!state.is_readable(), "Offline site should not be readable");
    }

    #[test]
    fn test_site_offline_not_writable() {
        let state = SiteFailoverState {
            site_id: 1,
            mode: SiteMode::Offline,
            consecutive_failures: 0,
            consecutive_successes: 0,
            last_check_us: 0,
            failover_count: 0,
        };
        assert!(!state.is_writable(), "Offline site should not be writable");
    }

    #[tokio::test]
    async fn test_failover_manager_new() {
        let config = FailoverConfig::default();
        let manager = FailoverManager::new(config, 1);
        let modes = manager.writable_sites().await;
        assert!(modes.is_empty(), "New manager should have no sites");
    }

    #[tokio::test]
    async fn test_register_site() {
        let config = FailoverConfig::default();
        let manager = FailoverManager::new(config, 1);
        manager.register_site(100).await;
        let mode = manager.site_mode(100).await;
        assert_eq!(mode, Some(SiteMode::ActiveReadWrite));
    }

    #[tokio::test]
    async fn test_record_health_healthy_no_transition() {
        let config = FailoverConfig::default();
        let manager = FailoverManager::new(config, 1);
        manager.register_site(100).await;
        
        let events = manager.record_health(100, true).await;
        assert!(events.is_empty(), "Single success should not trigger transition");
        
        let mode = manager.site_mode(100).await;
        assert_eq!(mode, Some(SiteMode::ActiveReadWrite));
    }

    #[tokio::test]
    async fn test_record_health_failures_trigger_demote() {
        let config = FailoverConfig {
            failure_threshold: 3,
            ..Default::default()
        };
        let manager = FailoverManager::new(config, 1);
        manager.register_site(100).await;
        
        manager.record_health(100, false).await;
        manager.record_health(100, false).await;
        let events = manager.record_health(100, false).await;
        
        assert!(!events.is_empty(), "3 failures should trigger demotion");
        if let FailoverEvent::SiteDemoted { new_mode, .. } = &events[0] {
            assert_eq!(new_mode, &SiteMode::DegradedAcceptWrites);
        } else {
            panic!("Expected SiteDemoted");
        }
    }

    #[tokio::test]
    async fn test_record_health_recovery_after_demotion() {
        let config = FailoverConfig {
            failure_threshold: 2,
            recovery_threshold: 2,
            ..Default::default()
        };
        let manager = FailoverManager::new(config, 1);
        manager.register_site(100).await;
        
        manager.record_health(100, false).await;
        manager.record_health(100, false).await;
        assert_eq!(manager.site_mode(100).await, Some(SiteMode::DegradedAcceptWrites));
        
        manager.record_health(100, false).await;
        assert_eq!(manager.site_mode(100).await, Some(SiteMode::Offline));
        
        let _ = manager.record_health(100, true).await;
        let _ = manager.record_health(100, true).await;
        let mode = manager.site_mode(100).await;
        assert_eq!(mode, Some(SiteMode::StandbyReadOnly), "Should recover to standby");
    }

    #[test]
    fn test_site_mode_default_is_active() {
        let mode: SiteMode = Default::default();
        assert_eq!(mode, SiteMode::ActiveReadWrite);
    }

    #[test]
    fn test_failover_event_variants() {
        let event1 = FailoverEvent::SitePromoted {
            site_id: 1,
            new_mode: SiteMode::ActiveReadWrite,
        };
        let event2 = FailoverEvent::SiteDemoted {
            site_id: 1,
            new_mode: SiteMode::Offline,
            reason: "test".to_string(),
        };
        let event3 = FailoverEvent::SiteRecovered { site_id: 1 };
        let event4 = FailoverEvent::ConflictRequiresResolution {
            site_id: 1,
            inode: 100,
        };
        
        format!("{:?}", event1);
        format!("{:?}", event2);
        format!("{:?}", event3);
        format!("{:?}", event4);
    }

    #[tokio::test]
    async fn test_unregistered_site_auto_registers() {
        let config = FailoverConfig::default();
        let manager = FailoverManager::new(config, 1);
        
        let events = manager.record_health(999, true).await;
        assert!(events.is_empty(), "Unknown site should auto-register");
        
        let mode = manager.site_mode(999).await;
        assert_eq!(mode, Some(SiteMode::ActiveReadWrite));
    }

    #[test]
    fn test_standby_not_writable() {
        let state = SiteFailoverState {
            site_id: 1,
            mode: SiteMode::StandbyReadOnly,
            consecutive_failures: 0,
            consecutive_successes: 0,
            last_check_us: 0,
            failover_count: 0,
        };
        assert!(!state.is_writable(), "Standby should not be writable");
        assert!(state.is_readable(), "Standby should be readable");
    }

    #[test]
    fn test_degraded_writable() {
        let state = SiteFailoverState {
            site_id: 1,
            mode: SiteMode::DegradedAcceptWrites,
            consecutive_failures: 0,
            consecutive_successes: 0,
            last_check_us: 0,
            failover_count: 0,
        };
        assert!(state.is_writable(), "Degraded should be writable");
        assert!(state.is_readable(), "Degraded should be readable");
    }

    #[test]
    fn test_site_failover_state_clone() {
        let state = SiteFailoverState::new(42);
        let cloned = state.clone();
        assert_eq!(state.site_id, cloned.site_id);
        assert_eq!(state.mode, cloned.mode);
    }

    #[test]
    fn test_failover_config_clone() {
        let config = FailoverConfig::default();
        let cloned = config.clone();
        assert_eq!(config.failure_threshold, cloned.failure_threshold);
    }

    #[test]
    fn test_link_status_serde_roundtrip() {
        let statuses = vec![LinkStatus::Up, LinkStatus::Degraded, LinkStatus::Down];
        for status in statuses {
            let serialized = serde_json::to_string(&status).unwrap();
            let deserialized: LinkStatus = serde_json::from_str(&serialized).unwrap();
            assert_eq!(status, deserialized);
        }
    }

    #[test]
    fn test_site_role_serde_roundtrip() {
        let roles = vec![SiteRole::Primary, SiteRole::Secondary];
        for role in roles {
            let serialized = serde_json::to_string(&role).unwrap();
            let deserialized: SiteRole = serde_json::from_str(&serialized).unwrap();
            assert_eq!(role, deserialized);
        }
    }

    #[test]
    fn test_active_active_stats_default() {
        let stats = ActiveActiveStats::default();
        assert_eq!(stats.writes_forwarded, 0);
        assert_eq!(stats.conflicts_resolved, 0);
        assert_eq!(stats.link_flaps, 0);
    }

    #[test]
    fn test_write_conflict_serde_roundtrip() {
        let conflict = WriteConflict {
            key: b"testkey".to_vec(),
            local_time: 10,
            remote_time: 20,
            winner: SiteRole::Primary,
        };
        let serialized = serde_json::to_string(&conflict).unwrap();
        let deserialized: WriteConflict = serde_json::from_str(&serialized).unwrap();
        assert_eq!(conflict.key, deserialized.key);
        assert_eq!(conflict.local_time, deserialized.local_time);
        assert_eq!(conflict.remote_time, deserialized.remote_time);
    }

    #[tokio::test]
    async fn test_failover_manager_multiple_sites() {
        let config = FailoverConfig::default();
        let manager = FailoverManager::new(config, 1);
        
        manager.register_site(1).await;
        manager.register_site(2).await;
        manager.register_site(3).await;
        
        let writable = manager.writable_sites().await;
        assert_eq!(writable.len(), 3);
    }

    #[tokio::test]
    async fn test_force_mode_generates_event() {
        let config = FailoverConfig::default();
        let manager = FailoverManager::new(config, 1);
        manager.register_site(100).await;
        
        manager.force_mode(100, SiteMode::Offline).await.unwrap();
        
        let events = manager.drain_events().await;
        assert!(!events.is_empty(), "Force mode should generate event");
    }

    #[tokio::test]
    async fn test_drain_events_clears() {
        let config = FailoverConfig::default();
        let manager = FailoverManager::new(config, 1);
        manager.register_site(100).await;
        
        manager.record_health(100, false).await;
        let _ = manager.drain_events().await;
        
        let events = manager.drain_events().await;
        assert!(events.is_empty(), "Second drain should be empty");
    }

    #[tokio::test]
    async fn test_failover_counts() {
        let config = FailoverConfig::default();
        let manager = FailoverManager::new(config, 1);
        manager.register_site(100).await;
        
        manager.force_mode(100, SiteMode::Offline).await.unwrap();
        manager.force_mode(100, SiteMode::StandbyReadOnly).await.unwrap();
        
        let counts = manager.failover_counts().await;
        assert_eq!(counts[&100], 2);
    }

    #[tokio::test]
    async fn test_all_states() {
        let config = FailoverConfig::default();
        let manager = FailoverManager::new(config, 1);
        manager.register_site(100).await;
        manager.register_site(200).await;
        
        let states = manager.all_states().await;
        assert_eq!(states.len(), 2);
    }

    #[tokio::test]
    async fn test_readable_sites() {
        let config = FailoverConfig::default();
        let manager = FailoverManager::new(config, 1);
        manager.register_site(100).await;
        manager.register_site(200).await;
        
        manager.force_mode(200, SiteMode::Offline).await.unwrap();
        
        let readable = manager.readable_sites().await;
        assert_eq!(readable, vec![100]);
    }

    #[tokio::test]
    async fn test_force_mode_unknown_site_fails() {
        let config = FailoverConfig::default();
        let manager = FailoverManager::new(config, 1);
        
        let result = manager.force_mode(999, SiteMode::Offline).await;
        assert!(result.is_err(), "Force mode on unknown site should fail");
    }

    #[test]
    fn test_batch_tag_serde_roundtrip() {
        let tag = BatchTag::new([0xAB; 32]);
        let serialized = serde_json::to_string(&tag).unwrap();
        let deserialized: BatchTag = serde_json::from_str(&serialized).unwrap();
        assert_eq!(tag, deserialized);
    }

    #[test]
    fn test_batch_key_different_keys_different_hmac() {
        let key1 = BatchAuthKey::from_bytes([0xAA; 32]);
        let key2 = BatchAuthKey::from_bytes([0xBB; 32]);
        let auth1 = BatchAuthenticator::new(key1, 1);
        let auth2 = BatchAuthenticator::new(key2, 1);
        
        let entries = vec![make_journal_entry(1, OpKind::Create, vec![1, 2, 3])];
        let tag1 = auth1.sign_batch(1, 1, &entries);
        let tag2 = auth2.sign_batch(1, 1, &entries);
        
        assert_ne!(tag1.bytes, tag2.bytes, "Different keys should produce different tags");
    }

    #[test]
    fn test_auth_result_variants() {
        let valid = AuthResult::Valid;
        let invalid = AuthResult::Invalid { reason: "test".to_string() };
        
        format!("{:?}", valid);
        format!("{:?}", invalid);
    }

    #[test]
    fn test_active_active_controller_debug() {
        let controller = ActiveActiveController::new("site-a".to_string(), SiteRole::Primary);
        let debug_str = format!("{:?}", controller);
        assert!(debug_str.contains("site-a"));
    }

    #[test]
    fn test_journal_entry_large_payload() {
        let payload: Vec<u8> = (0..10000).map(|i| (i % 256) as u8).collect();
        let entry = make_journal_entry(1, OpKind::Write, payload.clone());
        assert!(entry.validate_crc());
        
        let serialized = serde_json::to_string(&entry).unwrap();
        let deserialized: JournalEntry = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized.payload, payload);
    }
}

#[cfg(test)]
mod proptest_tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn prop_journal_entry_crc_roundtrip(
            seq in 0u64..1000u64,
            op in prop_oneof![
                Just(OpKind::Create),
                Just(OpKind::Unlink),
                Just(OpKind::Write),
                Just(OpKind::Truncate),
            ],
            payload in prop::collection::vec(0u8..=255, 0..512),
        ) {
            let entry = make_journal_entry(seq, op, payload);
            prop_assert!(entry.validate_crc(), "CRC should validate for random payload");
        }

        #[test]
        fn prop_batch_auth_roundtrip(
            seq in 0u64..100u64,
            payload in prop::collection::vec(0u8..=255, 0..256),
        ) {
            let key = BatchAuthKey::from_bytes([0xAA; 32]);
            let auth = BatchAuthenticator::new(key, 1);
            let entries = vec![make_journal_entry(seq, OpKind::Create, payload)];
            let tag = auth.sign_batch(1, 1, &entries);
            let result = auth.verify_batch(&tag, 1, 1, &entries);
            match result {
                AuthResult::Valid => prop_assert!(true),
                AuthResult::Invalid { reason } => prop_assert!(false, "Verification failed: {}", reason),
            }
        }

        #[test]
        fn prop_failover_mode_writable(
            mode in prop_oneof![
                Just(SiteMode::ActiveReadWrite),
                Just(SiteMode::DegradedAcceptWrites),
                Just(SiteMode::StandbyReadOnly),
                Just(SiteMode::Offline),
            ],
        ) {
            let state = SiteFailoverState {
                site_id: 1,
                mode,
                consecutive_failures: 0,
                consecutive_successes: 0,
                last_check_us: 0,
                failover_count: 0,
            };
            let is_writable = matches!(
                state.mode,
                SiteMode::ActiveReadWrite | SiteMode::DegradedAcceptWrites
            );
            prop_assert_eq!(state.is_writable(), is_writable);
        }

        #[test]
        fn prop_op_kind_serde_roundtrip(
            op in prop_oneof![
                Just(OpKind::Create),
                Just(OpKind::Unlink),
                Just(OpKind::Rename),
                Just(OpKind::Write),
                Just(OpKind::Truncate),
                Just(OpKind::SetAttr),
                Just(OpKind::Link),
                Just(OpKind::Symlink),
                Just(OpKind::MkDir),
                Just(OpKind::SetXattr),
                Just(OpKind::RemoveXattr),
            ],
        ) {
            let serialized = serde_json::to_string(&op).unwrap();
            let deserialized: OpKind = serde_json::from_str(&serialized).unwrap();
            prop_assert_eq!(op, deserialized);
        }

        #[test]
        fn prop_link_status_serde(
            status in prop_oneof![
                Just(LinkStatus::Up),
                Just(LinkStatus::Degraded),
                Just(LinkStatus::Down),
            ],
        ) {
            let serialized = serde_json::to_string(&status).unwrap();
            let deserialized: LinkStatus = serde_json::from_str(&serialized).unwrap();
            prop_assert_eq!(status, deserialized);
        }
    }
}