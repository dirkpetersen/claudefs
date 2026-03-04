//! Gateway READDIRPLUS encoding and cluster backend security tests.

#[cfg(test)]
mod tests {
    use claudefs_gateway::cluster_backend::ClusterVfsBackend;
    use claudefs_gateway::gateway_conn_pool::ConnPoolConfig;
    use claudefs_gateway::nfs::VfsBackend;
    use claudefs_gateway::nfs_readdirplus::{
        encode_fsinfo_ok, encode_fsstat_ok, encode_getattr_err, encode_getattr_ok,
        encode_lookup_ok, encode_read_ok, encode_readdirplus_err, encode_readdirplus_ok,
        encode_write_ok,
    };
    use claudefs_gateway::protocol::{
        Entryplus3, Fattr3, FileHandle3, FsInfoResult, FsStatResult, Ftype3, Nfstime3,
    };
    use claudefs_gateway::xdr::{XdrDecoder, XdrEncoder};
    use prost::bytes::Bytes;

    fn test_fattr() -> Fattr3 {
        Fattr3 {
            ftype: Ftype3::Reg,
            mode: 0o644,
            nlink: 1,
            uid: 0,
            gid: 0,
            size: 1024,
            used: 4096,
            rdev: (0, 0),
            fsid: 1,
            fileid: 100,
            atime: Nfstime3::zero(),
            mtime: Nfstime3::zero(),
            ctime: Nfstime3::zero(),
        }
    }

    fn make_backend() -> ClusterVfsBackend {
        ClusterVfsBackend::new(vec![], ConnPoolConfig::default())
    }

    #[test]
    fn test_gw_rdpc_sec_readdirplus_empty_eof() {
        let attr = test_fattr();
        let data = encode_readdirplus_ok(Some(&attr), 12345, &[], true);
        let mut dec = XdrDecoder::new(Bytes::from(data));
        assert_eq!(dec.decode_u32().unwrap(), 0);
        assert!(dec.decode_bool().unwrap());
        let _ = Fattr3::decode_xdr(&mut dec).unwrap();
        assert_eq!(dec.decode_u64().unwrap(), 12345);
        assert_eq!(dec.decode_u32().unwrap(), 0);
        assert!(dec.decode_bool().unwrap());
    }

    #[test]
    fn test_gw_rdpc_sec_readdirplus_single_entry() {
        let attr = test_fattr();
        let entry = Entryplus3 {
            fileid: 101,
            name: "test.txt".to_string(),
            cookie: 1,
            name_attributes: Some(test_fattr()),
            name_handle: Some(FileHandle3::from_inode(101)),
        };
        let data = encode_readdirplus_ok(Some(&attr), 0, &[entry], false);
        let mut dec = XdrDecoder::new(Bytes::from(data));
        assert_eq!(dec.decode_u32().unwrap(), 0);
        assert!(dec.decode_bool().unwrap());
        let _ = Fattr3::decode_xdr(&mut dec).unwrap();
        assert_eq!(dec.decode_u64().unwrap(), 0);
        assert_eq!(dec.decode_u32().unwrap(), 1);
        assert_eq!(dec.decode_u64().unwrap(), 101);
        assert_eq!(dec.decode_string().unwrap(), "test.txt");
        assert_eq!(dec.decode_u64().unwrap(), 1);
        assert!(dec.decode_bool().unwrap());
        let _ = Fattr3::decode_xdr(&mut dec).unwrap();
        assert_eq!(dec.decode_u32().unwrap(), 1);
        let _ = FileHandle3::decode_xdr(&mut dec).unwrap();
        assert_eq!(dec.decode_u32().unwrap(), 0);
        assert!(!dec.decode_bool().unwrap());
    }

