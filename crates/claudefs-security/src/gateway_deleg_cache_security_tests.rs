//! Gateway delegation, NFS cache, and SMB multichannel security tests.
//!
//! Part of A10 Phase 16: Gateway delegation & cache security audit

#[cfg(test)]
mod tests {
    use claudefs_gateway::nfs_cache::AttrCache;
    use claudefs_gateway::nfs_delegation::{
        Delegation, DelegationError, DelegationId, DelegationManager, DelegationState,
        DelegationType,
    };
    use claudefs_gateway::protocol::{Fattr3, FileHandle3, Ftype3, Nfstime3};
    use claudefs_gateway::smb_multichannel::{
        ChannelInfo, ChannelSelectionPolicy, InterfaceCapabilities, MultichannelConfig,
        MultichannelError, MultichannelManager, MultichannelRole, MultichannelSession,
        NicCapabilities,
    };
    use std::collections::HashSet;
    use std::time::Duration;

    fn make_file_handle(data: &[u8]) -> FileHandle3 {
        FileHandle3::new(data.to_vec()).unwrap()
    }

    fn make_fattr(fileid: u64) -> Fattr3 {
        Fattr3 {
            ftype: Ftype3::Reg,
            mode: 0o644,
            nlink: 1,
            uid: 0,
            gid: 0,
            size: 4096,
            used: 4096,
            rdev: (0, 0),
            fsid: 1,
            fileid,
            atime: Nfstime3::zero(),
            mtime: Nfstime3::zero(),
            ctime: Nfstime3::zero(),
        }
    }

    // ============================================================================
    // Category 1: NFSv4 Delegation Management (5 tests)
    // ============================================================================

    #[test]
    fn test_delegation_write_conflict() {
        let mut manager = DelegationManager::new();

        let id1 = manager
            .grant(1, 100, DelegationType::Write)
            .expect("first write delegation should succeed");
        assert!(manager.get(&id1).is_some());

        let result = manager.grant(1, 200, DelegationType::Write);
        assert!(matches!(result, Err(DelegationError::WriteConflict(1))));

        let result = manager.grant(1, 200, DelegationType::Read);
        assert!(matches!(result, Err(DelegationError::WriteConflict(1))));
        // FINDING-GW-DELEG-01: write delegation enforces exclusivity
    }

    #[test]
    fn test_delegation_recall_state_machine() {
        let mut manager = DelegationManager::new();

        let id1 = manager
            .grant(1, 100, DelegationType::Read)
            .expect("read delegation should succeed");

        let recalled_ids = manager.recall_file(1);
        assert_eq!(recalled_ids.len(), 1);
        assert!(recalled_ids.contains(&id1));

        let delegation = manager.get(&id1).expect("delegation should exist");
        assert!(matches!(delegation.state, DelegationState::RecallPending));
        assert!(!delegation.is_active());

        let id2 = manager
            .grant(2, 100, DelegationType::Read)
            .expect("second delegation should succeed");

        let recalled_ids = manager.recall_file(2);
        assert_eq!(recalled_ids.len(), 1);

        manager
            .return_delegation(&id2)
            .expect("return should succeed");

        let delegation = manager.get(&id2).expect("delegation should exist");
        assert!(matches!(delegation.state, DelegationState::Returned));
    }

    #[test]
    fn test_delegation_revoke_client() {
        let mut manager = DelegationManager::new();

        manager.grant(1, 100, DelegationType::Read).expect("grant");
        manager.grant(2, 100, DelegationType::Read).expect("grant");
        manager.grant(3, 100, DelegationType::Read).expect("grant");
        manager.grant(4, 200, DelegationType::Read).expect("grant");

        let revoked_ids = manager.revoke_client(100);
        assert_eq!(revoked_ids.len(), 3);

        for id in &revoked_ids {
            let delegation = manager.get(id).expect("delegation should exist");
            assert!(matches!(delegation.state, DelegationState::Revoked));
        }

        let file4_delegations = manager.file_delegations(4);
        assert_eq!(file4_delegations.len(), 1);
        assert!(file4_delegations[0].is_active());
    }

    #[test]
    fn test_delegation_double_return_error() {
        let mut manager = DelegationManager::new();

        let id = manager
            .grant(1, 100, DelegationType::Read)
            .expect("delegation should be granted");

        manager
            .return_delegation(&id)
            .expect("first return should succeed");

        let result = manager.return_delegation(&id);
        assert!(matches!(result, Err(DelegationError::AlreadyReturned)));
        // FINDING-GW-DELEG-07: prevents double-free of delegation state
    }

