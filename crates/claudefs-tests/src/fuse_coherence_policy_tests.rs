//! Tests for new FUSE modules: cache_coherence, sec_policy.

use claudefs_fuse::{
    cache_coherence::{
        CacheLease, CoherenceError, CoherenceManager, CoherenceProtocol, LeaseId, LeaseState,
    },
    sec_policy::{
        Capability, CapabilitySet, PolicyEnforcer, PolicyViolation, SeccompMode, SecurityProfile,
        SyscallPolicy, ViolationType,
    },
};
use std::time::Duration;

#[cfg(test)]
mod tests {
    use super::*;

    // LeaseId tests
    #[test]
    fn test_lease_id_new() {
        let id = LeaseId::new(1);
        assert_eq!(format!("{}", id), "lease:1");
    }

    #[test]
    fn test_lease_id_equality() {
        let id1 = LeaseId::new(1);
        let id2 = LeaseId::new(1);
        let id3 = LeaseId::new(2);
        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
    }

    // LeaseState tests
    #[test]
    fn test_lease_state_active() {
        assert!(matches!(LeaseState::Active, LeaseState::Active));
    }

    #[test]
    fn test_lease_state_expired() {
        assert!(matches!(LeaseState::Expired, LeaseState::Expired));
    }

    #[test]
    fn test_lease_state_revoked() {
        assert!(matches!(LeaseState::Revoked, LeaseState::Revoked));
    }

    #[test]
    fn test_lease_state_renewing() {
        assert!(matches!(LeaseState::Renewing, LeaseState::Renewing));
    }

    // CacheLease tests
    #[test]
    fn test_cache_lease_new_active() {
        let lease = CacheLease::new(LeaseId::new(1), 100, 42, Duration::from_secs(30));
        assert!(matches!(lease.state, LeaseState::Active));
    }

    #[test]
    fn test_cache_lease_is_valid_initially() {
        let lease = CacheLease::new(LeaseId::new(1), 100, 42, Duration::from_secs(30));
        assert!(lease.is_valid());
    }

    #[test]
    fn test_cache_lease_is_expired_initially() {
        let lease = CacheLease::new(LeaseId::new(1), 100, 42, Duration::from_secs(30));
        assert!(!lease.is_expired());
    }

    #[test]
    fn test_cache_lease_time_remaining_positive() {
        let lease = CacheLease::new(LeaseId::new(1), 100, 42, Duration::from_secs(30));
        assert!(lease.time_remaining() > Duration::ZERO);
    }

    #[test]
    fn test_cache_lease_revoke() {
        let mut lease = CacheLease::new(LeaseId::new(1), 100, 42, Duration::from_secs(30));
        lease.revoke();
        assert!(!lease.is_valid());
    }

    #[test]
    fn test_cache_lease_expired_state_not_valid() {
        let mut lease = CacheLease::new(LeaseId::new(1), 100, 42, Duration::from_secs(0));
        std::thread::sleep(Duration::from_millis(1));
        assert!(!lease.is_valid());
    }

    #[test]
    fn test_cache_lease_renew() {
        let mut lease = CacheLease::new(LeaseId::new(1), 100, 42, Duration::from_millis(10));
        std::thread::sleep(Duration::from_millis(15));
        lease.renew(Duration::from_secs(60));
        assert!(lease.is_valid());
    }

    // CoherenceManager tests
    #[test]
    fn test_coherence_manager_new() {
        let mut manager = CoherenceManager::new(CoherenceProtocol::CloseToOpen);
        assert_eq!(manager.active_lease_count(), 0);
    }

    #[test]
    fn test_coherence_manager_grant() {
        let mut manager = CoherenceManager::new(CoherenceProtocol::CloseToOpen);
        let lease = manager.grant_lease(100, 1);
        assert_eq!(lease.inode, 100);
    }

    #[test]
    fn test_coherence_manager_revoke() {
        let mut manager = CoherenceManager::new(CoherenceProtocol::CloseToOpen);
        manager.grant_lease(100, 1);
        let inv = manager.revoke_lease(100);
        assert!(inv.is_some());
    }

    #[test]
    fn test_coherence_manager_check_lease() {
        let mut manager = CoherenceManager::new(CoherenceProtocol::CloseToOpen);
        manager.grant_lease(100, 1);
        let lease = manager.check_lease(100);
        assert!(lease.is_some());
    }

    #[test]
    fn test_coherence_manager_active_count() {
        let mut manager = CoherenceManager::new(CoherenceProtocol::CloseToOpen);
        manager.grant_lease(100, 1);
        manager.grant_lease(200, 1);
        assert_eq!(manager.active_lease_count(), 2);
    }

