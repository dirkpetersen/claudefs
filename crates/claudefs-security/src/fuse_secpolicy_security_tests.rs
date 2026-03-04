//! FUSE security policy tests.

#[cfg(test)]
mod tests {
    use claudefs_fuse::sec_policy::{
        Capability, CapabilitySet, MountNamespace, PolicyEnforcer, PolicyViolation, SeccompMode,
        SecurityProfile, SyscallPolicy, ViolationType,
    };
    use std::time::{Duration, SystemTime};

    #[test]
    fn test_fuse_sp_sec_capability_set_new_is_empty() {
        let caps = CapabilitySet::new();
        assert!(caps.is_empty());
        assert_eq!(caps.len(), 0);
    }

    #[test]
    fn test_fuse_sp_sec_capability_set_fuse_minimal_has_sysadmin_only() {
        let caps = CapabilitySet::fuse_minimal();
        assert!(!caps.is_empty());
        assert_eq!(caps.len(), 1);
        assert!(caps.contains(&Capability::SysAdmin));
        assert!(!caps.contains(&Capability::DacOverride));
        assert!(!caps.contains(&Capability::NetAdmin));
    }

    #[test]
    fn test_fuse_sp_sec_capability_set_add_is_idempotent() {
        let mut caps = CapabilitySet::new();
        caps.add(Capability::SysAdmin);
        assert_eq!(caps.len(), 1);
        caps.add(Capability::SysAdmin);
        assert_eq!(caps.len(), 1);
        caps.add(Capability::SysAdmin);
        caps.add(Capability::SysAdmin);
        assert_eq!(caps.len(), 1);
    }

    #[test]
    fn test_fuse_sp_sec_capability_set_remove_returns_false_for_non_present() {
        let mut caps = CapabilitySet::new();
        caps.add(Capability::SysAdmin);
        let removed = caps.remove(Capability::NetAdmin);
        assert!(!removed);
        assert_eq!(caps.len(), 1);
        let removed_again = caps.remove(Capability::DacReadSearch);
        assert!(!removed_again);
    }

    #[test]
    fn test_fuse_sp_sec_capability_set_all_variants_addable() {
        let all_caps = [
            Capability::SysAdmin,
            Capability::DacReadSearch,
            Capability::DacOverride,
            Capability::Chown,
            Capability::FOwner,
            Capability::FSetId,
            Capability::Kill,
            Capability::SetGid,
            Capability::SetUid,
            Capability::SetPCap,
            Capability::NetAdmin,
            Capability::SysChroot,
            Capability::Mknod,
            Capability::Lease,
            Capability::AuditWrite,
        ];
        let mut caps = CapabilitySet::new();
        for cap in all_caps {
            caps.add(cap);
        }
        assert_eq!(caps.len(), 15);
        for cap in &all_caps {
            assert!(caps.contains(cap));
        }
    }

    #[test]
    fn test_fuse_sp_sec_syscall_policy_default_disabled_mode() {
        let policy = SyscallPolicy::new();
        assert_eq!(policy.mode(), SeccompMode::Disabled);
    }

    #[test]
    fn test_fuse_sp_sec_syscall_policy_fuse_allowlist_permits_key_syscalls() {
        let policy = SyscallPolicy::fuse_allowlist();
        assert!(policy.is_allowed("read"));
        assert!(policy.is_allowed("write"));
        assert!(policy.is_allowed("io_uring_enter"));
        assert!(policy.is_allowed("clone3"));
        assert!(policy.is_allowed("io_uring_setup"));
        assert!(policy.is_allowed("io_uring_register"));
    }

    #[test]
    fn test_fuse_sp_sec_syscall_policy_fuse_allowlist_blocks_fabricated() {
        let policy = SyscallPolicy::fuse_allowlist();
        assert!(!policy.is_allowed("fabricated_syscall_xyz_12345"));
        assert!(!policy.is_allowed("evil_syscall"));
        assert!(!policy.is_allowed("__malicious_syscall__"));
    }

    #[test]
    fn test_fuse_sp_sec_syscall_policy_fuse_allowlist_is_blocked_returns_false() {
        let policy = SyscallPolicy::fuse_allowlist();
        assert!(!policy.is_blocked("read"));
        assert!(!policy.is_blocked("reboot"));
        assert!(!policy.is_blocked("init_module"));
    }