    #[test]
    fn test_gw_rdpc_sec_readdirplus_multiple_no_attr() {
        let entries = vec![
            Entryplus3 {
                fileid: 101,
                name: "file1.txt".to_string(),
                cookie: 1,
                name_attributes: None,
                name_handle: None,
            },
            Entryplus3 {
                fileid: 102,
                name: "file2.txt".to_string(),
                cookie: 2,
                name_attributes: None,
                name_handle: None,
            },
        ];
        let data = encode_readdirplus_ok(None, 0, &entries, true);
        let mut dec = XdrDecoder::new(Bytes::from(data));
        assert_eq!(dec.decode_u32().unwrap(), 0);
        assert!(!dec.decode_bool().unwrap());
        assert_eq!(dec.decode_u64().unwrap(), 0);
        assert_eq!(dec.decode_u32().unwrap(), 1);
        assert_eq!(dec.decode_u64().unwrap(), 101);
        assert_eq!(dec.decode_string().unwrap(), "file1.txt");
        assert_eq!(dec.decode_u64().unwrap(), 1);
        assert!(!dec.decode_bool().unwrap());
        assert_eq!(dec.decode_u32().unwrap(), 0);
        assert_eq!(dec.decode_u32().unwrap(), 1);
        assert_eq!(dec.decode_u64().unwrap(), 102);
        assert_eq!(dec.decode_string().unwrap(), "file2.txt");
        assert_eq!(dec.decode_u64().unwrap(), 2);
        assert!(!dec.decode_bool().unwrap());
        assert_eq!(dec.decode_u32().unwrap(), 0);
        assert_eq!(dec.decode_u32().unwrap(), 0);
        assert!(dec.decode_bool().unwrap());
    }

    #[test]
    fn test_gw_rdpc_sec_readdirplus_no_dir_attr() {
        let entries = vec![Entryplus3 {
            fileid: 200,
            name: "item".to_string(),
            cookie: 5,
            name_attributes: None,
            name_handle: None,
        }];
        let data = encode_readdirplus_ok(None, 999, &entries, true);
        let mut dec = XdrDecoder::new(Bytes::from(data));
        assert_eq!(dec.decode_u32().unwrap(), 0);
        assert!(!dec.decode_bool().unwrap());
        assert_eq!(dec.decode_u64().unwrap(), 999);
    }

    #[test]
    fn test_gw_rdpc_sec_readdirplus_error() {
        let attr = test_fattr();
        let data = encode_readdirplus_err(2, Some(&attr));
        let mut dec = XdrDecoder::new(Bytes::from(data));
        assert_eq!(dec.decode_u32().unwrap(), 2);
        assert!(dec.decode_bool().unwrap());
        let _ = Fattr3::decode_xdr(&mut dec).unwrap();
    }

    #[test]
    fn test_gw_rdpc_sec_getattr_ok() {
        let attr = test_fattr();
        let data = encode_getattr_ok(&attr);
        let mut dec = XdrDecoder::new(Bytes::from(data));
        assert_eq!(dec.decode_u32().unwrap(), 0);
        let decoded = Fattr3::decode_xdr(&mut dec).unwrap();
        assert_eq!(decoded.fileid, 100);
        assert_eq!(decoded.size, 1024);
    }

    #[test]
    fn test_gw_rdpc_sec_getattr_err() {
        let data = encode_getattr_err(2);
        let mut dec = XdrDecoder::new(Bytes::from(data));
        assert_eq!(dec.decode_u32().unwrap(), 2);
    }

    #[test]
    fn test_gw_rdpc_sec_lookup_ok() {
        let fh = FileHandle3::from_inode(200);
        let attr = test_fattr();
        let data = encode_lookup_ok(&fh, Some(&attr), Some(&attr));
        let mut dec = XdrDecoder::new(Bytes::from(data));
        assert_eq!(dec.decode_u32().unwrap(), 0);
        let decoded_fh = FileHandle3::decode_xdr(&mut dec).unwrap();
        assert_eq!(decoded_fh.as_inode(), Some(200));
        assert!(dec.decode_bool().unwrap());
        let _ = Fattr3::decode_xdr(&mut dec).unwrap();
        assert!(dec.decode_bool().unwrap());
        let _ = Fattr3::decode_xdr(&mut dec).unwrap();
    }

