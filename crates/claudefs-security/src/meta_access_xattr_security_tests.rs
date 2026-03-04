//! Metadata access control, xattr, and NFS file handle security tests.
//!
//! Part of A10 Phase 19: Meta access/xattr/inode-gen security audit

#[cfg(test)]
mod tests {
    use claudefs_meta::access::{
        can_create_in, can_delete_from, check_access, check_sticky_bit, AccessMode, UserContext,
    };
    use claudefs_meta::inode_gen::{Generation, InodeGenManager, NfsFileHandle};
    use claudefs_meta::kvstore::MemoryKvStore;
    use claudefs_meta::types::{
        FileType, InodeAttr, InodeId, MetaError, ReplicationState, Timestamp, VectorClock,
    };
    use claudefs_meta::xattr::XattrStore;
    use std::sync::Arc;

    fn make_file_attr(uid: u32, gid: u32, mode: u32) -> InodeAttr {
        InodeAttr {
            ino: InodeId::new(2),
            file_type: FileType::RegularFile,
            mode: 0o100000 | mode,
            nlink: 1,
            uid,
            gid,
            size: 100,
            blocks: 1,
            atime: Timestamp::now(),
            mtime: Timestamp::now(),
            ctime: Timestamp::now(),
            crtime: Timestamp::now(),
            content_hash: None,
            repl_state: ReplicationState::Local,
            vector_clock: VectorClock::new(1, 0),
            generation: 0,
            symlink_target: None,
        }
    }

    fn make_dir_attr(uid: u32, gid: u32, mode: u32) -> InodeAttr {
        InodeAttr {
            ino: InodeId::new(1),
            file_type: FileType::Directory,
            mode: 0o040000 | mode,
            nlink: 2,
            uid,
            gid,
            size: 4096,
            blocks: 8,
            atime: Timestamp::now(),
            mtime: Timestamp::now(),
            ctime: Timestamp::now(),
            crtime: Timestamp::now(),
            content_hash: None,
            repl_state: ReplicationState::Local,
            vector_clock: VectorClock::new(1, 0),
            generation: 0,
            symlink_target: None,
        }
    }

    // ============================================================================
    // Category 1: POSIX Access Control — Owner/Group/Other (5 tests)
    // ============================================================================

    #[test]
    fn test_access_root_bypass() {
        let attr = make_file_attr(1000, 1000, 0o000);
        let ctx = UserContext::root();

        // FINDING-META-ACC-01: root user bypasses all permission checks — standard POSIX behavior
        assert!(check_access(&attr, &ctx, AccessMode::R_OK).is_ok());
        assert!(check_access(&attr, &ctx, AccessMode::W_OK).is_ok());
        assert!(check_access(&attr, &ctx, AccessMode::X_OK).is_ok());
    }

    #[test]
    fn test_access_owner_permissions() {
        let attr = make_file_attr(1000, 1000, 0o744);
        let ctx = UserContext::new(1000, 1000, vec![]);

        // Owner has read, write, execute
        assert!(check_access(&attr, &ctx, AccessMode::R_OK).is_ok());
        assert!(check_access(&attr, &ctx, AccessMode::W_OK).is_ok());
        assert!(check_access(&attr, &ctx, AccessMode::X_OK).is_ok());

        // With mode 0o400 (only read), owner can read but not write/execute
        let attr2 = make_file_attr(1000, 1000, 0o400);
        assert!(check_access(&attr2, &ctx, AccessMode::R_OK).is_ok());
        assert!(matches!(
            check_access(&attr2, &ctx, AccessMode::W_OK),
            Err(MetaError::PermissionDenied)
        ));
        assert!(matches!(
            check_access(&attr2, &ctx, AccessMode::X_OK),
            Err(MetaError::PermissionDenied)
        ));
    }

    #[test]
    fn test_access_group_permissions() {
        let attr = make_file_attr(1000, 2000, 0o070);
        let ctx = UserContext::new(3000, 2000, vec![]);

        // User in primary group has rwx
        assert!(check_access(&attr, &ctx, AccessMode::R_OK).is_ok());
        assert!(check_access(&attr, &ctx, AccessMode::W_OK).is_ok());
        assert!(check_access(&attr, &ctx, AccessMode::X_OK).is_ok());

        // FINDING-META-ACC-02: supplementary groups correctly checked
        let attr2 = make_file_attr(1000, 2000, 0o070);
        let ctx2 = UserContext::new(3000, 3000, vec![2000]);
        assert!(check_access(&attr2, &ctx2, AccessMode::R_OK).is_ok());
    }