    #[test]
    fn test_delegation_id_uniqueness() {
        let mut ids = HashSet::new();
        for _ in 0..50 {
            let id = DelegationId::generate();
            ids.insert(id.clone());
            assert_eq!(id.as_hex().len(), 32);
        }
        assert_eq!(ids.len(), 50);
    }

    // ============================================================================
    // Category 2: NFS Attribute Cache (5 tests)
    // ============================================================================

    #[test]
    fn test_attr_cache_insert_and_get() {
        let cache = AttrCache::new(100, 60);
        let fh = make_file_handle(b"file1");
        let attr = make_fattr(42);

        cache.insert(&fh, attr.clone());

        let result = cache.get(&fh);
        assert!(result.is_some());
        assert_eq!(result.unwrap().fileid, 42);

        let missing = make_file_handle(b"nonexistent");
        assert!(cache.get(&missing).is_none());
    }

    #[test]
    fn test_attr_cache_capacity_eviction() {
        let cache = AttrCache::new(2, 60);

        cache.insert(&make_file_handle(b"file1"), make_fattr(1));
        cache.insert(&make_file_handle(b"file2"), make_fattr(2));
        cache.insert(&make_file_handle(b"file3"), make_fattr(3));

        assert!(cache.len() <= 2);
        // FINDING-GW-DELEG-02: capacity limit prevents unbounded memory growth
    }

    #[test]
    fn test_attr_cache_hit_rate() {
        let cache = AttrCache::new(100, 60);
        let fh = make_file_handle(b"file1");
        let attr = make_fattr(1);

        cache.insert(&fh, attr);

        cache.get(&fh);
        cache.get(&fh);
        cache.get(&fh);
        cache.get(&make_file_handle(b"missing"));

        let hit_rate = cache.hit_rate();
        assert!((hit_rate - 0.75).abs() < 0.01);

        let stats = cache.stats();
        assert_eq!(stats, (3, 1, 1));

        let empty_cache = AttrCache::new(10, 60);
        assert_eq!(empty_cache.hit_rate(), 0.0);
    }

    #[test]
    fn test_attr_cache_invalidation() {
        let cache = AttrCache::new(100, 60);

        cache.insert(&make_file_handle(b"file1"), make_fattr(1));
        cache.insert(&make_file_handle(b"file2"), make_fattr(2));
        cache.insert(&make_file_handle(b"file3"), make_fattr(3));

        cache.invalidate(&make_file_handle(b"file2"));
        assert_eq!(cache.len(), 2);
        assert!(cache.get(&make_file_handle(b"file2")).is_none());

        cache.invalidate_all();
        assert!(cache.is_empty());
    }

    #[test]
    fn test_attr_cache_custom_ttl() {
        let cache = AttrCache::new(100, 60);

        cache.insert_with_ttl(
            &make_file_handle(b"short_ttl"),
            make_fattr(1),
            Duration::from_millis(1),
        );

        std::thread::sleep(Duration::from_millis(10));
        assert!(cache.get(&make_file_handle(b"short_ttl")).is_none());

        cache.insert(&make_file_handle(b"default_ttl"), make_fattr(2));
        assert!(cache.get(&make_file_handle(b"default_ttl")).is_some());
        // FINDING-GW-DELEG-03: per-entry TTL override works correctly
    }

    // ============================================================================
    // Category 3: SMB Multichannel Config (5 tests)
    // ============================================================================

    #[test]
    fn test_multichannel_config_defaults() {
        let config = MultichannelConfig::default();

        assert!(!config.enabled);
        assert_eq!(config.max_channels, 8);
        assert_eq!(config.min_channels, 2);
        assert!(!config.prefer_rdma);
        assert!(config.interfaces.is_empty());
        assert!(matches!(
            config.channel_selection,
            ChannelSelectionPolicy::RoundRobin
        ));
    }

    #[test]
    fn test_multichannel_config_builder() {
        let config = MultichannelConfig::new()
            .with_enabled(true)
            .with_max_channels(4)
            .with_min_channels(1)
            .with_prefer_rdma(true)
            .with_channel_selection(ChannelSelectionPolicy::PreferRdma);

        assert!(config.enabled);
        assert_eq!(config.max_channels, 4);
        assert_eq!(config.min_channels, 1);
        assert!(config.prefer_rdma);
        assert!(matches!(
            config.channel_selection,
            ChannelSelectionPolicy::PreferRdma
        ));
    }

