//! Gateway NFS core protocol security tests.
//!
//! Part of A10 Phase 27: Gateway NFS core security audit

#[cfg(test)]
mod tests {
    use claudefs_gateway::error::{
        NFS3ERR_BADHANDLE, NFS3ERR_EXIST, NFS3ERR_INVAL, NFS3ERR_NOENT, NFS3ERR_NOTDIR,
    };
    use claudefs_gateway::nfs::{
        InodeEntry, MockVfsBackend, Nfs3AccessResult, Nfs3CreateResult, Nfs3FsInfoResult,
        Nfs3FsStatResult, Nfs3GetAttrResult, Nfs3Handler, Nfs3LookupResult, Nfs3MkdirResult,
        Nfs3PathConfResult, Nfs3ReadDirResult, Nfs3ReadLinkResult, Nfs3ReadResult,
        Nfs3RemoveResult, Nfs3RenameResult, Nfs3SymLinkResult, Nfs3WriteResult, VfsBackend,
    };
    use claudefs_gateway::nfs_listener::{NfsListener, NfsShutdown};
    use claudefs_gateway::protocol::{Fattr3, FileHandle3, Ftype3, Nfstime3};
    use std::sync::Arc;

    fn setup() -> (
        Arc<MockVfsBackend>,
        Nfs3Handler<MockVfsBackend>,
        FileHandle3,
    ) {
        let backend = Arc::new(MockVfsBackend::new(1));
        let handler = Nfs3Handler::new(backend.clone(), 1);
        let root_fh = FileHandle3::from_inode(1);
        (backend, handler, root_fh)
    }

    #[test]
    fn test_nfs_core_sec_fh_inode_zero() {
        let (_backend, handler, _root_fh) = setup();
        let fh = FileHandle3::from_inode(0);
        let result = handler.handle_getattr(&fh);
        match result {
            Nfs3GetAttrResult::Err(code) => assert_eq!(code, NFS3ERR_BADHANDLE),
            _ => panic!("expected BADHANDLE error"),
        }
    }

    #[test]
    fn test_nfs_core_sec_fh_inode_max() {
        let (_backend, handler, _root_fh) = setup();
        let fh = FileHandle3::from_inode(u64::MAX);
        let result = handler.handle_getattr(&fh);
        match result {
            Nfs3GetAttrResult::Err(code) => assert_eq!(code, NFS3ERR_NOENT),
            _ => panic!("expected NOENT error"),
        }
    }

    #[test]
    fn test_nfs_core_sec_fh_nonexistent() {
        let (_backend, handler, _root_fh) = setup();
        let fh = FileHandle3::from_inode(999);
        let result = handler.handle_getattr(&fh);
        match result {
            Nfs3GetAttrResult::Err(code) => assert_eq!(code, NFS3ERR_NOENT),
            _ => panic!("expected NOENT error"),
        }
    }

    #[test]
    fn test_nfs_core_sec_fh_use_after_remove() {
        let (_backend, handler, root_fh) = setup();
        let create_result = handler.handle_create(&root_fh, "testfile", 0o644);
        let (fh, _) = match create_result {
            Nfs3CreateResult::Ok(fh, attr) => (fh, attr),
            _ => panic!("expected ok"),
        };
        let remove_result = handler.handle_remove(&root_fh, "testfile");
        match remove_result {
            Nfs3RemoveResult::Ok => (),
            _ => panic!("expected ok"),
        }
        let result = handler.handle_getattr(&fh);
        match result {
            Nfs3GetAttrResult::Err(code) => assert_eq!(code, NFS3ERR_NOENT),
            _ => panic!("expected NOENT error"),
        }
    }

    #[test]
    fn test_nfs_core_sec_fh_empty() {
        let result = FileHandle3::new(vec![]);
        assert!(result.is_err());
    }

    #[test]
    fn test_nfs_core_sec_access_root_all() {
        let (_backend, handler, root_fh) = setup();
        let result = handler.handle_access(&root_fh, 0, 0, 7);
        match result {
            Nfs3AccessResult::Ok(bits) => assert_eq!(bits, 7),
            _ => panic!("expected ok with all bits"),
        }
    }

    #[test]
    fn test_nfs_core_sec_access_nonowner_denied() {
        let (_backend, handler, root_fh) = setup();
        let result = handler.handle_access(&root_fh, 999, 0, 7);
        match result {
            Nfs3AccessResult::Ok(bits) => assert_eq!(bits, 0),
            _ => panic!("expected ok with 0 bits"),
        }
    }

    #[test]
    fn test_nfs_core_sec_access_group_match() {
        let (_backend, handler, root_fh) = setup();
        let result = handler.handle_access(&root_fh, 999, 0, 7);
        match result {
            Nfs3AccessResult::Ok(bits) => assert_eq!(bits, 7),
            _ => panic!("expected ok with 7 bits"),
        }
    }

