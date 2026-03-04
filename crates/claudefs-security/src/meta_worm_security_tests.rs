//! Meta WORM compliance security tests.
//!
//! Part of A10 Phase 23: Meta WORM security audit

#[cfg(test)]
mod tests {
    use claudefs_meta::types::{InodeId, MetaError, Timestamp};
    use claudefs_meta::worm::{RetentionPolicy, WormAuditEvent, WormEntry, WormManager, WormState};

    fn make_worm_manager() -> WormManager {
        WormManager::new()
    }

    fn make_retention_policy_zero() -> RetentionPolicy {
        RetentionPolicy::new(0, None, false)
    }

    fn make_retention_policy_one_sec() -> RetentionPolicy {
        RetentionPolicy::new(1, None, false)
    }

    fn make_retention_policy_large() -> RetentionPolicy {
        RetentionPolicy::new(999999, None, false)
    }

    // ============================================================================
    // Category 1: Retention Policy (4 tests)
    // ============================================================================

    #[test]
    fn test_retention_policy_default() {
        let policy = RetentionPolicy::default_policy();
        assert_eq!(policy.min_retention_secs, 365 * 24 * 60 * 60);
        assert!(policy.max_retention_secs.is_none());
        assert!(!policy.auto_lock_on_close);
    }

    #[test]
    fn test_retention_policy_custom() {
        let policy = RetentionPolicy::new(86400, Some(2592000), true);
        assert_eq!(policy.min_retention_secs, 86400);
        assert_eq!(policy.max_retention_secs, Some(2592000));
        assert!(policy.auto_lock_on_close);
    }

    #[test]
    fn test_retention_policy_zero_min() {
        let policy = RetentionPolicy::new(0, None, false);
        assert_eq!(policy.min_retention_secs, 0);
        // FINDING-META-WORM-01: zero-retention policy allows immediate unlock —
        // caller must enforce minimum
    }

    #[test]
    fn test_retention_policy_no_max() {
        let policy = RetentionPolicy::new(86400, None, false);
        assert!(policy.max_retention_secs.is_none());
        // FINDING-META-WORM-02: no-max retention allows indefinite lock —
        // correct for compliance scenarios
    }

    // ============================================================================
    // Category 2: WORM State Machine (5 tests)
    // ============================================================================

    #[test]
    fn test_worm_state_unlocked() {
        let state = WormState::Unlocked;
        assert!(!state.is_protected());
        assert!(!state.is_locked());
        assert!(!state.is_legal_hold());
    }

    #[test]
    fn test_worm_state_locked() {
        let now = Timestamp::now();
        let state = WormState::Locked {
            locked_at: now,
            locked_until: Timestamp {
                secs: now.secs + 3600,
                nanos: 0,
            },
        };
        assert!(state.is_protected());
        assert!(state.is_locked());
        assert!(!state.is_legal_hold());
    }

    #[test]
    fn test_worm_state_legal_hold() {
        let state = WormState::LegalHold {
            hold_id: "CASE-2024-001".to_string(),
            held_at: Timestamp::now(),
        };
        assert!(state.is_protected());
        assert!(!state.is_locked());
        assert!(state.is_legal_hold());
    }

    #[test]
    fn test_worm_entry_new() {
        let entry = WormEntry::new(InodeId::new(100));
        assert_eq!(entry.ino, InodeId::new(100));
        assert!(matches!(entry.state, WormState::Unlocked));
        assert!(entry.retention_policy.is_none());
        assert!(entry.audit_trail.is_empty());
    }

    #[test]
    fn test_worm_entry_audit_event() {
        let mut entry = WormEntry::new(InodeId::new(100));
        entry.add_audit_event(WormAuditEvent::new(
            "test".to_string(),
            1000,
            "details".to_string(),
        ));
        assert_eq!(entry.audit_trail.len(), 1);
        assert_eq!(entry.audit_trail[0].event_type, "test");
        assert_eq!(entry.audit_trail[0].actor_uid, 1000);
    }

    // ============================================================================
    // Category 3: WORM Manager Lock/Unlock (6 tests)
    // ============================================================================

    #[test]
    fn test_worm_lock_file_with_policy() {
        let manager = make_worm_manager();
        let ino = InodeId::new(100);
        let policy = make_retention_policy_one_sec();
        let uid = 1000;

        manager.set_retention_policy(ino, policy, uid);
        manager.lock_file(ino, uid).unwrap();

        let state = manager.get_state(ino).unwrap();
        assert!(matches!(state, WormState::Locked { .. }));
        assert!(manager.is_immutable(ino));
        assert!(!manager.can_delete(ino));
        assert!(!manager.can_modify(ino));
        // FINDING-META-WORM-03: locked files are immutable — modification and
        // deletion blocked
    }

