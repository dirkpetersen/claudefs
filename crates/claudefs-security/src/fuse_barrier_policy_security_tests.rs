//! FUSE fsync barrier, security policy, capability, and file attribute security tests.
//!
//! Part of A10 Phase 16: FUSE barrier & security policy audit

#[cfg(test)]
mod tests {
    use claudefs_fuse::attr::{FileAttr, FileType};
    use claudefs_fuse::fsync_barrier::{
        BarrierId, BarrierKind, BarrierManager, BarrierState, FsyncJournal, FsyncMode,
        JournalEntry as FsyncJournalEntry, WriteBarrier,
    };
    use claudefs_fuse::inode::InodeKind;
    use claudefs_fuse::sec_policy::{
        Capability, CapabilitySet, MountNamespace, PolicyEnforcer, PolicyViolation, SeccompMode,
        SecurityProfile, SyscallPolicy, ViolationType,
    };
    use std::time::{SystemTime, UNIX_EPOCH};

    fn make_barrier_id(id: u64) -> BarrierId {
        BarrierId::new(id)
    }

    fn make_write_barrier(id: u64, inode: u64, kind: BarrierKind, sequence: u64) -> WriteBarrier {
        WriteBarrier::new(BarrierId::new(id), inode, kind, sequence)
    }

    fn make_fsync_journal(max_entries: usize) -> FsyncJournal {
        FsyncJournal::new(max_entries)
    }

    fn make_barrier_manager() -> BarrierManager {
        BarrierManager::new(FsyncMode::default())
    }

    fn make_capability_set() -> CapabilitySet {
        CapabilitySet::new()
    }

    fn make_security_profile() -> SecurityProfile {
        SecurityProfile::default_profile()
    }

    fn make_hardened_profile() -> SecurityProfile {
        SecurityProfile::hardened()
    }

    fn make_policy_enforcer() -> PolicyEnforcer {
        PolicyEnforcer::new(SecurityProfile::hardened())
    }

    fn make_mount_namespace(ns_id: u64, pid: u32) -> MountNamespace {
        MountNamespace::new(ns_id, pid)
    }

    // ============================================================================
    // Category 1: Write Barrier State Machine (5 tests)
    // ============================================================================

    #[test]
    fn test_barrier_state_transitions() {
        let barrier_id = BarrierId::new(1);
        let mut barrier = WriteBarrier::new(barrier_id, 100, BarrierKind::DataAndMetadata, 1);

        assert!(matches!(barrier.state(), BarrierState::Pending));
        assert!(barrier.is_pending());
        assert!(!barrier.is_complete());

        barrier.mark_flushing();
        assert!(matches!(barrier.state(), BarrierState::Flushing));
        assert!(barrier.is_pending());
        assert!(!barrier.is_complete());

        barrier.mark_committed();
        assert!(matches!(barrier.state(), BarrierState::Committed));
        assert!(barrier.is_complete());
        assert!(!barrier.is_pending());
    }

    #[test]
    fn test_barrier_failure_path() {
        let barrier_id = BarrierId::new(1);
        let mut barrier = WriteBarrier::new(barrier_id, 100, BarrierKind::DataOnly, 1);

        barrier.mark_flushing();
        barrier.mark_failed("disk error");

        assert!(matches!(barrier.state(), BarrierState::Failed(msg) if msg == "disk error"));
        assert!(barrier.is_complete());
        assert!(!barrier.is_pending());
    }

    #[test]
    fn test_barrier_manager_create_and_flush() {
        let mut manager = BarrierManager::new(FsyncMode::default());
        let id1 = manager.create_barrier(100, BarrierKind::DataOnly);
        let id2 = manager.create_barrier(200, BarrierKind::MetadataOnly);
        let id3 = manager.create_barrier(300, BarrierKind::DataAndMetadata);

        manager.flush_barrier(&id1).unwrap();
        manager.commit_barrier(&id2).unwrap();
        manager.fail_barrier(&id3, "test failure").unwrap();

        let pending = manager.pending_barriers();
        assert_eq!(pending.len(), 1);
        assert_eq!(manager.committed_count(), 1);
        assert_eq!(manager.failed_count(), 1);
    }

    #[test]
    fn test_barrier_manager_invalid_id() {
        let mut manager = BarrierManager::new(FsyncMode::default());
        let invalid_id = BarrierId::new(999);

        let result = manager.flush_barrier(&invalid_id);
        assert!(result.is_err());

        let result = manager.commit_barrier(&invalid_id);
        assert!(result.is_err());

        let result = manager.fail_barrier(&invalid_id, "test");
        assert!(result.is_err());
    }

    #[test]
    fn test_barrier_id_display() {
        let id = BarrierId::new(42);
        assert_eq!(format!("{}", id), "barrier:42");

        let id2 = BarrierId::new(0);
        assert_eq!(format!("{}", id2), "barrier:0");
    }

    // ============================================================================
    // Category 2: Fsync Journal (5 tests)
    // ============================================================================