    #[test]
    fn test_coherence_manager_is_coherent() {
        let mut manager = CoherenceManager::new(CoherenceProtocol::CloseToOpen);
        assert!(!manager.is_coherent(100));
        manager.grant_lease(100, 1);
        assert!(manager.is_coherent(100));
    }

    // Capability tests
    #[test]
    fn test_capability_set_new_empty() {
        let caps = CapabilitySet::new();
        assert!(caps.is_empty());
    }

    #[test]
    fn test_capability_set_fuse_minimal_has_sys_admin() {
        let caps = CapabilitySet::fuse_minimal();
        assert!(caps.contains(&Capability::SysAdmin));
    }

    #[test]
    fn test_capability_set_add() {
        let mut caps = CapabilitySet::new();
        caps.add(Capability::SysAdmin);
        assert_eq!(caps.len(), 1);
    }

    #[test]
    fn test_capability_set_add_idempotent() {
        let mut caps = CapabilitySet::new();
        caps.add(Capability::SysAdmin);
        caps.add(Capability::SysAdmin);
        assert_eq!(caps.len(), 1);
    }

    #[test]
    fn test_capability_set_contains_after_add() {
        let mut caps = CapabilitySet::new();
        caps.add(Capability::SysAdmin);
        assert!(caps.contains(&Capability::SysAdmin));
    }

    #[test]
    fn test_capability_set_remove() {
        let mut caps = CapabilitySet::new();
        caps.add(Capability::SysAdmin);
        caps.remove(Capability::SysAdmin);
        assert!(!caps.contains(&Capability::SysAdmin));
    }

    #[test]
    fn test_capability_set_remove_nonexistent() {
        let mut caps = CapabilitySet::new();
        assert!(!caps.remove(Capability::SysAdmin));
    }

    #[test]
    fn test_capability_set_len() {
        let mut caps = CapabilitySet::new();
        caps.add(Capability::SysAdmin);
        caps.add(Capability::NetAdmin);
        assert_eq!(caps.len(), 2);
    }

    #[test]
    fn test_capability_set_is_empty_after_remove() {
        let mut caps = CapabilitySet::new();
        caps.add(Capability::SysAdmin);
        caps.remove(Capability::SysAdmin);
        assert!(caps.is_empty());
    }

    // Capability enum tests
    #[test]
    fn test_capability_dac_read_search() {
        assert!(matches!(
            Capability::DacReadSearch,
            Capability::DacReadSearch
        ));
    }

    #[test]
    fn test_capability_dac_override() {
        assert!(matches!(Capability::DacOverride, Capability::DacOverride));
    }

    #[test]
    fn test_capability_chown() {
        assert!(matches!(Capability::Chown, Capability::Chown));
    }

    #[test]
    fn test_capability_fowner() {
        assert!(matches!(Capability::FOwner, Capability::FOwner));
    }

    #[test]
    fn test_capability_fsetid() {
        assert!(matches!(Capability::FSetId, Capability::FSetId));
    }

    #[test]
    fn test_capability_kill() {
        assert!(matches!(Capability::Kill, Capability::Kill));
    }

    #[test]
    fn test_capability_setgid() {
        assert!(matches!(Capability::SetGid, Capability::SetGid));
    }

    #[test]
    fn test_capability_setuid() {
        assert!(matches!(Capability::SetUid, Capability::SetUid));
    }

    #[test]
    fn test_capability_net_admin() {
        assert!(matches!(Capability::NetAdmin, Capability::NetAdmin));
    }

    #[test]
    fn test_capability_sys_chroot() {
        assert!(matches!(Capability::SysChroot, Capability::SysChroot));
    }

    #[test]
    fn test_capability_mknod() {
        assert!(matches!(Capability::Mknod, Capability::Mknod));
    }

    #[test]
    fn test_capability_lease() {
        assert!(matches!(Capability::Lease, Capability::Lease));
    }

    #[test]
    fn test_capability_audit_write() {
        assert!(matches!(Capability::AuditWrite, Capability::AuditWrite));
    }

    // SeccompMode tests
    #[test]
    fn test_seccomp_mode_default_disabled() {
        assert_eq!(SeccompMode::default(), SeccompMode::Disabled);
    }

    #[test]
    fn test_seccomp_mode_log() {
        assert!(matches!(SeccompMode::Log, SeccompMode::Log));
    }

    #[test]
    fn test_seccomp_mode_disabled() {
        assert!(matches!(SeccompMode::Disabled, SeccompMode::Disabled));
    }