    #[test]
    fn test_gw_rdpc_sec_read_ok() {
        let attr = test_fattr();
        let data = b"hello world";
        let encoded = encode_read_ok(Some(&attr), data, true);
        let mut dec = XdrDecoder::new(Bytes::from(encoded));
        assert_eq!(dec.decode_u32().unwrap(), 0);
        assert!(dec.decode_bool().unwrap());
        let _ = Fattr3::decode_xdr(&mut dec).unwrap();
        assert_eq!(dec.decode_u32().unwrap(), 11);
        assert!(dec.decode_bool().unwrap());
        let opaque = dec.decode_opaque_variable().unwrap();
        assert_eq!(opaque, data);
    }

    #[test]
    fn test_gw_rdpc_sec_write_ok() {
        let data = encode_write_ok(1000, 2, 12345);
        let mut dec = XdrDecoder::new(Bytes::from(data));
        assert_eq!(dec.decode_u32().unwrap(), 0);
        assert_eq!(dec.decode_u32().unwrap(), 1000);
        assert_eq!(dec.decode_u32().unwrap(), 2);
        assert_eq!(dec.decode_u64().unwrap(), 12345);
    }

    #[test]
    fn test_gw_rdpc_sec_fsstat_ok() {
        let attr = test_fattr();
        let stat = FsStatResult {
            tbytes: 1000000000,
            fbytes: 500000000,
            abytes: 500000000,
            tfiles: 10000,
            ffiles: 5000,
            afiles: 5000,
            invarsec: 30,
        };
        let data = encode_fsstat_ok(Some(&attr), &stat);
        let mut dec = XdrDecoder::new(Bytes::from(data));
        assert_eq!(dec.decode_u32().unwrap(), 0);
        assert!(dec.decode_bool().unwrap());
        let _ = Fattr3::decode_xdr(&mut dec).unwrap();
        assert_eq!(dec.decode_u64().unwrap(), 1000000000);
        assert_eq!(dec.decode_u64().unwrap(), 500000000);
        assert_eq!(dec.decode_u64().unwrap(), 500000000);
        assert_eq!(dec.decode_u64().unwrap(), 10000);
        assert_eq!(dec.decode_u64().unwrap(), 5000);
        assert_eq!(dec.decode_u32().unwrap(), 5000);
        assert_eq!(dec.decode_u32().unwrap(), 30);
    }

    #[test]
    fn test_gw_rdpc_sec_fsstat_no_attr() {
        let stat = FsStatResult {
            tbytes: 2000,
            fbytes: 1000,
            abytes: 500,
            tfiles: 100,
            ffiles: 50,
            afiles: 25,
            invarsec: 60,
        };
        let data = encode_fsstat_ok(None, &stat);
        let mut dec = XdrDecoder::new(Bytes::from(data));
        assert_eq!(dec.decode_u32().unwrap(), 0);
        assert!(!dec.decode_bool().unwrap());
        assert_eq!(dec.decode_u64().unwrap(), 2000);
    }

    #[test]
    fn test_gw_rdpc_sec_fsinfo_ok() {
        let attr = test_fattr();
        let info = FsInfoResult {
            rtmax: 1048576,
            rtpref: 65536,
            rtmult: 4096,
            wtmax: 1048576,
            wtpref: 65536,
            wtmult: 4096,
            dtpref: 65536,
            maxfilesize: u64::MAX,
            time_delta: Nfstime3::zero(),
            properties: 0x0F,
        };
        let data = encode_fsinfo_ok(Some(&attr), &info);
        let mut dec = XdrDecoder::new(Bytes::from(data));
        assert_eq!(dec.decode_u32().unwrap(), 0);
        assert!(dec.decode_bool().unwrap());
        let _ = Fattr3::decode_xdr(&mut dec).unwrap();
        assert_eq!(dec.decode_u32().unwrap(), 1048576);
        assert_eq!(dec.decode_u32().unwrap(), 65536);
        assert_eq!(dec.decode_u32().unwrap(), 4096);
        assert_eq!(dec.decode_u32().unwrap(), 1048576);
        assert_eq!(dec.decode_u32().unwrap(), 65536);
        assert_eq!(dec.decode_u32().unwrap(), 4096);
        assert_eq!(dec.decode_u32().unwrap(), 65536);
        assert_eq!(dec.decode_u64().unwrap(), u64::MAX);
        let _ = Nfstime3::decode_xdr(&mut dec).unwrap();
        assert_eq!(dec.decode_u32().unwrap(), 0x0F);
    }