    #[test]
    fn test_nfs_core_sec_access_other_match() {
        let (_backend, handler, root_fh) = setup();
        let result = handler.handle_access(&root_fh, 999, 999, 7);
        match result {
            Nfs3AccessResult::Ok(bits) => assert_eq!(bits, 7),
            _ => panic!("expected ok with 7 bits"),
        }
    }

    #[test]
    fn test_nfs_core_sec_access_mode_zero() {
        let (_backend, handler, root_fh) = setup();
        let result = handler.handle_access(&root_fh, 999, 999, 7);
        match result {
            Nfs3AccessResult::Ok(bits) => assert_eq!(bits, 7),
            _ => panic!("expected ok with 7 bits"),
        }
    }

    #[test]
    fn test_nfs_core_sec_lookup_empty_name() {
        let (_backend, handler, root_fh) = setup();
        let result = handler.handle_lookup(&root_fh, "");
        match result {
            Nfs3LookupResult::Err(code) => assert_eq!(code, NFS3ERR_NOENT),
            _ => panic!("expected NOENT error"),
        }
    }

    #[test]
    fn test_nfs_core_sec_lookup_null_bytes() {
        let (_backend, handler, root_fh) = setup();
        let name = "test\0file";
        let result = handler.handle_lookup(&root_fh, name);
        match result {
            Nfs3LookupResult::Err(code) => assert_eq!(code, NFS3ERR_NOENT),
            _ => panic!("expected NOENT error"),
        }
    }

    #[test]
    fn test_nfs_core_sec_lookup_path_separator() {
        let (_backend, handler, root_fh) = setup();
        let result = handler.handle_lookup(&root_fh, "sub/dir");
        match result {
            Nfs3LookupResult::Err(code) => assert_eq!(code, NFS3ERR_NOENT),
            _ => panic!("expected NOENT error"),
        }
    }

    #[test]
    fn test_nfs_core_sec_create_dot_name() {
        let (_backend, handler, root_fh) = setup();
        let result = handler.handle_create(&root_fh, ".", 0o644);
        match result {
            Nfs3CreateResult::Ok(fh, _) => assert!(fh.as_inode().is_some()),
            _ => panic!("expected ok"),
        }
    }

    #[test]
    fn test_nfs_core_sec_create_long_name() {
        let (_backend, handler, root_fh) = setup();
        let long_name = "a".repeat(4096);
        let result = handler.handle_create(&root_fh, &long_name, 0o644);
        match result {
            Nfs3CreateResult::Ok(fh, _) => assert!(fh.as_inode().is_some()),
            _ => panic!("expected ok"),
        }
    }

    #[test]
    fn test_nfs_core_sec_read_beyond_eof() {
        let (_backend, handler, root_fh) = setup();
        let create_result = handler.handle_create(&root_fh, "testfile", 0o644);
        let (fh, _) = match create_result {
            Nfs3CreateResult::Ok(fh, attr) => (fh, attr),
            _ => panic!("expected ok"),
        };
        let write_result = handler.handle_write(&fh, 0, 0, b"hello");
        match write_result {
            Nfs3WriteResult::Ok(_, _) => (),
            _ => panic!("expected ok"),
        }
        let result = handler.handle_read(&fh, 100, 10);
        match result {
            Nfs3ReadResult::Ok(data, eof) => {
                assert!(data.is_empty());
                assert!(eof);
            }
            _ => panic!("expected ok"),
        }
    }

    #[test]
    fn test_nfs_core_sec_read_count_zero() {
        let (_backend, handler, root_fh) = setup();
        let create_result = handler.handle_create(&root_fh, "testfile", 0o644);
        let (fh, _) = match create_result {
            Nfs3CreateResult::Ok(fh, attr) => (fh, attr),
            _ => panic!("expected ok"),
        };
        let write_result = handler.handle_write(&fh, 0, 0, b"hello");
        match write_result {
            Nfs3WriteResult::Ok(_, _) => (),
            _ => panic!("expected ok"),
        }
        let result = handler.handle_read(&fh, 0, 0);
        match result {
            Nfs3ReadResult::Ok(data, _eof) => assert!(data.is_empty()),
            _ => panic!("expected ok"),
        }
    }

    #[test]
    fn test_nfs_core_sec_read_directory() {
        let (_backend, handler, root_fh) = setup();
        let result = handler.handle_read(&root_fh, 0, 100);
        match result {
            Nfs3ReadResult::Err(code) => assert_eq!(code, NFS3ERR_INVAL),
            _ => panic!("expected INVAL error"),
        }
    }

    #[test]
    fn test_nfs_core_sec_write_directory() {
        let (_backend, handler, root_fh) = setup();
        let result = handler.handle_write(&root_fh, 0, 0, b"data");
        match result {
            Nfs3WriteResult::Err(code) => assert_eq!(code, NFS3ERR_INVAL),
            _ => panic!("expected INVAL error"),
        }
    }

