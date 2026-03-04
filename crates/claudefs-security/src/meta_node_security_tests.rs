//! Meta node security tests.
//!
//! Part of A10 Phase: MetadataNode unified server security audit.

#[cfg(test)]
mod tests {
    use claudefs_meta::node::{MetadataNode, MetadataNodeConfig};
    use claudefs_meta::types::*;
    use claudefs_meta::worm::{RetentionPolicy, WormState};

    fn make_node() -> MetadataNode {
        MetadataNode::new(MetadataNodeConfig::default()).unwrap()
    }

    // ============================================================================
    // Category 1: Node Initialization Security (4 tests)
    // ============================================================================

    #[test]
    fn test_meta_node_sec_default_config_creates_valid_node() {
        let node = make_node();
        assert_eq!(node.node_id(), NodeId::new(1));
        assert_eq!(node.num_shards(), 256);
        assert!(node.is_healthy());
    }

    #[test]
    fn test_meta_node_sec_accessors_return_correct_values() {
        let node = make_node();
        assert_eq!(node.node_id(), NodeId::new(1));
        assert_eq!(node.site_id(), 1);
        assert_eq!(node.num_shards(), 256);
    }

    #[test]
    fn test_meta_node_sec_next_inode_id_starts_at_one() {
        let node = make_node();
        assert_eq!(node.next_inode_id(), 1);
    }

    #[test]
    fn test_meta_node_sec_in_memory_kvstore_works() {
        let config = MetadataNodeConfig {
            data_dir: None,
            ..MetadataNodeConfig::default()
        };
        let node = MetadataNode::new(config).unwrap();
        let _ = node.kv_store();
        assert!(node.is_healthy());
    }

    // ============================================================================
    // Category 2: File Operations Security (5 tests)
    // ============================================================================

    #[test]
    fn test_meta_node_sec_create_file_on_root_returns_valid_attr() {
        let node = make_node();
        let attr = node
            .create_file(InodeId::ROOT_INODE, "test.txt", 1000, 1000, 0o644)
            .unwrap();
        assert_eq!(attr.file_type, FileType::RegularFile);
        assert_eq!(attr.uid, 1000);
        assert_eq!(attr.gid, 1000);
        assert_eq!(attr.mode, 0o644);
    }

    #[test]
    fn test_meta_node_sec_create_file_same_name_returns_error() {
        let node = make_node();
        let _ = node
            .create_file(InodeId::ROOT_INODE, "test.txt", 1000, 1000, 0o644)
            .unwrap();
        let result = node.create_file(InodeId::ROOT_INODE, "test.txt", 1000, 1000, 0o644);
        assert!(result.is_err());
        match result {
            Err(MetaError::EntryExists { parent, name }) => {
                assert_eq!(parent, InodeId::ROOT_INODE);
                assert_eq!(name, "test.txt");
            }
            _ => panic!("Expected EntryExists error"),
        }
    }

    #[test]
    fn test_meta_node_sec_lookup_after_create_returns_file() {
        let node = make_node();
        let attr = node
            .create_file(InodeId::ROOT_INODE, "test.txt", 1000, 1000, 0o644)
            .unwrap();
        let entry = node.lookup(InodeId::ROOT_INODE, "test.txt").unwrap();
        assert_eq!(entry.name, "test.txt");
        assert_eq!(entry.ino, attr.ino);
        assert_eq!(entry.file_type, FileType::RegularFile);
    }

    #[test]
    fn test_meta_node_sec_unlink_removes_file() {
        let node = make_node();
        let _ = node
            .create_file(InodeId::ROOT_INODE, "test.txt", 1000, 1000, 0o644)
            .unwrap();
        node.unlink(InodeId::ROOT_INODE, "test.txt").unwrap();
        let result = node.lookup(InodeId::ROOT_INODE, "test.txt");
        assert!(result.is_err());
        match result {
            Err(MetaError::EntryNotFound { parent, name }) => {
                assert_eq!(parent, InodeId::ROOT_INODE);
                assert_eq!(name, "test.txt");
            }
            _ => panic!("Expected EntryNotFound error"),
        }
    }

    #[test]
    fn test_meta_node_sec_getattr_after_create_returns_valid_attrs() {
        let node = make_node();
        let created = node
            .create_file(InodeId::ROOT_INODE, "test.txt", 1000, 1000, 0o644)
            .unwrap();
        let attr = node.getattr(created.ino).unwrap();
        assert_eq!(attr.ino, created.ino);
        assert_eq!(attr.file_type, FileType::RegularFile);
        assert_eq!(attr.uid, 1000);
        assert_eq!(attr.gid, 1000);
        assert_eq!(attr.mode, 0o644);
    }

    // ============================================================================
    // Category 3: Directory Operations Security (5 tests)
    // ============================================================================

