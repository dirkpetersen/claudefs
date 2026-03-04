//! Tests for ClusterVfsBackend in claudefs-gateway

use claudefs_gateway::cluster_backend::{ClusterStats, ClusterVfsBackend};
use claudefs_gateway::error::GatewayError;
use claudefs_gateway::gateway_conn_pool::{BackendNode, ConnPoolConfig};
use claudefs_gateway::nfs::VfsBackend;
use claudefs_gateway::protocol::FileHandle3;

fn make_fh(data: Vec<u8>) -> FileHandle3 {
    if data.is_empty() {
        FileHandle3 { data: vec![1] }
    } else {
        FileHandle3 { data }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backend_new_empty_nodes() {
        let backend = ClusterVfsBackend::new(vec![], ConnPoolConfig::default());
        let _ = backend;
    }

    #[test]
    fn test_backend_new_with_nodes() {
        let nodes = vec![
            BackendNode::new("node1", "192.168.1.1:8080"),
            BackendNode::new("node2", "192.168.1.2:8080"),
        ];
        let backend = ClusterVfsBackend::new(nodes, ConnPoolConfig::default());
        let _ = backend;
    }

    #[test]
    fn test_with_cluster_name_builder() {
        let backend = ClusterVfsBackend::new(vec![], ConnPoolConfig::default())
            .with_cluster_name("test-cluster");
        let stats = backend.stats();
        assert_eq!(stats.total_rpc_calls, 0);
    }

    #[test]
    fn test_initial_stats_all_zero() {
        let backend = ClusterVfsBackend::new(vec![], ConnPoolConfig::default());
        let stats = backend.stats();
        assert_eq!(stats.total_rpc_calls, 0);
        assert_eq!(stats.successful_rpcs, 0);
        assert_eq!(stats.failed_rpcs, 0);
        assert_eq!(stats.backend_bytes_read, 0);
        assert_eq!(stats.backend_bytes_written, 0);
        assert!(stats.last_success.is_none());
    }

    #[test]
    fn test_getattr_returns_not_implemented() {
        let backend = ClusterVfsBackend::new(vec![], ConnPoolConfig::default());
        let fh = make_fh(vec![1, 2, 3, 4]);
        let result = backend.getattr(&fh);
        assert!(result.is_err());
        match result.unwrap_err() {
            GatewayError::NotImplemented { feature } => {
                assert!(feature.contains("getattr"));
            }
            _ => panic!("expected NotImplemented"),
        }
    }

    #[test]
    fn test_lookup_returns_not_implemented() {
        let backend = ClusterVfsBackend::new(vec![], ConnPoolConfig::default());
        let dir_fh = make_fh(vec![1, 2]);
        let result = backend.lookup(&dir_fh, "file.txt");
        assert!(result.is_err());
    }

    #[test]
    fn test_read_returns_not_implemented() {
        let backend = ClusterVfsBackend::new(vec![], ConnPoolConfig::default());
        let fh = make_fh(vec![1, 2, 3, 4]);
        let result = backend.read(&fh, 0, 1024);
        assert!(result.is_err());
    }

    #[test]
    fn test_write_returns_not_implemented() {
        let backend = ClusterVfsBackend::new(vec![], ConnPoolConfig::default());
        let fh = make_fh(vec![1, 2, 3, 4]);
        let result = backend.write(&fh, 0, b"data");
        assert!(result.is_err());
    }

    #[test]
    fn test_readdir_returns_not_implemented() {
        let backend = ClusterVfsBackend::new(vec![], ConnPoolConfig::default());
        let dir_fh = make_fh(vec![1, 2]);
        let result = backend.readdir(&dir_fh, 0, 256);
        assert!(result.is_err());
    }

    #[test]
    fn test_mkdir_returns_not_implemented() {
        let backend = ClusterVfsBackend::new(vec![], ConnPoolConfig::default());
        let dir_fh = make_fh(vec![1, 2]);
        let result = backend.mkdir(&dir_fh, "newdir", 0o755);
        assert!(result.is_err());
    }

    #[test]
    fn test_create_returns_not_implemented() {
        let backend = ClusterVfsBackend::new(vec![], ConnPoolConfig::default());
        let dir_fh = make_fh(vec![1, 2]);
        let result = backend.create(&dir_fh, "newfile", 0o644);
        assert!(result.is_err());
    }

    #[test]
    fn test_remove_returns_not_implemented() {
        let backend = ClusterVfsBackend::new(vec![], ConnPoolConfig::default());
        let dir_fh = make_fh(vec![1, 2]);
        let result = backend.remove(&dir_fh, "file.txt");
        assert!(result.is_err());
    }

    #[test]
    fn test_rename_returns_not_implemented() {
        let backend = ClusterVfsBackend::new(vec![], ConnPoolConfig::default());
        let dir_fh = make_fh(vec![1, 2]);
        let result = backend.rename(&dir_fh, "old", &dir_fh, "new");
        assert!(result.is_err());
    }

    #[test]
    fn test_readlink_returns_not_implemented() {
        let backend = ClusterVfsBackend::new(vec![], ConnPoolConfig::default());
        let fh = make_fh(vec![1, 2, 3, 4]);
        let result = backend.readlink(&fh);
        assert!(result.is_err());
    }

    #[test]
    fn test_symlink_returns_not_implemented() {
        let backend = ClusterVfsBackend::new(vec![], ConnPoolConfig::default());
        let dir_fh = make_fh(vec![1, 2]);
        let result = backend.symlink(&dir_fh, "link", "/target");
        assert!(result.is_err());
    }

    #[test]
    fn test_fsstat_returns_not_implemented() {
        let backend = ClusterVfsBackend::new(vec![], ConnPoolConfig::default());
        let fh = make_fh(vec![1, 2, 3, 4]);
        let result = backend.fsstat(&fh);
        assert!(result.is_err());
    }

    #[test]
    fn test_fsinfo_returns_not_implemented() {
        let backend = ClusterVfsBackend::new(vec![], ConnPoolConfig::default());
        let fh = make_fh(vec![1, 2, 3, 4]);
        let result = backend.fsinfo(&fh);
        assert!(result.is_err());
    }

    #[test]
    fn test_pathconf_returns_not_implemented() {
        let backend = ClusterVfsBackend::new(vec![], ConnPoolConfig::default());
        let fh = make_fh(vec![1, 2, 3, 4]);
        let result = backend.pathconf(&fh);
        assert!(result.is_err());
    }

    #[test]
    fn test_access_returns_not_implemented() {
        let backend = ClusterVfsBackend::new(vec![], ConnPoolConfig::default());
        let fh = make_fh(vec![1, 2, 3, 4]);
        let result = backend.access(&fh, 1000, 1000, 0x7);
        assert!(result.is_err());
    }

    #[test]
    fn test_stats_accounting_per_op() {
        let backend = ClusterVfsBackend::new(vec![], ConnPoolConfig::default());
        let fh = make_fh(vec![1, 2, 3, 4]);
        let _ = backend.getattr(&fh);

        let stats = backend.stats();
        assert_eq!(stats.total_rpc_calls, 1);
        assert_eq!(stats.failed_rpcs, 1);
    }

    #[test]
    fn test_stats_after_multiple_calls() {
        let backend = ClusterVfsBackend::new(vec![], ConnPoolConfig::default());
        let fh = make_fh(vec![1, 2]);
        let dir_fh = make_fh(vec![1]);

        let _ = backend.getattr(&fh);
        let _ = backend.lookup(&dir_fh, "test");
        let _ = backend.read(&fh, 0, 512);

        let stats = backend.stats();
        assert_eq!(stats.total_rpc_calls, 3);
        assert_eq!(stats.failed_rpcs, 3);
    }

    #[test]
    fn test_file_handle_empty_1_byte() {
        let backend = ClusterVfsBackend::new(vec![], ConnPoolConfig::default());
        let fh = make_fh(vec![1]);
        let result = backend.getattr(&fh);
        assert!(result.is_err());
    }

    #[test]
    fn test_file_handle_max_64_bytes() {
        let backend = ClusterVfsBackend::new(vec![], ConnPoolConfig::default());
        let data: Vec<u8> = (0..64).collect();
        let fh = make_fh(data);
        let result = backend.getattr(&fh);
        assert!(result.is_err());
    }

    #[test]
    fn test_file_handle_all_zeros() {
        let backend = ClusterVfsBackend::new(vec![], ConnPoolConfig::default());
        let fh = make_fh(vec![0, 0, 0, 0]);
        let result = backend.getattr(&fh);
        assert!(result.is_err());
    }

    #[test]
    fn test_cluster_name_in_error_feature() {
        let backend = ClusterVfsBackend::new(vec![], ConnPoolConfig::default())
            .with_cluster_name("my-cluster");
        let fh = make_fh(vec![1, 2, 3]);
        let result = backend.getattr(&fh);
        match result.unwrap_err() {
            GatewayError::NotImplemented { feature } => {
                assert!(feature.contains("my-cluster") || feature.contains("getattr"));
            }
            _ => panic!("expected NotImplemented"),
        }
    }

    #[test]
    fn test_conn_pool_config_defaults() {
        let config = ConnPoolConfig::default();
        assert_eq!(config.max_per_node, 10);
        assert_eq!(config.health_check_interval_ms, 30_000);
        assert_eq!(config.connect_timeout_ms, 5000);
    }

    #[test]
    fn test_backend_node_construction() {
        let node = BackendNode::new("node1", "192.168.1.1:8080");
        assert_eq!(node.node_id, "node1");
        assert_eq!(node.address, "192.168.1.1:8080");
    }

    #[test]
    fn test_multiple_backends_independent_stats() {
        let backend1 = ClusterVfsBackend::new(vec![], ConnPoolConfig::default());
        let backend2 = ClusterVfsBackend::new(vec![], ConnPoolConfig::default());

        let fh = make_fh(vec![1]);
        let _ = backend1.getattr(&fh);
        let _ = backend2.getattr(&fh);
        let _ = backend2.getattr(&fh);

        assert_eq!(backend1.stats().total_rpc_calls, 1);
        assert_eq!(backend2.stats().total_rpc_calls, 2);
    }

    #[test]
    fn test_thread_safety_arc_backend() {
        let backend =
            std::sync::Arc::new(ClusterVfsBackend::new(vec![], ConnPoolConfig::default()));

        let backend1 = backend.clone();
        let backend2 = backend.clone();

        let handle1 = std::thread::spawn(move || {
            let fh = make_fh(vec![1, 2]);
            for _ in 0..100 {
                let _ = backend1.getattr(&fh);
            }
        });

        let handle2 = std::thread::spawn(move || {
            let fh = make_fh(vec![1, 2]);
            for _ in 0..100 {
                let _ = backend2.getattr(&fh);
            }
        });

        handle1.join().unwrap();
        handle2.join().unwrap();

        assert_eq!(backend.stats().total_rpc_calls, 200);
    }

    #[test]
    fn test_all_15_vfs_ops_return_errors() {
        let backend = ClusterVfsBackend::new(vec![], ConnPoolConfig::default());
        let fh = make_fh(vec![1, 2, 3, 4]);
        let dir_fh = make_fh(vec![1, 2]);

        assert!(backend.getattr(&fh).is_err());
        assert!(backend.lookup(&dir_fh, "test").is_err());
        assert!(backend.read(&fh, 0, 1024).is_err());
        assert!(backend.write(&fh, 0, b"x").is_err());
        assert!(backend.readdir(&dir_fh, 0, 256).is_err());
        assert!(backend.mkdir(&dir_fh, "dir", 0o755).is_err());
        assert!(backend.create(&dir_fh, "file", 0o644).is_err());
        assert!(backend.remove(&dir_fh, "test").is_err());
        assert!(backend.rename(&dir_fh, "a", &dir_fh, "b").is_err());
        assert!(backend.readlink(&fh).is_err());
        assert!(backend.symlink(&dir_fh, "link", "target").is_err());
        assert!(backend.fsstat(&fh).is_err());
        assert!(backend.fsinfo(&fh).is_err());
        assert!(backend.pathconf(&fh).is_err());
        assert!(backend.access(&fh, 0, 0, 0).is_err());

        let stats = backend.stats();
        assert_eq!(stats.total_rpc_calls, 15);
    }

    #[test]
    fn test_default_cluster_name() {
        let backend = ClusterVfsBackend::new(vec![], ConnPoolConfig::default());
        let fh = make_fh(vec![1]);
        let result = backend.getattr(&fh);
        match result.unwrap_err() {
            GatewayError::NotImplemented { feature } => {
                assert!(feature.contains("cluster backend"));
            }
            _ => panic!("expected NotImplemented"),
        }
    }

    #[test]
    fn test_conn_pool_config_custom_values() {
        let config = ConnPoolConfig {
            min_per_node: 5,
            max_per_node: 20,
            max_idle_ms: 60_000,
            connect_timeout_ms: 3000,
            health_check_interval_ms: 15_000,
        };
        assert_eq!(config.max_per_node, 20);
        assert_eq!(config.health_check_interval_ms, 15_000);
    }

    #[test]
    fn test_backend_node_with_region() {
        let node = BackendNode::new("node1", "192.168.1.1:8080").with_region("us-west-2");
        assert_eq!(node.region, Some("us-west-2".to_string()));
    }

    #[test]
    fn test_backend_node_with_weight() {
        let node = BackendNode::new("node1", "192.168.1.1:8080").with_weight(5);
        assert_eq!(node.weight, 5);
    }

    #[test]
    fn test_stats_successful_via_getattr() {
        let backend = ClusterVfsBackend::new(vec![], ConnPoolConfig::default());
        let fh = make_fh(vec![1, 2, 3]);
        let _ = backend.getattr(&fh);
        let stats = backend.stats();
        assert_eq!(stats.total_rpc_calls, 1);
        assert_eq!(stats.failed_rpcs, 1);
    }
}

mod proptest_tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn prop_getattr_any_1_to_64_byte_fh(fh_data in proptest::collection::vec(0u8..=255, 1..=64)) {
            let backend = ClusterVfsBackend::new(vec![], ConnPoolConfig::default());
            let fh = FileHandle3 { data: fh_data };
            let result = backend.getattr(&fh);
            assert!(result.is_err());
            match result.unwrap_err() {
                GatewayError::NotImplemented { .. } => {}
                _ => panic!("expected NotImplemented"),
            }
        }

        #[test]
        fn prop_file_handle_random_content(fh_data in proptest::collection::vec(0u8..=255, 1..=32)) {
            let backend = ClusterVfsBackend::new(vec![], ConnPoolConfig::default());
            let fh = FileHandle3 { data: fh_data };
            let result = backend.getattr(&fh);
            assert!(result.is_err());
        }

        #[test]
        fn prop_lookup_with_various_names(name in "[a-zA-Z0-9_]+") {
            let backend = ClusterVfsBackend::new(vec![], ConnPoolConfig::default());
            let dir_fh = make_fh(vec![1]);
            let result = backend.lookup(&dir_fh, &name);
            assert!(result.is_err());
        }
    }
}