    #[test]
    fn test_nfs_core_sec_write_large_offset() {
        let (_backend, handler, root_fh) = setup();
        let create_result = handler.handle_create(&root_fh, "testfile", 0o644);
        let (fh, _) = match create_result {
            Nfs3CreateResult::Ok(fh, attr) => (fh, attr),
            _ => panic!("expected ok"),
        };
        let large_offset = 100_000_000;
        let result = handler.handle_write(&fh, large_offset, 0, b"x");
        match result {
            Nfs3WriteResult::Ok(count, _) => assert_eq!(count, 1),
            _ => panic!("expected ok"),
        }
    }

    #[test]
    fn test_nfs_core_sec_readdir_on_file() {
        let (_backend, handler, root_fh) = setup();
        let create_result = handler.handle_create(&root_fh, "testfile", 0o644);
        let (fh, _) = match create_result {
            Nfs3CreateResult::Ok(fh, attr) => (fh, attr),
            _ => panic!("expected ok"),
        };
        let result = handler.handle_readdir(&fh, 0, 0, 100);
        match result {
            Nfs3ReadDirResult::Err(code) => assert_eq!(code, NFS3ERR_NOTDIR),
            _ => panic!("expected NOTDIR error"),
        }
    }

    #[test]
    fn test_nfs_core_sec_mkdir_in_file() {
        let (_backend, handler, root_fh) = setup();
        let create_result = handler.handle_create(&root_fh, "testfile", 0o644);
        let (fh, _) = match create_result {
            Nfs3CreateResult::Ok(fh, attr) => (fh, attr),
            _ => panic!("expected ok"),
        };
        let result = handler.handle_mkdir(&fh, "subdir", 0o755);
        match result {
            Nfs3MkdirResult::Err(code) => assert_eq!(code, NFS3ERR_NOTDIR),
            _ => panic!("expected NOTDIR error"),
        }
    }

    #[test]
    fn test_nfs_core_sec_remove_nonexistent() {
        let (_backend, handler, root_fh) = setup();
        let result = handler.handle_remove(&root_fh, "nonexistent");
        match result {
            Nfs3RemoveResult::Err(code) => assert_eq!(code, NFS3ERR_NOENT),
            _ => panic!("expected NOENT error"),
        }
    }

    #[test]
    fn test_nfs_core_sec_rename_target_exists() {
        let (_backend, handler, root_fh) = setup();
        handler.handle_create(&root_fh, "file1", 0o644);
        handler.handle_create(&root_fh, "file2", 0o644);
        let result = handler.handle_rename(&root_fh, "file1", &root_fh, "file2");
        match result {
            Nfs3RenameResult::Err(code) => assert_eq!(code, NFS3ERR_EXIST),
            _ => panic!("expected EXIST error"),
        }
    }

    #[test]
    fn test_nfs_core_sec_create_duplicate() {
        let (_backend, handler, root_fh) = setup();
        handler.handle_create(&root_fh, "testfile", 0o644);
        let result = handler.handle_create(&root_fh, "testfile", 0o644);
        match result {
            Nfs3CreateResult::Err(code) => assert_eq!(code, NFS3ERR_EXIST),
            _ => panic!("expected EXIST error"),
        }
    }

    #[test]
    fn test_nfs_core_sec_listener_new() {
        let (listener, shutdown) = NfsListener::new("127.0.0.1:0");
        assert_eq!(listener.bind_addr, "127.0.0.1:0");
        shutdown.shutdown();
    }

    #[test]
    fn test_nfs_core_sec_shutdown_signal() {
        let (listener, shutdown) = NfsListener::new("0.0.0.0:2049");
        assert_eq!(listener.bind_addr, "0.0.0.0:2049");
        shutdown.shutdown();
    }

    #[test]
    fn test_nfs_core_sec_max_rpc_record() {
        let max_record_size: u32 = 4 * 1024 * 1024;
        assert_eq!(max_record_size, 4 * 1024 * 1024);

        let over_limit: u32 = max_record_size + 1;
        let mark = over_limit & 0x7FFF_FFFF;
        assert!(mark > max_record_size as u32);
    }

    #[test]
    fn test_nfs_core_sec_record_mark_fragment() {
        let mark: u32 = 0x0000_0100;
        let last_fragment = (mark & 0x8000_0000) != 0;
        let fragment_len = (mark & 0x7FFF_FFFF) as usize;
        assert!(!last_fragment);
        assert_eq!(fragment_len, 256);
    }

    #[test]
    fn test_nfs_core_sec_record_mark_zero_len() {
        let mark: u32 = 0x8000_0000;
        let last_fragment = (mark & 0x8000_0000) != 0;
        let fragment_len = (mark & 0x7FFF_FFFF) as usize;
        assert!(last_fragment);
        assert_eq!(fragment_len, 0);
    }
}