    #[test]
    fn test_access_other_permissions() {
        let attr = make_file_attr(1000, 1000, 0o604);
        let ctx = UserContext::new(2000, 2000, vec![]);

        // Other has read (6) but no write/execute
        assert!(check_access(&attr, &ctx, AccessMode::R_OK).is_ok());
        assert!(matches!(
            check_access(&attr, &ctx, AccessMode::W_OK),
            Err(MetaError::PermissionDenied)
        ));
        assert!(matches!(
            check_access(&attr, &ctx, AccessMode::X_OK),
            Err(MetaError::PermissionDenied)
        ));
    }

    #[test]
    fn test_access_no_permission_denied() {
        let attr = make_file_attr(1000, 1000, 0o700);
        let ctx = UserContext::new(2000, 2000, vec![]);

        // FINDING-META-ACC-03: non-owner correctly denied all access to owner-only file
        assert!(matches!(
            check_access(&attr, &ctx, AccessMode::R_OK),
            Err(MetaError::PermissionDenied)
        ));
        assert!(matches!(
            check_access(&attr, &ctx, AccessMode::W_OK),
            Err(MetaError::PermissionDenied)
        ));
        assert!(matches!(
            check_access(&attr, &ctx, AccessMode::X_OK),
            Err(MetaError::PermissionDenied)
        ));
    }

    // ============================================================================
    // Category 2: POSIX Access Control — Sticky Bit & Directory Ops (5 tests)
    // ============================================================================

    #[test]
    fn test_sticky_bit_owner_can_delete() {
        let parent = make_dir_attr(1000, 1000, 0o1777);
        let child = make_file_attr(2000, 2000, 0o644);
        let ctx = UserContext::new(2000, 2000, vec![]);

        // FINDING-META-ACC-04: file owner can delete own file in sticky directory
        assert!(check_sticky_bit(&parent, &child, &ctx).is_ok());
    }

    #[test]
    fn test_sticky_bit_dir_owner_can_delete() {
        let parent = make_dir_attr(1000, 1000, 0o1777);
        let child = make_file_attr(2000, 2000, 0o644);
        let ctx = UserContext::new(1000, 1000, vec![]);

        // FINDING-META-ACC-05: directory owner can delete any file in their sticky directory
        assert!(check_sticky_bit(&parent, &child, &ctx).is_ok());
    }

    #[test]
    fn test_sticky_bit_non_owner_blocked() {
        let parent = make_dir_attr(1000, 1000, 0o1777);
        let child = make_file_attr(2000, 2000, 0o644);
        let ctx = UserContext::new(3000, 3000, vec![]);

        // FINDING-META-ACC-06: non-owner correctly blocked from deleting in sticky dir
        assert!(matches!(
            check_sticky_bit(&parent, &child, &ctx),
            Err(MetaError::PermissionDenied)
        ));
    }

    #[test]
    fn test_can_create_in_directory() {
        let parent = make_dir_attr(1000, 1000, 0o755);

        // Owner can create
        let ctx = UserContext::new(1000, 1000, vec![]);
        assert!(can_create_in(&parent, &ctx).is_ok());

        // Root can create
        assert!(can_create_in(&parent, &UserContext::root()).is_ok());

        // Non-directory fails
        let not_dir = make_file_attr(1000, 1000, 0o755);
        assert!(matches!(
            can_create_in(&not_dir, &ctx),
            Err(MetaError::NotADirectory(_))
        ));
    }

    #[test]
    fn test_can_delete_from_with_sticky() {
        let parent = make_dir_attr(1000, 1000, 0o1777);
        let child = make_file_attr(2000, 2000, 0o644);

        // Child owner can delete
        let ctx_owner = UserContext::new(2000, 2000, vec![]);
        assert!(can_delete_from(&parent, &child, &ctx_owner).is_ok());

        // Non-owner cannot delete
        let ctx_other = UserContext::new(3000, 3000, vec![]);
        assert!(matches!(
            can_delete_from(&parent, &child, &ctx_other),
            Err(MetaError::PermissionDenied)
        ));

        // Root can delete
        assert!(can_delete_from(&parent, &child, &UserContext::root()).is_ok());
    }