    #[test]
    fn test_journal_append_and_commit() {
        let mut journal = FsyncJournal::new(10);
        let _ = journal.append(1, "write", 1).unwrap();
        let _ = journal.append(2, "write", 1).unwrap();
        let _ = journal.append(3, "fsync", 2).unwrap();
        let _ = journal.append(4, "write", 3).unwrap();

        assert_eq!(journal.pending_count(), 4);

        journal.commit_up_to(2);
        assert_eq!(journal.pending_count(), 2);

        let entries: Vec<_> = journal.entries().iter().map(|e| e.entry_id()).collect();
        assert!(entries.iter().all(|&id| id > 2));
    }

    #[test]
    fn test_journal_full_rejection() {
        let mut journal = FsyncJournal::new(3);
        let _ = journal.append(1, "write", 1).unwrap();
        let _ = journal.append(1, "write", 2).unwrap();
        let _ = journal.append(1, "write", 3).unwrap();

        assert!(journal.is_full());

        let result = journal.append(1, "write", 4);
        assert!(result.is_err());
    }

    #[test]
    fn test_journal_entries_for_inode() {
        let mut journal = FsyncJournal::new(10);
        let _ = journal.append(1, "write", 1).unwrap();
        let _ = journal.append(2, "write", 1).unwrap();
        let _ = journal.append(1, "fsync", 2).unwrap();
        let _ = journal.append(3, "write", 1).unwrap();
        let _ = journal.append(1, "write", 3).unwrap();

        let inode1_entries = journal.entries_for_inode(1);
        assert_eq!(inode1_entries.len(), 3);

        let inode99_entries = journal.entries_for_inode(99);
        assert!(inode99_entries.is_empty());
    }

    #[test]
    fn test_barrier_manager_record_fsync() {
        let mut manager = BarrierManager::new(FsyncMode::default());
        let entry_id = manager.record_fsync(100, 5).unwrap();

        assert_eq!(entry_id, 1);
        assert_eq!(manager.journal().pending_count(), 1);

        manager.journal_mut().commit_up_to(1);
        assert_eq!(manager.journal().pending_count(), 0);
    }

    #[test]
    fn test_fsync_mode_default() {
        let mode = FsyncMode::default();
        assert!(matches!(mode, FsyncMode::Ordered { max_delay_ms: 100 }));

        let _ = FsyncMode::Sync;
        let _ = FsyncMode::Async;
    }

    // ============================================================================
    // Category 3: Capability Set Security (5 tests)
    // ============================================================================

    #[test]
    fn test_capability_set_fuse_minimal() {
        let caps = CapabilitySet::fuse_minimal();
        assert!(caps.contains(&Capability::SysAdmin));
        assert_eq!(caps.len(), 1);
        assert!(!caps.contains(&Capability::DacOverride));
        assert!(!caps.contains(&Capability::NetAdmin));
    }

    #[test]
    fn test_capability_set_add_remove() {
        let mut caps = CapabilitySet::new();
        caps.add(Capability::SysAdmin);
        caps.add(Capability::NetAdmin);
        caps.add(Capability::Chown);
        assert_eq!(caps.len(), 3);

        caps.add(Capability::SysAdmin);
        assert_eq!(caps.len(), 3);

        let removed = caps.remove(Capability::SysAdmin);
        assert!(removed);
        assert_eq!(caps.len(), 2);

        let removed = caps.remove(Capability::SysAdmin);
        assert!(!removed);
    }

    #[test]
    fn test_capability_set_contains() {
        let mut caps = CapabilitySet::new();
        caps.add(Capability::DacOverride);
        caps.add(Capability::FOwner);

        assert!(caps.contains(&Capability::DacOverride));
        assert!(caps.contains(&Capability::FOwner));
        assert!(!caps.contains(&Capability::SysAdmin));
    }

    #[test]
    fn test_security_profile_hardened() {
        let profile = SecurityProfile::hardened();
        assert!(profile
            .required_capabilities()
            .contains(&Capability::SysAdmin));
        assert!(profile.enforce_no_new_privs());
        assert!(profile.is_syscall_permitted("read"));
        assert!(!profile.is_syscall_permitted("totally_fake_syscall"));
    }

    #[test]
    fn test_security_profile_default_permissive() {
        let profile = SecurityProfile::default_profile();
        assert!(profile.required_capabilities().is_empty());
        assert!(!profile.enforce_no_new_privs());
        assert!(profile.mount_ns().is_none());
    }

    // ============================================================================
    // Category 4: Syscall Policy & Enforcement (5 tests)
    // ============================================================================

    #[test]
    fn test_syscall_policy_fuse_allowlist() {
        let policy = SyscallPolicy::fuse_allowlist();
        assert!(policy.is_allowed("read"));
        assert!(policy.is_allowed("write"));
        assert!(policy.is_allowed("io_uring_enter"));
        assert!(policy.is_allowed("io_uring_setup"));
        assert!(matches!(policy.mode(), SeccompMode::Enforce));
    }

