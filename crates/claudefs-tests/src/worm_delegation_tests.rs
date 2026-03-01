//! WORM and Delegation Tests
//!
//! Tests for A5 WORM compliance and file delegation cross-scenarios.

#[cfg(test)]
mod tests {
    use claudefs_fuse::{
        deleg::{DelegState, DelegType, Delegation, DelegationManager},
        worm::{ImmutabilityMode, WormRecord, WormRegistry},
    };

    #[test]
    fn test_immutability_none_allows_write() {
        let mode = ImmutabilityMode::None;
        assert!(!mode.is_write_blocked(0));
    }

    #[test]
    fn test_immutability_none_allows_delete() {
        let mode = ImmutabilityMode::None;
        assert!(!mode.is_delete_blocked(0));
    }

    #[test]
    fn test_immutability_append_only_blocks_write() {
        let mode = ImmutabilityMode::AppendOnly;
        assert!(mode.is_write_blocked(0));
    }

    #[test]
    fn test_immutability_append_only_allows_append() {
        let mode = ImmutabilityMode::AppendOnly;
        assert!(mode.is_append_allowed(0));
    }

    #[test]
    fn test_immutability_immutable_blocks_all() {
        let mode = ImmutabilityMode::Immutable;
        assert!(mode.is_write_blocked(0));
        assert!(mode.is_delete_blocked(0));
        assert!(mode.is_rename_blocked(0));
        assert!(mode.is_truncate_blocked(0));
    }

    #[test]
    fn test_worm_retention_blocks_during_period() {
        let mode = ImmutabilityMode::WormRetention {
            retention_expires_at_secs: 9999,
        };
        assert!(mode.is_write_blocked(0));
    }

    #[test]
    fn test_worm_retention_allows_after_expiry() {
        let mode = ImmutabilityMode::WormRetention {
            retention_expires_at_secs: 9999,
        };
        assert!(!mode.is_write_blocked(99999));
    }

    #[test]
    fn test_legal_hold_blocks_delete() {
        let mode = ImmutabilityMode::LegalHold {
            hold_id: "hold1".into(),
        };
        assert!(mode.is_delete_blocked(0));
    }

    #[test]
    fn test_legal_hold_blocks_rename() {
        let mode = ImmutabilityMode::LegalHold {
            hold_id: "hold1".into(),
        };
        assert!(mode.is_rename_blocked(0));
    }

    #[test]
    fn test_worm_record_new() {
        let record = WormRecord::new(1, ImmutabilityMode::Immutable, 100, 1000);
        assert_eq!(record.ino, 1);
    }

    #[test]
    fn test_worm_record_check_write_immutable() {
        let record = WormRecord::new(1, ImmutabilityMode::Immutable, 0, 0);
        let result = record.check_write(100);
        assert!(result.is_err());
    }

    #[test]
    fn test_worm_record_check_delete_immutable() {
        let record = WormRecord::new(1, ImmutabilityMode::Immutable, 0, 0);
        let result = record.check_delete(100);
        assert!(result.is_err());
    }

    #[test]
    fn test_worm_record_check_rename_immutable() {
        let record = WormRecord::new(1, ImmutabilityMode::Immutable, 0, 0);
        let result = record.check_rename(100);
        assert!(result.is_err());
    }

    #[test]
    fn test_worm_record_check_truncate_immutable() {
        let record = WormRecord::new(1, ImmutabilityMode::Immutable, 0, 0);
        let result = record.check_truncate(100);
        assert!(result.is_err());
    }

    #[test]
    fn test_worm_record_check_write_none() {
        let record = WormRecord::new(1, ImmutabilityMode::None, 0, 0);
        let result = record.check_write(100);
        assert!(result.is_ok());
    }

    #[test]
    fn test_worm_registry_new() {
        let registry = WormRegistry::new();
        assert_eq!(registry.len(), 0);
    }

    #[test]
    fn test_worm_registry_set_and_get() {
        let mut registry = WormRegistry::new();
        let mode = ImmutabilityMode::Immutable;
        registry.set_mode(1, mode.clone(), 0, 0);
        let result = registry.get(1);
        assert!(result.is_some());
    }

    #[test]
    fn test_worm_registry_check_write_unknown_ino() {
        let registry = WormRegistry::new();
        let result = registry.check_write(999, 0);
        assert!(result.is_ok());
    }

    #[test]
    fn test_worm_registry_check_write_immutable() {
        let mut registry = WormRegistry::new();
        registry.set_mode(1, ImmutabilityMode::Immutable, 0, 0);
        let result = registry.check_write(1, 0);
        assert!(result.is_err());
    }

    #[test]
    fn test_worm_registry_clear() {
        let mut registry = WormRegistry::new();
        registry.set_mode(1, ImmutabilityMode::Immutable, 0, 0);
        registry.clear(1);
        let result = registry.get(1);
        assert!(result.is_none());
    }