    #[test]
    fn test_nic_capabilities_builder() {
        let nic = NicCapabilities::new("eth0".to_string(), "192.168.1.1".to_string())
            .with_speed(10000)
            .with_port(8445)
            .with_rdma()
            .with_rss()
            .with_tso()
            .with_checksum_offload()
            .with_enabled(true);

        assert_eq!(nic.interface_name, "eth0");
        assert_eq!(nic.ip_address, "192.168.1.1");
        assert_eq!(nic.port, 8445);
        assert_eq!(nic.link_speed_mbps, 10000);
        assert!(nic.capabilities.rdma);
        assert!(nic.capabilities.rss);
        assert!(nic.capabilities.tso);
        assert!(nic.capabilities.checksum_offload);
        assert!(nic.enabled);

        let default_port_nic = NicCapabilities::new("eth1".to_string(), "192.168.1.2".to_string());
        assert_eq!(default_port_nic.port, 445);
    }

    #[test]
    fn test_interface_capabilities_default() {
        let caps = InterfaceCapabilities::default();

        assert!(!caps.rdma);
        assert!(!caps.rss);
        assert!(!caps.tso);
        assert!(!caps.checksum_offload);
    }

    #[test]
    fn test_multichannel_disabled_returns_empty() {
        let config = MultichannelConfig::new().with_enabled(false);
        let mut manager = MultichannelManager::new(config);

        manager
            .add_interface(NicCapabilities::new(
                "eth0".to_string(),
                "192.168.1.1".to_string(),
            ))
            .unwrap();

        let selected = manager.select_interfaces_for_client(3);
        assert!(selected.is_empty());
        // FINDING-GW-DELEG-04: disabled multichannel prevents channel allocation
    }

    // ============================================================================
    // Category 4: SMB Multichannel Interface Selection (5 tests)
    // ============================================================================

    #[test]
    fn test_multichannel_duplicate_interface() {
        let config = MultichannelConfig::new().with_enabled(true);
        let mut manager = MultichannelManager::new(config);

        let nic =
            NicCapabilities::new("eth0".to_string(), "192.168.1.1".to_string()).with_speed(1000);
        assert!(manager.add_interface(nic.clone()).is_ok());
        assert!(matches!(
            manager.add_interface(nic),
            Err(MultichannelError::DuplicateInterface(_))
        ));
        // FINDING-GW-DELEG-05: duplicate detection prevents confusion
    }

    #[test]
    fn test_multichannel_weighted_by_speed() {
        let config = MultichannelConfig::new()
            .with_enabled(true)
            .with_channel_selection(ChannelSelectionPolicy::WeightedBySpeed);
        let mut manager = MultichannelManager::new(config);

        manager
            .add_interface(
                NicCapabilities::new("eth0".to_string(), "192.168.1.1".to_string())
                    .with_speed(1000),
            )
            .unwrap();
        manager
            .add_interface(
                NicCapabilities::new("eth1".to_string(), "192.168.1.2".to_string())
                    .with_speed(10000),
            )
            .unwrap();
        manager
            .add_interface(
                NicCapabilities::new("eth2".to_string(), "192.168.1.3".to_string())
                    .with_speed(5000),
            )
            .unwrap();

        let selected = manager.select_interfaces_for_client(2);
        assert_eq!(selected.len(), 2);
        assert_eq!(selected[0].interface_name, "eth1");
        assert_eq!(selected[1].interface_name, "eth2");
    }

    #[test]
    fn test_multichannel_prefer_rdma() {
        let config = MultichannelConfig::new()
            .with_enabled(true)
            .with_channel_selection(ChannelSelectionPolicy::PreferRdma);
        let mut manager = MultichannelManager::new(config);

        manager
            .add_interface(
                NicCapabilities::new("eth0".to_string(), "192.168.1.1".to_string())
                    .with_speed(10000)
                    .with_rdma(),
            )
            .unwrap();
        manager
            .add_interface(
                NicCapabilities::new("eth1".to_string(), "192.168.1.2".to_string())
                    .with_speed(5000)
                    .with_rdma(),
            )
            .unwrap();
        manager
            .add_interface(
                NicCapabilities::new("eth2".to_string(), "192.168.1.3".to_string())
                    .with_speed(25000),
            )
            .unwrap();

        let selected = manager.select_interfaces_for_client(3);
        assert_eq!(selected.len(), 3);
        assert!(selected[0].capabilities.rdma);
    }