    #[test]
    fn test_fuse_sp_sec_syscall_policy_empty_allows_anything() {
        let policy = SyscallPolicy::new();
        assert!(policy.is_allowed("any_random_syscall"));
        assert!(policy.is_allowed("made_up_syscall_xyz"));
        assert!(!policy.is_blocked("any_random_syscall"));
    }

    #[test]
    fn test_fuse_sp_sec_syscall_policy_with_mode_changes_mode() {
        let policy = SyscallPolicy::new().with_mode(SeccompMode::Enforce);
        assert_eq!(policy.mode(), SeccompMode::Enforce);
        let policy2 = SyscallPolicy::fuse_allowlist().with_mode(SeccompMode::Log);
        assert_eq!(policy2.mode(), SeccompMode::Log);
    }

    #[test]
    fn test_fuse_sp_sec_security_profile_default_is_permissive() {
        let profile = SecurityProfile::default_profile();
        assert!(profile.required_capabilities().is_empty());
        assert!(!profile.enforce_no_new_privs());
        assert!(profile.mount_ns().is_none());
    }

    #[test]
    fn test_fuse_sp_sec_security_profile_hardened_has_sysadmin_no_new_privs() {
        let profile = SecurityProfile::hardened();
        assert!(profile
            .required_capabilities()
            .contains(&Capability::SysAdmin));
        assert!(profile.enforce_no_new_privs());
        assert_eq!(profile.required_capabilities().len(), 1);
    }

    #[test]
    fn test_fuse_sp_sec_security_profile_hardened_permits_read_syscall() {
        let profile = SecurityProfile::hardened();
        assert!(profile.is_syscall_permitted("read"));
        assert!(profile.is_syscall_permitted("write"));
        assert!(profile.is_syscall_permitted("open"));
    }

    #[test]
    fn test_fuse_sp_sec_security_profile_hardened_blocks_unknown_syscall() {
        let profile = SecurityProfile::hardened();
        assert!(!profile.is_syscall_permitted("unknown_syscall_xyz_999"));
        assert!(!profile.is_syscall_permitted("malicious_call"));
    }

    #[test]
    fn test_fuse_sp_sec_security_profile_with_mount_namespace_sets_ns() {
        let ns = MountNamespace::new(12345, 67890);
        let profile = SecurityProfile::default_profile().with_mount_namespace(ns);
        assert!(profile.mount_ns().is_some());
        let mount_ns = profile.mount_ns().unwrap();
        assert_eq!(mount_ns.ns_id(), 12345);
        assert_eq!(mount_ns.pid(), 67890);
    }

    #[test]
    fn test_fuse_sp_sec_enforcer_new_has_zero_violations() {
        let profile = SecurityProfile::default_profile();
        let enforcer = PolicyEnforcer::new(profile);
        assert_eq!(enforcer.violation_count(), 0);
        assert!(!enforcer.is_over_limit());
    }

    #[test]
    fn test_fuse_sp_sec_enforcer_check_syscall_allowed_returns_ok() {
        let profile = SecurityProfile::hardened();
        let mut enforcer = PolicyEnforcer::new(profile);
        let result = enforcer.check_syscall("read");
        assert!(result.is_ok());
        assert_eq!(enforcer.violation_count(), 0);
    }

    #[test]
    fn test_fuse_sp_sec_enforcer_check_syscall_blocked_records_and_errors() {
        let profile = SecurityProfile::hardened();
        let mut enforcer = PolicyEnforcer::new(profile);
        let result = enforcer.check_syscall("evil_unpermitted_syscall_xyz");
        assert!(result.is_err());
        assert_eq!(enforcer.violation_count(), 1);
    }

    #[test]
    fn test_fuse_sp_sec_enforcer_violation_count_increments_correctly() {
        let profile = SecurityProfile::default_profile();
        let mut enforcer = PolicyEnforcer::new(profile);
        assert_eq!(enforcer.violation_count(), 0);
        enforcer.record_violation(
            ViolationType::UnauthorizedSyscall("test1".to_string()),
            "details1",
        );
        assert_eq!(enforcer.violation_count(), 1);
        enforcer.record_violation(
            ViolationType::CapabilityEscalation("cap1".to_string()),
            "details2",
        );
        assert_eq!(enforcer.violation_count(), 2);
        enforcer.record_violation(
            ViolationType::UnauthorizedMount("/mnt".to_string()),
            "details3",
        );
        assert_eq!(enforcer.violation_count(), 3);
    }