    #[test]
    fn test_meta_node_sec_mkdir_creates_directory() {
        let node = make_node();
        let attr = node
            .mkdir(InodeId::ROOT_INODE, "testdir", 1000, 1000, 0o755)
            .unwrap();
        assert_eq!(attr.file_type, FileType::Directory);
        assert_eq!(attr.uid, 1000);
        assert_eq!(attr.gid, 1000);
        assert_eq!(attr.mode, 0o755);
    }

    #[test]
    fn test_meta_node_sec_readdir_root_returns_expected_entries() {
        let node = make_node();
        let _ = node
            .create_file(InodeId::ROOT_INODE, "file1.txt", 1000, 1000, 0o644)
            .unwrap();
        let _ = node
            .mkdir(InodeId::ROOT_INODE, "dir1", 1000, 1000, 0o755)
            .unwrap();
        let entries = node.readdir(InodeId::ROOT_INODE).unwrap();
        assert!(entries.len() >= 2);
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"file1.txt"));
        assert!(names.contains(&"dir1"));
    }

    #[test]
    fn test_meta_node_sec_rmdir_empty_dir_succeeds() {
        let node = make_node();
        let _ = node
            .mkdir(InodeId::ROOT_INODE, "emptydir", 1000, 1000, 0o755)
            .unwrap();
        node.rmdir(InodeId::ROOT_INODE, "emptydir").unwrap();
        let result = node.lookup(InodeId::ROOT_INODE, "emptydir");
        assert!(result.is_err());
    }

    #[test]
    fn test_meta_node_sec_rmdir_nonempty_dir_returns_error() {
        let node = make_node();
        let dir = node
            .mkdir(InodeId::ROOT_INODE, "nonemptydir", 1000, 1000, 0o755)
            .unwrap();
        let _ = node.create_file(dir.ino, "file.txt", 1000, 1000, 0o644);
        let result = node.rmdir(InodeId::ROOT_INODE, "nonemptydir");
        assert!(result.is_err());
        match result {
            Err(MetaError::DirectoryNotEmpty(ino)) => {
                assert_eq!(ino, dir.ino);
            }
            _ => panic!("Expected DirectoryNotEmpty error"),
        }
    }

    #[test]
    fn test_meta_node_sec_mkdir_same_name_twice_returns_error() {
        let node = make_node();
        let _ = node
            .mkdir(InodeId::ROOT_INODE, "testdir", 1000, 1000, 0o755)
            .unwrap();
        let result = node.mkdir(InodeId::ROOT_INODE, "testdir", 1000, 1000, 0o755);
        assert!(result.is_err());
        match result {
            Err(MetaError::EntryExists { parent, name }) => {
                assert_eq!(parent, InodeId::ROOT_INODE);
                assert_eq!(name, "testdir");
            }
            _ => panic!("Expected EntryExists error"),
        }
    }

    // ============================================================================
    // Category 4: Rename Security (4 tests)
    // ============================================================================

    #[test]
    fn test_meta_node_sec_rename_within_same_parent_succeeds() {
        let node = make_node();
        let _ = node
            .create_file(InodeId::ROOT_INODE, "old.txt", 1000, 1000, 0o644)
            .unwrap();
        node.rename(
            InodeId::ROOT_INODE,
            "old.txt",
            InodeId::ROOT_INODE,
            "new.txt",
        )
        .unwrap();
        let entry = node.lookup(InodeId::ROOT_INODE, "new.txt").unwrap();
        assert_eq!(entry.name, "new.txt");
    }

    #[test]
    fn test_meta_node_sec_rename_nonexistent_src_returns_error() {
        let node = make_node();
        let result = node.rename(
            InodeId::ROOT_INODE,
            "nonexistent.txt",
            InodeId::ROOT_INODE,
            "newfile.txt",
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_meta_node_sec_lookup_after_rename_reflects_new_name() {
        let node = make_node();
        let attr = node
            .create_file(InodeId::ROOT_INODE, "old.txt", 1000, 1000, 0o644)
            .unwrap();
        node.rename(
            InodeId::ROOT_INODE,
            "old.txt",
            InodeId::ROOT_INODE,
            "new.txt",
        )
        .unwrap();
        let entry = node.lookup(InodeId::ROOT_INODE, "new.txt").unwrap();
        assert_eq!(entry.name, "new.txt");
        assert_eq!(entry.ino, attr.ino);
    }

    #[test]
    fn test_meta_node_sec_lookup_after_rename_old_not_found() {
        let node = make_node();
        let _ = node
            .create_file(InodeId::ROOT_INODE, "old.txt", 1000, 1000, 0o644)
            .unwrap();
        node.rename(
            InodeId::ROOT_INODE,
            "old.txt",
            InodeId::ROOT_INODE,
            "new.txt",
        )
        .unwrap();
        let result = node.lookup(InodeId::ROOT_INODE, "old.txt");
        assert!(result.is_err());
    }

    // ============================================================================
    // Category 5: Handle and Symlink Security (5 tests)
    // ============================================================================

    #[test]
    fn test_meta_node_sec_open_returns_file_handle_gt_zero() {
        let node = make_node();
        let attr = node
            .create_file(InodeId::ROOT_INODE, "test.txt", 1000, 1000, 0o644)
            .unwrap();
        let fh = node.open(attr.ino, 1, 0x01).unwrap();
        assert!(fh > 0);
    }

    #[test]
    fn test_meta_node_sec_close_valid_handle_succeeds() {
        let node = make_node();
        let attr = node
            .create_file(InodeId::ROOT_INODE, "test.txt", 1000, 1000, 0o644)
            .unwrap();
        let fh = node.open(attr.ino, 1, 0x01).unwrap();
        let result = node.close(fh);
        assert!(result.is_ok());
    }

    #[test]
    fn test_meta_node_sec_close_invalid_handle_returns_error() {
        let node = make_node();
        let result = node.close(99999);
        assert!(result.is_err());
    }

    #[test]
    fn test_meta_node_sec_symlink_creates_entry() {
        let node = make_node();
        let attr = node
            .symlink(InodeId::ROOT_INODE, "mylink", "/target/path", 1000, 1000)
            .unwrap();
        assert_eq!(attr.file_type, FileType::Symlink);
        assert_eq!(attr.symlink_target, Some("/target/path".to_string()));
    }

    #[test]
    fn test_meta_node_sec_symlink_lookup_returns_correct_type() {
        let node = make_node();
        let _ = node
            .symlink(InodeId::ROOT_INODE, "mylink", "/target", 1000, 1000)
            .unwrap();
        let entry = node.lookup(InodeId::ROOT_INODE, "mylink").unwrap();
        assert_eq!(entry.file_type, FileType::Symlink);
    }

    // ============================================================================
    // Category 6: setattr and WORM Protection (5 tests)
    // ============================================================================

    #[test]
    fn test_meta_node_sec_setattr_updates_attrs() {
        let node = make_node();
        let mut attr = node
            .create_file(InodeId::ROOT_INODE, "test.txt", 1000, 1000, 0o644)
            .unwrap();
        attr.mode = 0o600;
        attr.uid = 2000;
        node.setattr(attr.ino, attr.clone()).unwrap();
        let updated = node.getattr(attr.ino).unwrap();
        assert_eq!(updated.mode, 0o600);
        assert_eq!(updated.uid, 2000);
    }

    #[test]
    fn test_meta_node_sec_setattr_nonexistent_inode_returns_error() {
        let node = make_node();
        let attr = InodeAttr::new_file(InodeId::new(99999), 1000, 1000, 0o644, 1);
        let result = node.setattr(InodeId::new(99999), attr);
        assert!(result.is_err());
    }

    #[test]
    fn test_meta_node_sec_worm_blocks_setattr() {
        let node = make_node();
        let attr = node
            .create_file(InodeId::ROOT_INODE, "wormfile.txt", 1000, 1000, 0o644)
            .unwrap();
        let policy = RetentionPolicy::new(3600, None, false);
        node.worm_manager()
            .set_retention_policy(attr.ino, policy, 1000);
        node.worm_manager().lock_file(attr.ino, 1000).unwrap();
        let state = node.worm_manager().get_state(attr.ino).unwrap();
        assert!(state.is_protected());
        let mut updated_attr = attr.clone();
        updated_attr.mode = 0o777;
        let result = node.setattr(attr.ino, updated_attr);
        assert!(result.is_err());
        match result {
            Err(MetaError::PermissionDenied) => {}
            _ => panic!("Expected PermissionDenied error"),
        }
    }

    #[test]
    fn test_meta_node_sec_worm_blocks_unlink() {
        let node = make_node();
        let attr = node
            .create_file(InodeId::ROOT_INODE, "wormfile.txt", 1000, 1000, 0o644)
            .unwrap();
        let policy = RetentionPolicy::new(3600, None, false);
        node.worm_manager()
            .set_retention_policy(attr.ino, policy, 1000);
        node.worm_manager().lock_file(attr.ino, 1000).unwrap();
        let result = node.unlink(InodeId::ROOT_INODE, "wormfile.txt");
        assert!(result.is_err());
        match result {
            Err(MetaError::PermissionDenied) => {}
            _ => panic!("Expected PermissionDenied error"),
        }
    }

    #[test]
    fn test_meta_node_sec_worm_blocks_rmdir() {
        let node = make_node();
        let dir = node
            .mkdir(InodeId::ROOT_INODE, "wormdir", 1000, 1000, 0o755)
            .unwrap();
        let policy = RetentionPolicy::new(3600, None, false);
        node.worm_manager()
            .set_retention_policy(dir.ino, policy, 1000);
        node.worm_manager().lock_file(dir.ino, 1000).unwrap();
        let result = node.rmdir(InodeId::ROOT_INODE, "wormdir");
        assert!(result.is_err());
        match result {
            Err(MetaError::PermissionDenied) => {}
            _ => panic!("Expected PermissionDenied error"),
        }
    }
}