    #[test]
    fn test_gw_rdpc_sec_fsinfo_no_attr() {
        let info = FsInfoResult::defaults();
        let data = encode_fsinfo_ok(None, &info);
        let mut dec = XdrDecoder::new(Bytes::from(data));
        assert_eq!(dec.decode_u32().unwrap(), 0);
        assert!(!dec.decode_bool().unwrap());
        assert_eq!(dec.decode_u32().unwrap(), 1048576);
    }

    #[test]
    fn test_gw_rdpc_sec_backend_default_name() {
        let backend = make_backend();
        let stats = backend.stats();
        assert_eq!(stats.total_rpc_calls, 0);
    }

    #[test]
    fn test_gw_rdpc_sec_backend_with_name() {
        let backend = make_backend().with_cluster_name("custom-cluster");
        let stats = backend.stats();
        assert_eq!(stats.total_rpc_calls, 0);
    }

    #[test]
    fn test_gw_rdpc_sec_backend_stats_initial_zero() {
        let backend = make_backend();
        let stats = backend.stats();
        assert_eq!(stats.total_rpc_calls, 0);
        assert_eq!(stats.successful_rpcs, 0);
        assert_eq!(stats.failed_rpcs, 0);
        assert_eq!(stats.backend_bytes_read, 0);
        assert_eq!(stats.backend_bytes_written, 0);
    }

    #[test]
    fn test_gw_rdpc_sec_backend_stats_last_success_none() {
        let backend = make_backend();
        let stats = backend.stats();
        assert!(stats.last_success.is_none());
    }

    #[test]
    fn test_gw_rdpc_sec_backend_getattr_not_implemented() {
        let backend = make_backend();
        let fh = FileHandle3::from_inode(1);
        let result = backend.getattr(&fh);
        assert!(result.is_err());
        let stats = backend.stats();
        assert_eq!(stats.total_rpc_calls, 1);
        assert_eq!(stats.failed_rpcs, 1);
    }

    #[test]
    fn test_gw_rdpc_sec_backend_lookup_not_implemented() {
        let backend = make_backend();
        let dir_fh = FileHandle3::from_inode(1);
        let result = backend.lookup(&dir_fh, "file.txt");
        assert!(result.is_err());
        let stats = backend.stats();
        assert_eq!(stats.total_rpc_calls, 1);
        assert_eq!(stats.failed_rpcs, 1);
    }

    #[test]
    fn test_gw_rdpc_sec_backend_all_ops_increment_stats() {
        let backend = make_backend();
        let fh = FileHandle3::from_inode(1);
        let dir_fh = FileHandle3::from_inode(2);
        let _ = backend.getattr(&fh);
        let _ = backend.lookup(&dir_fh, "name");
        let _ = backend.read(&fh, 0, 1024);
        let _ = backend.write(&fh, 0, b"data");
        let _ = backend.readdir(&dir_fh, 0, 256);
        let _ = backend.mkdir(&dir_fh, "dir", 0o755);
        let _ = backend.create(&dir_fh, "file", 0o644);
        let _ = backend.remove(&dir_fh, "file");
        let _ = backend.rename(&dir_fh, "old", &dir_fh, "new");
        let _ = backend.readlink(&fh);
        let _ = backend.symlink(&dir_fh, "link", "/target");
        let _ = backend.fsstat(&fh);
        let _ = backend.fsinfo(&fh);
        let _ = backend.pathconf(&fh);
        let _ = backend.access(&fh, 0, 0, 7);
        let stats = backend.stats();
        assert_eq!(stats.total_rpc_calls, 15);
        assert_eq!(stats.failed_rpcs, 15);
        assert_eq!(stats.successful_rpcs, 0);
    }