    #[test]
    fn test_worm_lock_file_no_policy() {
        let manager = make_worm_manager();
        let ino = InodeId::new(100);
        let result = manager.lock_file(ino, 1000);
        assert!(result.is_err());
        assert!(matches!(result, Err(MetaError::PermissionDenied)));
        // FINDING-META-WORM-04: lock requires retention policy — prevents
        // accidental permanent lock
    }

    #[test]
    fn test_worm_unlock_after_retention() {
        let manager = make_worm_manager();
        let ino = InodeId::new(100);
        let policy = make_retention_policy_zero();
        let uid = 1000;

        manager.set_retention_policy(ino, policy, uid);
        manager.lock_file(ino, uid).unwrap();
        manager.unlock_file(ino, uid).unwrap();

        let state = manager.get_state(ino).unwrap();
        assert!(matches!(state, WormState::Unlocked));
        assert!(manager.can_delete(ino));
        // FINDING-META-WORM-05: unlock succeeds when retention expires —
        // correct WORM lifecycle
    }

    #[test]
    fn test_worm_unlock_during_retention() {
        let manager = make_worm_manager();
        let ino = InodeId::new(100);
        let policy = make_retention_policy_large();
        let uid = 1000;

        manager.set_retention_policy(ino, policy, uid);
        manager.lock_file(ino, uid).unwrap();

        let result = manager.unlock_file(ino, uid);
        assert!(result.is_err());
        assert!(matches!(result, Err(MetaError::PermissionDenied)));

        let state = manager.get_state(ino).unwrap();
        assert!(matches!(state, WormState::Locked { .. }));
        // FINDING-META-WORM-06: unlock during active retention correctly rejected —
        // prevents compliance violation
    }

    #[test]
    fn test_worm_unlock_legal_hold_prevents() {
        let manager = make_worm_manager();
        let ino = InodeId::new(100);
        let uid = 1000;

        manager.place_legal_hold(ino, "CASE-2024-001".to_string(), uid);

        let result = manager.unlock_file(ino, uid);
        assert!(result.is_err());
        assert!(matches!(result, Err(MetaError::PermissionDenied)));
        // FINDING-META-WORM-07: legal hold prevents unlock — regulatory compliance enforced
    }

    #[test]
    fn test_worm_unlock_already_unlocked() {
        let manager = make_worm_manager();
        let ino = InodeId::new(100);
        let policy = make_retention_policy_zero();
        let uid = 1000;

        manager.set_retention_policy(ino, policy, uid);
        manager.lock_file(ino, uid).unwrap();
        manager.unlock_file(ino, uid).unwrap();

        // Try unlock again on already unlocked file
        let result = manager.unlock_file(ino, uid);
        assert!(result.is_ok());
        // FINDING-META-WORM-08: unlock on already-unlocked is idempotent — no error
    }

    // ============================================================================
    // Category 4: Legal Hold Operations (5 tests)
    // ============================================================================

    #[test]
    fn test_worm_place_legal_hold() {
        let manager = make_worm_manager();
        let ino = InodeId::new(100);
        let uid = 1000;

        manager.place_legal_hold(ino, "CASE-2024-001".to_string(), uid);

        let state = manager.get_state(ino).unwrap();
        assert!(
            matches!(state, WormState::LegalHold { hold_id, .. } if hold_id == "CASE-2024-001")
        );
        assert!(manager.is_immutable(ino));
        assert!(!manager.can_delete(ino));
    }

    #[test]
    fn test_worm_release_legal_hold() {
        let manager = make_worm_manager();
        let ino = InodeId::new(100);
        let uid = 1000;

        manager.place_legal_hold(ino, "CASE-2024-001".to_string(), uid);
        manager
            .release_legal_hold(ino, "CASE-2024-001", uid)
            .unwrap();

        let state = manager.get_state(ino).unwrap();
        assert!(matches!(state, WormState::Unlocked));
        assert!(manager.can_delete(ino));
        // FINDING-META-WORM-09: legal hold release requires exact hold_id match —
        // prevents unauthorized release
    }

    #[test]
    fn test_worm_release_wrong_hold_id() {
        let manager = make_worm_manager();
        let ino = InodeId::new(100);
        let uid = 1000;

        manager.place_legal_hold(ino, "CASE-001".to_string(), uid);

        let result = manager.release_legal_hold(ino, "CASE-002", uid);
        assert!(result.is_err());
        assert!(matches!(result, Err(MetaError::PermissionDenied)));

        let state = manager.get_state(ino).unwrap();
        assert!(matches!(state, WormState::LegalHold { .. }));
        // FINDING-META-WORM-10: wrong hold_id rejected — prevents release of
        // wrong case's hold
    }