    // ============================================================================
    // Category 3: Extended Attributes (XAttr) (5 tests)
    // ============================================================================

    #[test]
    fn test_xattr_set_and_get() {
        let store = XattrStore::new(Arc::new(MemoryKvStore::new()));
        let ino = InodeId::new(42);

        // FINDING-META-ACC-07: xattr set/get roundtrip works correctly
        store.set(ino, "user.author", b"alice").unwrap();
        assert_eq!(store.get(ino, "user.author").unwrap(), b"alice");
    }

    #[test]
    fn test_xattr_get_nonexistent() {
        let store = XattrStore::new(Arc::new(MemoryKvStore::new()));
        let ino = InodeId::new(42);

        // FINDING-META-ACC-08: missing xattr returns proper error, not empty/default
        assert!(matches!(
            store.get(ino, "user.missing"),
            Err(MetaError::EntryNotFound { .. })
        ));
    }

    #[test]
    fn test_xattr_list_and_remove() {
        let store = XattrStore::new(Arc::new(MemoryKvStore::new()));
        let ino = InodeId::new(42);

        store.set(ino, "user.author", b"alice").unwrap();
        store.set(ino, "user.project", b"claudefs").unwrap();
        store.set(ino, "claudefs.tier", b"flash").unwrap();

        let names = store.list(ino).unwrap();
        assert_eq!(names.len(), 3);

        store.remove(ino, "user.author").unwrap();
        let names = store.list(ino).unwrap();
        assert_eq!(names.len(), 2);

        assert!(matches!(
            store.get(ino, "user.author"),
            Err(MetaError::EntryNotFound { .. })
        ));
    }

    #[test]
    fn test_xattr_remove_all() {
        let store = XattrStore::new(Arc::new(MemoryKvStore::new()));
        let ino = InodeId::new(42);

        store.set(ino, "user.author", b"alice").unwrap();
        store.set(ino, "user.project", b"claudefs").unwrap();
        store.set(ino, "claudefs.tier", b"flash").unwrap();

        // FINDING-META-ACC-09: remove_all completely cleans up inode xattrs
        store.remove_all(ino).unwrap();
        let names = store.list(ino).unwrap();
        assert!(names.is_empty());
    }

    #[test]
    fn test_xattr_isolation_per_inode() {
        let store = XattrStore::new(Arc::new(MemoryKvStore::new()));

        store.set(InodeId::new(1), "user.a", b"1").unwrap();
        store.set(InodeId::new(2), "user.a", b"2").unwrap();

        // FINDING-META-ACC-10: xattr operations are correctly inode-scoped — no cross-inode leakage
        assert_eq!(store.get(InodeId::new(1), "user.a").unwrap(), b"1");
        assert_eq!(store.get(InodeId::new(2), "user.a").unwrap(), b"2");

        store.remove_all(InodeId::new(1)).unwrap();
        assert!(store.get(InodeId::new(1), "user.a").is_err());
        assert_eq!(store.get(InodeId::new(2), "user.a").unwrap(), b"2");
    }

    // ============================================================================
    // Category 4: NFS File Handle & Generation (5 tests)
    // ============================================================================

    #[test]
    fn test_generation_default_and_next() {
        assert_eq!(Generation::default().as_u64(), 1);
        assert_eq!(Generation::new(5).next().as_u64(), 6);
        assert_eq!(Generation::new(0).next().as_u64(), 1);
    }

    #[test]
    fn test_nfs_file_handle_serialization() {
        let handle = NfsFileHandle::new(InodeId::new(42), Generation::new(7));
        let bytes = handle.to_bytes();

        // FINDING-META-ACC-11: file handle serialization is deterministic and validates minimum length
        assert_eq!(bytes.len(), 16);

        let restored = NfsFileHandle::from_bytes(&bytes).unwrap();
        assert_eq!(restored.ino, InodeId::new(42));
        assert_eq!(restored.generation.as_u64(), 7);

        // Short buffer returns None
        assert!(NfsFileHandle::from_bytes(&[0u8; 8]).is_none());
    }

    #[test]
    fn test_inode_gen_allocate_and_reuse() {
        let mgr = InodeGenManager::new();

        let gen1 = mgr.allocate(InodeId::new(100));
        assert_eq!(gen1.as_u64(), 1);

        // FINDING-META-ACC-12: inode reuse correctly increments generation
        let gen2 = mgr.allocate(InodeId::new(100));
        assert_eq!(gen2.as_u64(), 2);

        // Different inode starts at 1
        let gen3 = mgr.allocate(InodeId::new(200));
        assert_eq!(gen3.as_u64(), 1);
    }