    #[test]
    fn test_multichannel_pin_to_interface() {
        let config = MultichannelConfig::new()
            .with_enabled(true)
            .with_channel_selection(ChannelSelectionPolicy::PinToInterface("eth1".to_string()));
        let mut manager = MultichannelManager::new(config);

        manager
            .add_interface(NicCapabilities::new(
                "eth0".to_string(),
                "192.168.1.1".to_string(),
            ))
            .unwrap();
        manager
            .add_interface(NicCapabilities::new(
                "eth1".to_string(),
                "192.168.1.2".to_string(),
            ))
            .unwrap();
        manager
            .add_interface(NicCapabilities::new(
                "eth2".to_string(),
                "192.168.1.3".to_string(),
            ))
            .unwrap();

        let selected = manager.select_interfaces_for_client(5);
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].interface_name, "eth1");

        let pin_nonexistent_config = MultichannelConfig::new()
            .with_enabled(true)
            .with_channel_selection(ChannelSelectionPolicy::PinToInterface("eth99".to_string()));
        let mut manager = MultichannelManager::new(pin_nonexistent_config);
        manager
            .add_interface(NicCapabilities::new(
                "eth0".to_string(),
                "192.168.1.1".to_string(),
            ))
            .unwrap();

        let selected = manager.select_interfaces_for_client(5);
        assert!(selected.is_empty());
    }

    #[test]
    fn test_multichannel_remove_interface() {
        let config = MultichannelConfig::new().with_enabled(true);
        let mut manager = MultichannelManager::new(config);

        manager
            .add_interface(NicCapabilities::new(
                "eth0".to_string(),
                "192.168.1.1".to_string(),
            ))
            .unwrap();
        manager
            .add_interface(NicCapabilities::new(
                "eth1".to_string(),
                "192.168.1.2".to_string(),
            ))
            .unwrap();

        assert!(manager.remove_interface("eth0"));
        assert!(!manager.remove_interface("eth0"));
        assert_eq!(manager.available_interfaces().len(), 1);
    }

    // ============================================================================
    // Category 5: SMB Sessions & Edge Cases (5 tests)
    // ============================================================================

    #[test]
    fn test_multichannel_session_lifecycle() {
        let config = MultichannelConfig::new().with_enabled(true);
        let mut manager = MultichannelManager::new(config);

        manager.create_session(1);
        assert!(manager.get_session(1).is_some());

        manager.create_session(2);
        assert_eq!(manager.session_count(), 2);

        assert!(manager.remove_session(1));
        assert!(manager.get_session(1).is_none());
        assert_eq!(manager.session_count(), 1);
    }

    #[test]
    fn test_multichannel_session_stats() {
        let mut session = MultichannelSession::new(1);

        session.add_channel(ChannelInfo {
            channel_id: 0,
            interface: "eth0".to_string(),
            role: MultichannelRole::Primary,
            is_active: true,
            bytes_sent: 0,
            bytes_received: 0,
        });

        session.update_stats(0, 1000, 2000);

        assert_eq!(session.total_bytes_sent, 1000);
        assert_eq!(session.total_bytes_received, 2000);

        let channel = &session.channels[0];
        assert_eq!(channel.bytes_sent, 1000);
        assert_eq!(channel.bytes_received, 2000);
    }

    #[test]
    fn test_multichannel_available_filters_disabled() {
        let config = MultichannelConfig::new().with_enabled(true);
        let mut manager = MultichannelManager::new(config);

        manager
            .add_interface(
                NicCapabilities::new("eth0".to_string(), "192.168.1.1".to_string())
                    .with_enabled(true),
            )
            .unwrap();
        manager
            .add_interface(
                NicCapabilities::new("eth1".to_string(), "192.168.1.2".to_string())
                    .with_enabled(false),
            )
            .unwrap();
        manager
            .add_interface(
                NicCapabilities::new("eth2".to_string(), "192.168.1.3".to_string())
                    .with_enabled(true),
            )
            .unwrap();

        assert_eq!(manager.available_interfaces().len(), 2);
    }

    #[test]
    fn test_delegation_count_tracking() {
        let mut manager = DelegationManager::new();

        let id1 = manager.grant(1, 100, DelegationType::Read).unwrap();
        let id2 = manager.grant(2, 100, DelegationType::Read).unwrap();
        let _id3 = manager.grant(3, 100, DelegationType::Read).unwrap();

        assert_eq!(manager.active_count(), 3);

        manager.return_delegation(&id1).unwrap();
        assert_eq!(manager.active_count(), 2);
        assert_eq!(manager.total_count(), 3);

        manager.revoke_client(100);
        assert_eq!(manager.active_count(), 0);
    }

    #[test]
    fn test_delegation_file_delegations() {
        let mut manager = DelegationManager::new();

        manager.grant(1, 100, DelegationType::Read).unwrap();
        manager.grant(1, 200, DelegationType::Read).unwrap();
        manager.grant(2, 300, DelegationType::Write).unwrap();

        let file1_delegations = manager.file_delegations(1);
        assert_eq!(file1_delegations.len(), 2);

        let file999_delegations = manager.file_delegations(999);
        assert!(file999_delegations.is_empty());
        // FINDING-GW-DELEG-06: file-scoped queries filter correctly
    }
}
