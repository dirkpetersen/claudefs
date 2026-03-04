//! Gateway SMB3 protocol security tests.
//!
//! Part of A10 Phase 26: Gateway SMB3 security audit

#[cfg(test)]
mod tests {
    use claudefs_gateway::error::GatewayError;
    use claudefs_gateway::smb::{
        OpenFlags, SmbAuthInfo, SmbDirEntry, SmbFileId, SmbFileStat, SmbSessionId, SmbTreeId,
        SmbVfsOps, SmbVfsStub,
    };
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    fn make_test_auth(session_id: u64) -> SmbAuthInfo {
        SmbAuthInfo {
            session_id: SmbSessionId(session_id),
            uid: 1000,
            gid: 1000,
            supplementary_gids: vec![],
            username: "testuser".to_string(),
            domain: "TESTDOMAIN".to_string(),
        }
    }

    #[test]
    fn test_smb_sec_session_id_zero() {
        let id = SmbSessionId(0);
        assert_eq!(id.0, 0u64);
    }

    #[test]
    fn test_smb_sec_session_id_max() {
        let id = SmbSessionId(u64::MAX);
        assert_eq!(id.0, u64::MAX);
    }

    #[test]
    fn test_smb_sec_session_id_hashable() {
        let id1 = SmbSessionId(42);
        let id2 = SmbSessionId(42);
        let id3 = SmbSessionId(43);

        let mut hasher1 = DefaultHasher::new();
        let mut hasher2 = DefaultHasher::new();
        let mut hasher3 = DefaultHasher::new();

        id1.hash(&mut hasher1);
        id2.hash(&mut hasher2);
        id3.hash(&mut hasher3);

        assert_eq!(hasher1.finish(), hasher2.finish());
        assert_ne!(hasher1.finish(), hasher3.finish());
    }

    #[test]
    fn test_smb_sec_tree_id_max() {
        let tree_id = SmbTreeId(u32::MAX);
        assert_eq!(tree_id.0, u32::MAX);
    }

    #[test]
    fn test_smb_sec_auth_root_uid() {
        let auth = SmbAuthInfo {
            session_id: SmbSessionId(1),
            uid: 0,
            gid: 0,
            supplementary_gids: vec![0],
            username: "root".to_string(),
            domain: "LOCAL".to_string(),
        };
        assert_eq!(auth.uid, 0);
        assert_eq!(auth.username, "root");
    }

    #[test]
    fn test_smb_sec_auth_empty_username() {
        let auth = SmbAuthInfo {
            session_id: SmbSessionId(1),
            uid: 1000,
            gid: 1000,
            supplementary_gids: vec![],
            username: "".to_string(),
            domain: "TEST".to_string(),
        };
        assert!(auth.username.is_empty());
    }

    #[test]
    fn test_smb_sec_auth_empty_domain() {
        let auth = SmbAuthInfo {
            session_id: SmbSessionId(1),
            uid: 1000,
            gid: 1000,
            supplementary_gids: vec![],
            username: "test".to_string(),
            domain: "".to_string(),
        };
        assert!(auth.domain.is_empty());
    }

    #[test]
    fn test_smb_sec_auth_large_gids() {
        let large_gids: Vec<u32> = (0..1000).collect();
        let auth = SmbAuthInfo {
            session_id: SmbSessionId(1),
            uid: 1000,
            gid: 1000,
            supplementary_gids: large_gids.clone(),
            username: "test".to_string(),
            domain: "TEST".to_string(),
        };
        assert_eq!(auth.supplementary_gids.len(), 1000);
        assert_eq!(auth.supplementary_gids[0], 0);
        assert_eq!(auth.supplementary_gids[999], 999);
    }

    #[test]
    fn test_smb_sec_flags_conflicting() {
        let flags = OpenFlags::new(true, true, true, true, true);
        assert!(flags.create && flags.exclusive && flags.truncate);
    }