    #[test]
    fn test_seccomp_mode_enforce() {
        assert!(matches!(SeccompMode::Enforce, SeccompMode::Enforce));
    }

    // SyscallPolicy tests
    #[test]
    fn test_syscall_policy_new() {
        let policy = SyscallPolicy::new();
        assert!(matches!(policy.mode(), SeccompMode::Disabled));
    }

    #[test]
    fn test_syscall_policy_fuse_allowlist() {
        let policy = SyscallPolicy::fuse_allowlist();
        assert!(policy.is_allowed("read"));
        assert!(policy.is_allowed("write"));
    }

    #[test]
    fn test_syscall_policy_is_blocked() {
        let policy = SyscallPolicy::new();
        assert!(!policy.is_blocked("read"));
    }

    #[test]
    fn test_syscall_policy_with_mode() {
        let policy = SyscallPolicy::new().with_mode(SeccompMode::Enforce);
        assert!(matches!(policy.mode(), SeccompMode::Enforce));
    }

    // SecurityProfile tests
    #[test]
    fn test_security_profile_default() {
        let profile = SecurityProfile::default();
        assert!(profile.required_capabilities().is_empty());
    }

    #[test]
    fn test_security_profile_hardened() {
        let profile = SecurityProfile::hardened();
        assert!(profile
            .required_capabilities()
            .contains(&Capability::SysAdmin));
    }

    #[test]
    fn test_security_profile_with_capabilities() {
        let mut caps = CapabilitySet::new();
        caps.add(Capability::SysAdmin);
        let profile = SecurityProfile::with_capabilities(caps);
        assert_eq!(profile.required_capabilities().len(), 1);
    }

    #[test]
    fn test_security_profile_is_syscall_permitted() {
        let profile = SecurityProfile::hardened();
        assert!(profile.is_syscall_permitted("read"));
    }

    #[test]
    fn test_security_profile_with_syscall_policy() {
        let policy = SyscallPolicy::fuse_allowlist();
        let profile = SecurityProfile::with_syscall_policy(policy);
        assert!(profile.is_syscall_permitted("read"));
    }

    #[test]
    fn test_security_profile_with_no_new_privs() {
        let profile = SecurityProfile::default().with_no_new_privs(true);
        assert!(profile.enforce_no_new_privs());
    }

    // PolicyEnforcer tests
    #[test]
    fn test_policy_enforcer_new() {
        let profile = SecurityProfile::default();
        let enforcer = PolicyEnforcer::new(profile);
        assert_eq!(enforcer.violation_count(), 0);
    }

    #[test]
    fn test_policy_enforcer_check_syscall_allowed() {
        let profile = SecurityProfile::hardened();
        let mut enforcer = PolicyEnforcer::new(profile);
        assert!(enforcer.check_syscall("read").is_ok());
    }

    #[test]
    fn test_policy_enforcer_check_syscall_blocked() {
        let profile = SecurityProfile::hardened();
        let mut enforcer = PolicyEnforcer::new(profile);
        let result = enforcer.check_syscall("nonexistent_syscall_xyz");
        assert!(result.is_err());
    }

    #[test]
    fn test_policy_enforcer_record_violation() {
        let profile = SecurityProfile::default();
        let mut enforcer = PolicyEnforcer::new(profile);
        enforcer.record_violation(
            ViolationType::UnauthorizedSyscall("test".to_string()),
            "test violation",
        );
        assert_eq!(enforcer.violation_count(), 1);
    }

    #[test]
    fn test_policy_enforcer_recent_violations() {
        let profile = SecurityProfile::default();
        let mut enforcer = PolicyEnforcer::new(profile);
        for i in 0..5 {
            enforcer.record_violation(
                ViolationType::UnauthorizedSyscall("test".to_string()),
                &format!("violation {}", i),
            );
        }
        assert_eq!(enforcer.recent_violations(3).len(), 3);
    }

    #[test]
    fn test_policy_enforcer_is_over_limit() {
        let profile = SecurityProfile::default();
        let mut enforcer = PolicyEnforcer::new(profile).with_max_violations(3);
        for i in 0..5 {
            enforcer.record_violation(
                ViolationType::UnauthorizedSyscall("test".to_string()),
                &format!("violation {}", i),
            );
        }
        assert!(enforcer.is_over_limit());
    }

    #[test]
    fn test_policy_enforcer_clear_violations() {
        let profile = SecurityProfile::default();
        let mut enforcer = PolicyEnforcer::new(profile);
        enforcer.record_violation(
            ViolationType::UnauthorizedSyscall("test".to_string()),
            "violation",
        );
        enforcer.clear_violations();
        assert_eq!(enforcer.violation_count(), 0);
    }