    #[test]
    fn test_policy_enforcer_blocks_unauthorized() {
        let mut enforcer = PolicyEnforcer::new(SecurityProfile::hardened());

        let result = enforcer.check_syscall("read");
        assert!(result.is_ok());

        let result = enforcer.check_syscall("nonexistent_evil_syscall");
        assert!(result.is_err());

        assert_eq!(enforcer.violation_count(), 1);
    }

    #[test]
    fn test_policy_enforcer_violation_limit() {
        let mut enforcer =
            PolicyEnforcer::new(SecurityProfile::default_profile()).with_max_violations(3);

        for i in 0..5 {
            enforcer.record_violation(
                ViolationType::UnauthorizedSyscall("test".to_string()),
                &format!("Violation {}", i),
            );
        }

        assert!(enforcer.is_over_limit());

        let result = enforcer.check_syscall("read");
        assert!(result.is_err());

        enforcer.clear_violations();
        assert_eq!(enforcer.violation_count(), 0);
        assert!(!enforcer.is_over_limit());
    }

    #[test]
    fn test_policy_enforcer_recent_violations() {
        let mut enforcer = PolicyEnforcer::new(SecurityProfile::default_profile());

        for i in 0..5 {
            enforcer.record_violation(
                ViolationType::UnauthorizedSyscall(format!("syscall{}", i)),
                &format!("Violation {}", i),
            );
        }

        let recent = enforcer.recent_violations(3);
        assert_eq!(recent.len(), 3);

        let all = enforcer.recent_violations(10);
        assert_eq!(all.len(), 5);
    }

    #[test]
    fn test_mount_namespace_creation() {
        let ns = MountNamespace::new(12345, 67890);
        assert_eq!(ns.ns_id(), 12345);
        assert_eq!(ns.pid(), 67890);
        assert!(ns.age_secs() < 2);

        let profile = SecurityProfile::default_profile().with_mount_namespace(ns.clone());
        assert!(profile.mount_ns().is_some());
    }

    // ============================================================================
    // Category 5: File Attributes & Edge Cases (5 tests)
    // ============================================================================

    #[test]
    fn test_file_attr_new_file() {
        let attr = FileAttr::new_file(2, 1000, 0o644, 1000, 1000);
        assert_eq!(attr.kind, FileType::RegularFile);
        assert_eq!(attr.ino, 2);
        assert_eq!(attr.size, 1000);
        assert_eq!(attr.nlink, 1);
        assert_eq!(attr.blksize, 4096);
    }

    #[test]
    fn test_file_attr_new_dir() {
        let attr = FileAttr::new_dir(3, 0o755, 0, 0);
        assert_eq!(attr.kind, FileType::Directory);
        assert_eq!(attr.nlink, 2);
        assert_eq!(attr.size, 4096);
    }

    #[test]
    fn test_file_attr_new_symlink() {
        let attr = FileAttr::new_symlink(4, 20, 500, 500);
        assert_eq!(attr.kind, FileType::Symlink);
        assert_eq!(attr.size, 20);
        assert_eq!(attr.perm, 0o777);
    }

    #[test]
    fn test_file_type_variants() {
        assert_eq!(FileType::RegularFile, FileType::RegularFile);
        assert_eq!(FileType::Directory, FileType::Directory);
        assert_eq!(FileType::Symlink, FileType::Symlink);
        assert_eq!(FileType::BlockDevice, FileType::BlockDevice);
        assert_eq!(FileType::CharDevice, FileType::CharDevice);
        assert_eq!(FileType::NamedPipe, FileType::NamedPipe);
        assert_eq!(FileType::Socket, FileType::Socket);

        assert_ne!(FileType::RegularFile, FileType::Directory);
    }

    #[test]
    fn test_violation_types() {
        let v1 = PolicyViolation::new(
            ViolationType::UnauthorizedSyscall("execve".to_string()),
            "Attempted execve",
        );
        assert!(matches!(
            v1.violation_type(),
            ViolationType::UnauthorizedSyscall(_)
        ));
        assert_eq!(v1.details(), "Attempted execve");

        let v2 = PolicyViolation::new(
            ViolationType::CapabilityEscalation("CAP_SYS_ADMIN".to_string()),
            "Tried to gain admin",
        );
        assert!(matches!(
            v2.violation_type(),
            ViolationType::CapabilityEscalation(_)
        ));

        let v3 = PolicyViolation::new(
            ViolationType::NewPrivilegesAttempt("setuid".to_string()),
            "Tried setuid",
        );
        assert!(matches!(
            v3.violation_type(),
            ViolationType::NewPrivilegesAttempt(_)
        ));

        let v4 = PolicyViolation::new(
            ViolationType::UnauthorizedMount("/evil".to_string()),
            "Unauthorized mount",
        );
        assert!(matches!(
            v4.violation_type(),
            ViolationType::UnauthorizedMount(_)
        ));

        let now = SystemTime::now();
        let earliest = UNIX_EPOCH;
        assert!(v1.timestamp() > earliest && v1.timestamp() <= now);
    }
}