    #[test]
    fn test_smb_sec_flags_all_true() {
        let flags = OpenFlags::new(true, true, true, true, true);
        assert!(flags.read);
        assert!(flags.write);
        assert!(flags.create);
        assert!(flags.truncate);
        assert!(flags.exclusive);
    }

    #[test]
    fn test_smb_sec_flags_all_false() {
        let flags = OpenFlags::new(false, false, false, false, false);
        assert!(!flags.read);
        assert!(!flags.write);
        assert!(!flags.create);
        assert!(!flags.truncate);
        assert!(!flags.exclusive);
    }

    #[test]
    fn test_smb_sec_flags_write_no_create() {
        let flags = OpenFlags::new(false, true, false, false, false);
        assert!(flags.write);
        assert!(!flags.create);
    }

    #[test]
    fn test_smb_sec_stat_max_size() {
        let stat = SmbFileStat {
            size: u64::MAX,
            uid: 0,
            gid: 0,
            mode: 0o777,
            inode: 1,
            atime_ns: 0,
            mtime_ns: 0,
            ctime_ns: 0,
        };
        assert_eq!(stat.size, u64::MAX);
    }

    #[test]
    fn test_smb_sec_stat_zero_fields() {
        let stat = SmbFileStat {
            size: 0,
            uid: 0,
            gid: 0,
            mode: 0,
            inode: 0,
            atime_ns: 0,
            mtime_ns: 0,
            ctime_ns: 0,
        };
        assert_eq!(stat.size, 0);
        assert_eq!(stat.uid, 0);
        assert_eq!(stat.mode, 0);
    }

    #[test]
    fn test_smb_sec_direntry_empty_name() {
        let entry = SmbDirEntry {
            name: "".to_string(),
            stat: SmbFileStat {
                size: 0,
                uid: 0,
                gid: 0,
                mode: 0,
                inode: 0,
                atime_ns: 0,
                mtime_ns: 0,
                ctime_ns: 0,
            },
        };
        assert!(entry.name.is_empty());
    }

    #[test]
    fn test_smb_sec_stub_all_not_implemented() {
        let stub = SmbVfsStub;
        let auth = make_test_auth(1);
        let flags = OpenFlags::new(true, false, false, false, false);

        match stub.smb_open(&auth, "/test", flags) {
            Err(GatewayError::NotImplemented { feature }) => assert_eq!(feature, "smb3"),
            _ => panic!("Expected NotImplemented"),
        }

        match stub.smb_close(SmbFileId(1)) {
            Err(GatewayError::NotImplemented { feature }) => assert_eq!(feature, "smb3"),
            _ => panic!("Expected NotImplemented"),
        }

        match stub.smb_read(SmbFileId(1), 0, 100) {
            Err(GatewayError::NotImplemented { feature }) => assert_eq!(feature, "smb3"),
            _ => panic!("Expected NotImplemented"),
        }

        match stub.smb_write(SmbFileId(1), 0, &[0u8; 10]) {
            Err(GatewayError::NotImplemented { feature }) => assert_eq!(feature, "smb3"),
            _ => panic!("Expected NotImplemented"),
        }

        match stub.smb_stat(&auth, "/test") {
            Err(GatewayError::NotImplemented { feature }) => assert_eq!(feature, "smb3"),
            _ => panic!("Expected NotImplemented"),
        }

        match stub.smb_mkdir(&auth, "/test") {
            Err(GatewayError::NotImplemented { feature }) => assert_eq!(feature, "smb3"),
            _ => panic!("Expected NotImplemented"),
        }

        match stub.smb_unlink(&auth, "/test") {
            Err(GatewayError::NotImplemented { feature }) => assert_eq!(feature, "smb3"),
            _ => panic!("Expected NotImplemented"),
        }

        match stub.smb_rename(&auth, "/old", "/new") {
            Err(GatewayError::NotImplemented { feature }) => assert_eq!(feature, "smb3"),
            _ => panic!("Expected NotImplemented"),
        }

        match stub.smb_readdir(&auth, "/test") {
            Err(GatewayError::NotImplemented { feature }) => assert_eq!(feature, "smb3"),
            _ => panic!("Expected NotImplemented"),
        }
    }