    #[test]
    fn test_fuse_sp_sec_enforcer_recent_violations_returns_min() {
        let profile = SecurityProfile::default_profile();
        let mut enforcer = PolicyEnforcer::new(profile);
        for i in 0..10 {
            enforcer.record_violation(
                ViolationType::UnauthorizedSyscall(format!("syscall_{}", i)),
                &format!("details_{}", i),
            );
        }
        assert_eq!(enforcer.violation_count(), 10);
        let recent3 = enforcer.recent_violations(3);
        assert_eq!(recent3.len(), 3);
        let recent5 = enforcer.recent_violations(5);
        assert_eq!(recent5.len(), 5);
        let recent20 = enforcer.recent_violations(20);
        assert_eq!(recent20.len(), 10);
    }

    #[test]
    fn test_fuse_sp_sec_enforcer_default_max_violations_is_100() {
        let profile = SecurityProfile::default_profile();
        let enforcer = PolicyEnforcer::new(profile);
        assert_eq!(enforcer.max_violations(), 100);
    }

    #[test]
    fn test_fuse_sp_sec_enforcer_with_max_violations_overrides() {
        let enforcer =
            PolicyEnforcer::new(SecurityProfile::default_profile()).with_max_violations(25);
        assert_eq!(enforcer.max_violations(), 25);
        let enforcer2 =
            PolicyEnforcer::new(SecurityProfile::default_profile()).with_max_violations(5);
        assert_eq!(enforcer2.max_violations(), 5);
    }

    #[test]
    fn test_fuse_sp_sec_enforcer_is_over_limit_true_when_exceeded() {
        let profile = SecurityProfile::default_profile();
        let mut enforcer = PolicyEnforcer::new(profile).with_max_violations(3);
        enforcer.record_violation(ViolationType::UnauthorizedSyscall("s1".to_string()), "d1");
        assert!(!enforcer.is_over_limit());
        enforcer.record_violation(ViolationType::UnauthorizedSyscall("s2".to_string()), "d2");
        assert!(!enforcer.is_over_limit());
        enforcer.record_violation(ViolationType::UnauthorizedSyscall("s3".to_string()), "d3");
        assert!(enforcer.is_over_limit());
        enforcer.record_violation(ViolationType::UnauthorizedSyscall("s4".to_string()), "d4");
        assert!(enforcer.is_over_limit());
    }

    #[test]
    fn test_fuse_sp_sec_enforcer_over_limit_rejects_all_checks() {
        let profile = SecurityProfile::hardened();
        let mut enforcer = PolicyEnforcer::new(profile).with_max_violations(2);
        let _ = enforcer.check_syscall("blocked_call_1");
        let _ = enforcer.check_syscall("blocked_call_2");
        assert!(enforcer.is_over_limit());
        let result = enforcer.check_syscall("read");
        assert!(result.is_err());
        let result2 = enforcer.check_syscall("write");
        assert!(result2.is_err());
    }

    #[test]
    fn test_fuse_sp_sec_enforcer_clear_violations_resets_count() {
        let profile = SecurityProfile::default_profile();
        let mut enforcer = PolicyEnforcer::new(profile);
        for i in 0..50 {
            enforcer.record_violation(
                ViolationType::UnauthorizedSyscall(format!("s{}", i)),
                &format!("d{}", i),
            );
        }
        assert_eq!(enforcer.violation_count(), 50);
        enforcer.clear_violations();
        assert_eq!(enforcer.violation_count(), 0);
        assert!(!enforcer.is_over_limit());
    }