    #[test]
    fn test_worm_registry_len() {
        let mut registry = WormRegistry::new();
        registry.set_mode(1, ImmutabilityMode::Immutable, 0, 0);
        registry.set_mode(2, ImmutabilityMode::AppendOnly, 0, 0);
        assert_eq!(registry.len(), 2);
    }

    #[test]
    fn test_delegation_new_read() {
        let deleg = Delegation::new(1, 10, DelegType::Read, 0, 100, 300);
        assert_eq!(deleg.deleg_type, DelegType::Read);
    }

    #[test]
    fn test_delegation_new_write() {
        let deleg = Delegation::new(1, 10, DelegType::Write, 0, 100, 300);
        assert_eq!(deleg.deleg_type, DelegType::Write);
    }

    #[test]
    fn test_delegation_is_active_initially() {
        let deleg = Delegation::new(1, 10, DelegType::Read, 0, 100, 300);
        assert!(deleg.is_active());
    }

    #[test]
    fn test_delegation_is_expired_before() {
        let deleg = Delegation::new(1, 10, DelegType::Read, 0, 100, 300);
        assert!(!deleg.is_expired(200));
    }

    #[test]
    fn test_delegation_is_expired_after() {
        let deleg = Delegation::new(1, 10, DelegType::Read, 0, 100, 300);
        assert!(deleg.is_expired(500));
    }

    #[test]
    fn test_delegation_time_remaining() {
        let deleg = Delegation::new(1, 10, DelegType::Read, 0, 100, 300);
        let remaining = deleg.time_remaining_secs(150);
        assert!(remaining >= 0);
    }

    #[test]
    fn test_delegation_recall() {
        let mut deleg = Delegation::new(1, 10, DelegType::Read, 0, 100, 300);
        deleg.recall(200);
        assert!(matches!(deleg.state, DelegState::Recalled { .. }));
    }

    #[test]
    fn test_delegation_returned() {
        let mut deleg = Delegation::new(1, 10, DelegType::Read, 0, 100, 300);
        deleg.recall(200);
        deleg.returned(200);
        assert!(matches!(deleg.state, DelegState::Returned { .. }));
    }

    #[test]
    fn test_delegation_revoke() {
        let mut deleg = Delegation::new(1, 10, DelegType::Read, 0, 100, 300);
        deleg.revoke(200);
        assert!(matches!(deleg.state, DelegState::Revoked { .. }));
    }

    #[test]
    fn test_delegation_manager_new() {
        let manager = DelegationManager::new(300);
        assert_eq!(manager.active_count(), 0);
    }

    #[test]
    fn test_delegation_manager_grant_read() {
        let mut manager = DelegationManager::new(300);
        let result = manager.grant(10, DelegType::Read, 0, 100);
        assert!(result.is_ok());
    }

    #[test]
    fn test_delegation_manager_grant_write() {
        let mut manager = DelegationManager::new(300);
        let result = manager.grant(10, DelegType::Write, 0, 100);
        assert!(result.is_ok());
    }

    #[test]
    fn test_delegation_manager_conflict_write_blocks_read() {
        let mut manager = DelegationManager::new(300);
        manager.grant(10, DelegType::Write, 0, 100).unwrap();
        let result = manager.grant(10, DelegType::Read, 1, 100);
        assert!(result.is_err());
    }

    #[test]
    fn test_delegation_manager_conflict_write_blocks_write() {
        let mut manager = DelegationManager::new(300);
        manager.grant(10, DelegType::Write, 0, 100).unwrap();
        let result = manager.grant(10, DelegType::Write, 1, 100);
        assert!(result.is_err());
    }

    #[test]
    fn test_delegation_manager_multiple_reads_allowed() {
        let mut manager = DelegationManager::new(300);
        manager.grant(10, DelegType::Read, 0, 100).unwrap();
        let result = manager.grant(10, DelegType::Read, 1, 100);
        assert!(result.is_ok());
    }

    #[test]
    fn test_delegation_manager_recall_for_ino() {
        let mut manager = DelegationManager::new(300);
        manager.grant(10, DelegType::Read, 0, 100).unwrap();
        manager.recall_for_ino(10, 200);
        assert_eq!(manager.delegations_for_ino(10).len(), 0);
    }

    #[test]
    fn test_delegation_manager_return_deleg() {
        let mut manager = DelegationManager::new(300);
        let result = manager.grant(10, DelegType::Read, 0, 100);
        assert!(result.is_ok());
        let deleg_id = result.unwrap();
        let return_result = manager.return_deleg(deleg_id, 200);
        assert!(return_result.is_ok());
    }

    #[test]
    fn test_delegation_manager_return_unknown() {
        let mut manager = DelegationManager::new(300);
        let result = manager.return_deleg(999, 200);
        assert!(result.is_err());
    }

    #[test]
    fn test_delegation_manager_revoke_expired() {
        let mut manager = DelegationManager::new(300);
        manager.grant(10, DelegType::Read, 0, 100).unwrap();
        manager.revoke_expired(500);
        assert_eq!(manager.active_count(), 0);
    }
}