    #[test]
    fn test_smb_sec_stub_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<SmbVfsStub>();
    }

    #[test]
    fn test_smb_sec_stub_path_traversal() {
        let stub = SmbVfsStub;
        let auth = make_test_auth(1);
        let flags = OpenFlags::new(true, false, false, false, false);

        match stub.smb_open(&auth, "../../etc/passwd", flags) {
            Err(GatewayError::NotImplemented { feature }) => assert_eq!(feature, "smb3"),
            _ => panic!("Expected NotImplemented"),
        }
    }

    #[test]
    fn test_smb_sec_stub_null_byte_path() {
        let stub = SmbVfsStub;
        let auth = make_test_auth(1);
        let flags = OpenFlags::new(true, false, false, false, false);
        let path = "/test\0evil";

        match stub.smb_open(&auth, path, flags) {
            Err(GatewayError::NotImplemented { feature }) => assert_eq!(feature, "smb3"),
            _ => panic!("Expected NotImplemented"),
        }
    }

    #[test]
    fn test_smb_sec_stub_long_path() {
        let stub = SmbVfsStub;
        let auth = make_test_auth(1);
        let flags = OpenFlags::new(true, false, false, false, false);
        let path: String = "a".repeat(8192);

        match stub.smb_open(&auth, &path, flags) {
            Err(GatewayError::NotImplemented { feature }) => assert_eq!(feature, "smb3"),
            _ => panic!("Expected NotImplemented"),
        }
    }

    #[test]
    fn test_smb_sec_path_unicode_normalization() {
        let stub = SmbVfsStub;
        let auth = make_test_auth(1);
        let flags = OpenFlags::new(true, false, false, false, false);
        let path = "test\u{0301}";

        match stub.smb_open(&auth, path, flags) {
            Err(GatewayError::NotImplemented { feature }) => assert_eq!(feature, "smb3"),
            _ => panic!("Expected NotImplemented"),
        }
    }

    #[test]
    fn test_smb_sec_path_windows_sep() {
        let stub = SmbVfsStub;
        let auth = make_test_auth(1);
        let flags = OpenFlags::new(true, false, false, false, false);
        let path = "C:\\test\\file";

        match stub.smb_open(&auth, path, flags) {
            Err(GatewayError::NotImplemented { feature }) => assert_eq!(feature, "smb3"),
            _ => panic!("Expected NotImplemented"),
        }
    }

    #[test]
    fn test_smb_sec_path_abs_vs_rel() {
        let stub = SmbVfsStub;
        let auth = make_test_auth(1);
        let flags = OpenFlags::new(true, false, false, false, false);

        match stub.smb_open(&auth, "/abs/path", flags) {
            Err(GatewayError::NotImplemented { feature }) => assert_eq!(feature, "smb3"),
            _ => panic!("Expected NotImplemented"),
        }

        match stub.smb_open(&auth, "rel/path", flags) {
            Err(GatewayError::NotImplemented { feature }) => assert_eq!(feature, "smb3"),
            _ => panic!("Expected NotImplemented"),
        }
    }

    #[test]
    fn test_smb_sec_path_double_slash() {
        let stub = SmbVfsStub;
        let auth = make_test_auth(1);
        let flags = OpenFlags::new(true, false, false, false, false);
        let path = "//test//file";

        match stub.smb_open(&auth, path, flags) {
            Err(GatewayError::NotImplemented { feature }) => assert_eq!(feature, "smb3"),
            _ => panic!("Expected NotImplemented"),
        }
    }

    #[test]
    fn test_smb_sec_path_empty() {
        let stub = SmbVfsStub;
        let auth = make_test_auth(1);
        let flags = OpenFlags::new(true, false, false, false, false);
        let path = "";

        match stub.smb_open(&auth, path, flags) {
            Err(GatewayError::NotImplemented { feature }) => assert_eq!(feature, "smb3"),
            _ => panic!("Expected NotImplemented"),
        }
    }
}