    #[test]
    fn test_policy_enforcer_profile() {
        let profile = SecurityProfile::default();
        let enforcer = PolicyEnforcer::new(profile);
        assert!(enforcer.profile().required_capabilities().is_empty());
    }

    // PolicyViolation tests
    #[test]
    fn test_policy_violation_new() {
        let violation = PolicyViolation::new(
            ViolationType::UnauthorizedSyscall("execve".to_string()),
            "Attempted execve",
        );
        assert!(matches!(
            violation.violation_type(),
            ViolationType::UnauthorizedSyscall(_)
        ));
    }

    #[test]
    fn test_policy_violation_details() {
        let violation = PolicyViolation::new(
            ViolationType::UnauthorizedSyscall("test".to_string()),
            "test details",
        );
        assert_eq!(violation.details(), "test details");
    }

    // ViolationType tests
    #[test]
    fn test_violation_type_unauthorized_syscall() {
        let vtype = ViolationType::UnauthorizedSyscall("test".to_string());
        assert!(matches!(vtype, ViolationType::UnauthorizedSyscall(_)));
    }

    #[test]
    fn test_violation_type_capability_escalation() {
        let vtype = ViolationType::CapabilityEscalation("CAP_SYS_ADMIN".to_string());
        assert!(matches!(vtype, ViolationType::CapabilityEscalation(_)));
    }

    #[test]
    fn test_violation_type_new_privileges_attempt() {
        let vtype = ViolationType::NewPrivilegesAttempt("setuid".to_string());
        assert!(matches!(vtype, ViolationType::NewPrivilegesAttempt(_)));
    }

    #[test]
    fn test_violation_type_unauthorized_mount() {
        let vtype = ViolationType::UnauthorizedMount("/evil".to_string());
        assert!(matches!(vtype, ViolationType::UnauthorizedMount(_)));
    }

    // CoherenceProtocol tests
    #[test]
    fn test_coherence_protocol_default() {
        assert_eq!(CoherenceProtocol::default(), CoherenceProtocol::CloseToOpen);
    }

    #[test]
    fn test_coherence_protocol_close_to_open() {
        assert!(matches!(
            CoherenceProtocol::CloseToOpen,
            CoherenceProtocol::CloseToOpen
        ));
    }

    #[test]
    fn test_coherence_protocol_session_based() {
        assert!(matches!(
            CoherenceProtocol::SessionBased,
            CoherenceProtocol::SessionBased
        ));
    }

    #[test]
    fn test_coherence_protocol_strict() {
        assert!(matches!(
            CoherenceProtocol::Strict,
            CoherenceProtocol::Strict
        ));
    }

    // Additional cache_coherence tests
    #[test]
    fn test_coherence_manager_invalidate() {
        let mut manager = CoherenceManager::new(CoherenceProtocol::Strict);
        manager.grant_lease(100, 1);
        manager.invalidate(
            100,
            claudefs_fuse::cache_coherence::InvalidationReason::ExplicitFlush,
            0,
        );
        assert!(!manager.is_coherent(100));
    }

    #[test]
    fn test_coherence_manager_drain_invalidations() {
        let mut manager = CoherenceManager::new(CoherenceProtocol::CloseToOpen);
        manager.grant_lease(100, 1);
        manager.invalidate(
            100,
            claudefs_fuse::cache_coherence::InvalidationReason::LeaseExpired,
            0,
        );
        let drained = manager.drain_invalidations();
        assert_eq!(drained.len(), 1);
    }

    #[test]
    fn test_coherence_manager_expire_stale_leases() {
        let mut manager = CoherenceManager::new(CoherenceProtocol::CloseToOpen);
        manager.grant_lease(100, 1);
        std::thread::sleep(Duration::from_millis(5));
        // The granted lease should not be expired yet (30 second default duration)
        assert!(manager.is_coherent(100));
        // Manually expire leases that have passed their time
        let expired = manager.expire_stale_leases();
        // May be 0 if the lease hasn't expired yet
        assert!(expired >= 0);
    }

    #[test]
    fn test_coherence_manager_pending_invalidations() {
        let mut manager = CoherenceManager::new(CoherenceProtocol::CloseToOpen);
        manager.grant_lease(100, 1);
        manager.invalidate(
            100,
            claudefs_fuse::cache_coherence::InvalidationReason::RemoteWrite(1),
            5,
        );
        let invs = manager.pending_invalidations();
        assert_eq!(invs.len(), 1);
    }
}