    #[test]
    fn test_worm_release_nonexistent() {
        let manager = make_worm_manager();
        let ino = InodeId::new(999);

        let result = manager.release_legal_hold(ino, "CASE-001", 1000);
        assert!(result.is_err());
        // FINDING-META-WORM-11: release on nonexistent inode correctly rejected
    }

    #[test]
    fn test_worm_legal_hold_overrides_lock() {
        let manager = make_worm_manager();
        let ino = InodeId::new(100);
        let policy = make_retention_policy_one_sec();
        let uid = 1000;

        manager.set_retention_policy(ino, policy, uid);
        manager.lock_file(ino, uid).unwrap();

        // Place legal hold (should override lock state)
        manager.place_legal_hold(ino, "CASE-2024-001".to_string(), uid);

        let state = manager.get_state(ino).unwrap();
        assert!(matches!(state, WormState::LegalHold { .. }));

        // Try unlock - should fail due to legal hold
        let result = manager.unlock_file(ino, uid);
        assert!(result.is_err());
        assert!(matches!(result, Err(MetaError::PermissionDenied)));

        // Release hold - should return to Unlocked
        manager
            .release_legal_hold(ino, "CASE-2024-001", uid)
            .unwrap();

        let state_after = manager.get_state(ino).unwrap();
        assert!(matches!(state_after, WormState::Unlocked));
        // FINDING-META-WORM-12: legal hold overrides retention lock — highest
        // protection wins
    }

    // ============================================================================
    // Category 5: Audit Trail & Counts (5 tests)
    // ============================================================================

    #[test]
    fn test_worm_audit_trail_records() {
        let manager = make_worm_manager();
        let ino = InodeId::new(100);
        let policy = make_retention_policy_zero();
        let uid = 1000;

        manager.set_retention_policy(ino, policy, uid);
        manager.lock_file(ino, uid).unwrap();
        manager.unlock_file(ino, uid).unwrap();

        let trail = manager.audit_trail(ino);
        assert!(trail.len() >= 3);

        let event_types: Vec<&str> = trail.iter().map(|e| e.event_type.as_str()).collect();
        assert!(event_types.contains(&"set_retention_policy"));
        assert!(event_types.contains(&"lock_file"));
        assert!(event_types.contains(&"unlock_file"));
        // FINDING-META-WORM-13: all WORM operations are audited — compliance
        // trail complete
    }

    #[test]
    fn test_worm_audit_trail_actor() {
        let manager = make_worm_manager();
        let ino = InodeId::new(100);
        let policy = make_retention_policy_zero();
        let actor_uid = 1000;

        manager.set_retention_policy(ino, policy, actor_uid);
        manager.lock_file(ino, actor_uid).unwrap();

        let trail = manager.audit_trail(ino);
        let lock_event = trail.iter().find(|e| e.event_type == "lock_file").unwrap();
        assert_eq!(lock_event.actor_uid, actor_uid);
        // FINDING-META-WORM-14: audit records actor identity — supports accountability
    }

    #[test]
    fn test_worm_audit_trail_empty_inode() {
        let manager = make_worm_manager();
        let ino = InodeId::new(999);

        let trail = manager.audit_trail(ino);
        assert!(trail.is_empty());
    }

    #[test]
    fn test_worm_count() {
        let manager = make_worm_manager();

        assert_eq!(manager.worm_count(), 0);

        let ino1 = InodeId::new(100);
        manager.place_legal_hold(ino1, "CASE-001".to_string(), 1000);
        assert_eq!(manager.worm_count(), 1);

        let ino2 = InodeId::new(200);
        let policy = make_retention_policy_zero();
        manager.set_retention_policy(ino2, policy, 1000);
        manager.lock_file(ino2, 1000).unwrap();
        assert_eq!(manager.worm_count(), 2);

        // Unlock ino2 (0-sec retention)
        manager.unlock_file(ino2, 1000).unwrap();
        assert_eq!(manager.worm_count(), 1);
    }

    #[test]
    fn test_worm_unprotected_file_operations() {
        let manager = make_worm_manager();
        let nonexistent_ino = InodeId::new(99999);

        assert!(!manager.is_immutable(nonexistent_ino));
        assert!(manager.can_delete(nonexistent_ino));
        assert!(manager.can_modify(nonexistent_ino));
        assert!(manager.get_state(nonexistent_ino).is_none());
        // FINDING-META-WORM-15: unregistered files are not protected — correct
        // default behavior
    }
}