    #[test]
    fn test_inode_gen_stale_handle_detection() {
        let mgr = InodeGenManager::new();

        let gen = mgr.allocate(InodeId::new(100));
        let handle = mgr.make_handle(InodeId::new(100));

        assert!(mgr.validate_handle(&handle));

        // Mark deleted and reallocate - old handle should be stale
        mgr.mark_deleted(&InodeId::new(100));
        mgr.allocate(InodeId::new(100));

        // FINDING-META-ACC-13: stale NFS handles correctly detected after inode recycling — prevents accessing wrong file
        assert!(!mgr.validate_handle(&handle));

        // New handle passes validation
        let new_handle = mgr.make_handle(InodeId::new(100));
        assert!(mgr.validate_handle(&new_handle));
    }

    #[test]
    fn test_inode_gen_export_import() {
        let mgr = InodeGenManager::new();
        mgr.allocate(InodeId::new(100));
        mgr.allocate(InodeId::new(200));
        mgr.allocate(InodeId::new(300));

        // FINDING-META-ACC-14: generation snapshot/restore works for crash recovery
        let exported = mgr.export_generations();
        assert_eq!(exported.len(), 3);

        let mgr2 = InodeGenManager::new();
        mgr2.load_generations(exported);

        assert_eq!(mgr2.tracked_count(), 3);
        assert_eq!(mgr2.get(&InodeId::new(100)), Some(Generation::new(1)));
    }

    // ============================================================================
    // Category 5: Integration & Edge Cases (5 tests)
    // ============================================================================

    #[test]
    fn test_access_mode_flags() {
        assert_eq!(AccessMode::F_OK.0, 0);
        assert_eq!(AccessMode::R_OK.0, 4);
        assert_eq!(AccessMode::W_OK.0, 2);
        assert_eq!(AccessMode::X_OK.0, 1);

        assert!(AccessMode::R_OK.has_read());
        assert!(!AccessMode::R_OK.has_write());

        let rw = AccessMode(AccessMode::R_OK.0 | AccessMode::W_OK.0);
        assert!(rw.has_read() && rw.has_write());
    }

    #[test]
    fn test_user_context_supplementary_groups() {
        let ctx = UserContext::new(1000, 1000, vec![2000, 3000]);

        assert!(!ctx.is_root());
        assert!(ctx.in_group(1000)); // primary group
        assert!(ctx.in_group(2000)); // supplementary
        assert!(ctx.in_group(3000)); // supplementary
        assert!(!ctx.in_group(4000)); // not a member
    }

    #[test]
    fn test_xattr_overwrite() {
        let store = XattrStore::new(Arc::new(MemoryKvStore::new()));
        let ino = InodeId::new(42);

        store.set(ino, "user.v", b"v1").unwrap();
        store.set(ino, "user.v", b"v2").unwrap();

        // FINDING-META-ACC-15: xattr overwrite replaces value atomically
        assert_eq!(store.get(ino, "user.v").unwrap(), b"v2");
    }

    #[test]
    fn test_inode_gen_clear_and_tracked() {
        let mgr = InodeGenManager::new();
        mgr.allocate(InodeId::new(100));
        mgr.allocate(InodeId::new(101));
        mgr.allocate(InodeId::new(102));
        mgr.allocate(InodeId::new(103));
        mgr.allocate(InodeId::new(104));

        assert_eq!(mgr.tracked_count(), 5);

        // FINDING-META-ACC-16: clear properly resets all tracking state
        mgr.clear();
        assert_eq!(mgr.tracked_count(), 0);
        assert!(mgr.get(&InodeId::new(100)).is_none());
    }

    #[test]
    fn test_nfs_handle_unknown_inode() {
        let mgr = InodeGenManager::new();

        // FINDING-META-ACC-17: handles for unknown inodes are safely handled
        let handle = NfsFileHandle::new(InodeId::new(999), Generation::default());
        assert!(!mgr.validate_handle(&handle));

        // Make handle for unallocated inode returns default generation
        let new_handle = mgr.make_handle(InodeId::new(999));
        assert_eq!(new_handle.generation.as_u64(), 1); // default
    }
}