    #[test]
    fn test_fuse_sp_sec_violation_type_all_variants_constructible() {
        let v1 = ViolationType::UnauthorizedSyscall("execve".to_string());
        let v2 = ViolationType::CapabilityEscalation("CAP_SYS_ADMIN".to_string());
        let v3 = ViolationType::NewPrivilegesAttempt("setuid binary".to_string());
        let v4 = ViolationType::UnauthorizedMount("/evil".to_string());
        let pv1 = PolicyViolation::new(v1, "test1");
        let pv2 = PolicyViolation::new(v2, "test2");
        let pv3 = PolicyViolation::new(v3, "test3");
        let pv4 = PolicyViolation::new(v4, "test4");
        assert!(matches!(
            pv1.violation_type(),
            ViolationType::UnauthorizedSyscall(_)
        ));
        assert!(matches!(
            pv2.violation_type(),
            ViolationType::CapabilityEscalation(_)
        ));
        assert!(matches!(
            pv3.violation_type(),
            ViolationType::NewPrivilegesAttempt(_)
        ));
        assert!(matches!(
            pv4.violation_type(),
            ViolationType::UnauthorizedMount(_)
        ));
    }

    #[test]
    fn test_fuse_sp_sec_policy_violation_timestamp_is_recent() {
        let before = SystemTime::now()
            .checked_sub(Duration::from_secs(10))
            .unwrap();
        let violation = PolicyViolation::new(
            ViolationType::UnauthorizedSyscall("test".to_string()),
            "test details",
        );
        let ts = violation.timestamp();
        assert!(ts > before);
        let now = SystemTime::now();
        assert!(ts <= now);
    }

    #[test]
    fn test_fuse_sp_sec_mount_namespace_age_secs_nonnegative() {
        let ns = MountNamespace::new(12345, 999);
        std::thread::sleep(std::time::Duration::from_millis(10));
        let age = ns.age_secs();
        assert!(age < 10);
    }

    #[test]
    fn test_fuse_sp_sec_mount_namespace_preserves_ns_id_and_pid() {
        let ns = MountNamespace::new(9876543210, 123456);
        assert_eq!(ns.ns_id(), 9876543210);
        assert_eq!(ns.pid(), 123456);
    }

    #[test]
    fn test_fuse_sp_sec_violation_eviction_keeps_count_within_max() {
        let profile = SecurityProfile::default_profile();
        let mut enforcer = PolicyEnforcer::new(profile).with_max_violations(3);
        for i in 0..5 {
            enforcer.record_violation(
                ViolationType::UnauthorizedSyscall(format!("syscall_{}", i)),
                &format!("details_{}", i),
            );
        }
        assert_eq!(enforcer.violation_count(), 3);
        assert!(enforcer.violation_count() <= enforcer.max_violations());
        enforcer.record_violation(ViolationType::UnauthorizedMount("/x".to_string()), "x");
        assert_eq!(enforcer.violation_count(), 3);
        enforcer.record_violation(ViolationType::NewPrivilegesAttempt("y".to_string()), "y");
        assert_eq!(enforcer.violation_count(), 3);
    }

    #[test]
    fn test_fuse_sp_sec_security_profile_with_capabilities() {
        let mut caps = CapabilitySet::new();
        caps.add(Capability::SysAdmin);
        caps.add(Capability::NetAdmin);
        let profile = SecurityProfile::with_capabilities(caps);
        assert_eq!(profile.required_capabilities().len(), 2);
        assert!(profile
            .required_capabilities()
            .contains(&Capability::SysAdmin));
        assert!(profile
            .required_capabilities()
            .contains(&Capability::NetAdmin));
    }

    #[test]
    fn test_fuse_sp_sec_security_profile_with_syscall_policy() {
        let policy = SyscallPolicy::fuse_allowlist();
        let profile = SecurityProfile::with_syscall_policy(policy);
        assert!(profile.is_syscall_permitted("read"));
        assert!(profile.is_syscall_permitted("write"));
        assert!(!profile.is_syscall_permitted("nonexistent_syscall_xyz"));
    }

    #[test]
    fn test_fuse_sp_sec_security_profile_with_no_new_privs() {
        let profile = SecurityProfile::default_profile().with_no_new_privs(true);
        assert!(profile.enforce_no_new_privs());
        let profile2 = SecurityProfile::hardened().with_no_new_privs(false);
        assert!(!profile2.enforce_no_new_privs());
    }

    #[test]
    fn test_fuse_sp_sec_policy_enforcer_profile_accessor() {
        let profile = SecurityProfile::hardened();
        let enforcer = PolicyEnforcer::new(profile);
        assert!(enforcer
            .profile()
            .required_capabilities()
            .contains(&Capability::SysAdmin));
        assert!(enforcer.profile().enforce_no_new_privs());
    }
}