    #[test]
    fn test_gw_rdpc_sec_backend_stats_match_ops() {
        let backend = make_backend();
        let fh = FileHandle3::from_inode(1);
        let _ = backend.getattr(&fh);
        let _ = backend.read(&fh, 0, 100);
        let _ = backend.write(&fh, 0, b"test");
        let stats = backend.stats();
        assert_eq!(stats.total_rpc_calls, 3);
    }

    #[test]
    fn test_gw_rdpc_sec_backend_failed_equals_total() {
        let backend = make_backend();
        let fh = FileHandle3::from_inode(1);
        for _ in 0..10 {
            let _ = backend.getattr(&fh);
        }
        let stats = backend.stats();
        assert_eq!(stats.failed_rpcs, stats.total_rpc_calls);
    }

    #[test]
    fn test_gw_rdpc_sec_entry_empty_name() {
        let entry = Entryplus3 {
            fileid: 1,
            name: "".to_string(),
            cookie: 0,
            name_attributes: None,
            name_handle: None,
        };
        let data = encode_readdirplus_ok(None, 0, &[entry], false);
        let mut dec = XdrDecoder::new(Bytes::from(data));
        dec.decode_u32().unwrap();
        dec.decode_bool().unwrap();
        dec.decode_u64().unwrap();
        assert_eq!(dec.decode_u32().unwrap(), 1);
        dec.decode_u64().unwrap();
        assert_eq!(dec.decode_string().unwrap(), "");
    }

    #[test]
    fn test_gw_rdpc_sec_entry_long_name() {
        let long_name = "a".repeat(1000);
        let entry = Entryplus3 {
            fileid: 1,
            name: long_name.clone(),
            cookie: 0,
            name_attributes: None,
            name_handle: None,
        };
        let data = encode_readdirplus_ok(None, 0, &[entry], false);
        let mut dec = XdrDecoder::new(Bytes::from(data));
        dec.decode_u32().unwrap();
        dec.decode_bool().unwrap();
        dec.decode_u64().unwrap();
        assert_eq!(dec.decode_u32().unwrap(), 1);
        dec.decode_u64().unwrap();
        assert_eq!(dec.decode_string().unwrap(), long_name);
    }

    #[test]
    fn test_gw_rdpc_sec_read_ok_empty_data() {
        let data = encode_read_ok(None, &[], false);
        let mut dec = XdrDecoder::new(Bytes::from(data));
        assert_eq!(dec.decode_u32().unwrap(), 0);
        assert!(!dec.decode_bool().unwrap());
        assert_eq!(dec.decode_u32().unwrap(), 0);
        assert!(!dec.decode_bool().unwrap());
        let opaque = dec.decode_opaque_variable().unwrap();
        assert!(opaque.is_empty());
    }

    #[test]
    fn test_gw_rdpc_sec_read_ok_large_data() {
        let large_data = vec![0xABu8; 65536];
        let encoded = encode_read_ok(None, &large_data, true);
        let mut dec = XdrDecoder::new(Bytes::from(encoded));
        assert_eq!(dec.decode_u32().unwrap(), 0);
        assert!(!dec.decode_bool().unwrap());
        assert_eq!(dec.decode_u32().unwrap(), 65536);
        assert!(dec.decode_bool().unwrap());
        let opaque = dec.decode_opaque_variable().unwrap();
        assert_eq!(opaque.len(), 65536);
        assert!(opaque.iter().all(|&b| b == 0xAB));
    }

    #[test]
    fn test_gw_rdpc_sec_write_ok_zero_count() {
        let data = encode_write_ok(0, 0, 0);
        let mut dec = XdrDecoder::new(Bytes::from(data));
        assert_eq!(dec.decode_u32().unwrap(), 0);
        assert_eq!(dec.decode_u32().unwrap(), 0);
        assert_eq!(dec.decode_u32().unwrap(), 0);
        assert_eq!(dec.decode_u64().unwrap(), 0);
    }
}
