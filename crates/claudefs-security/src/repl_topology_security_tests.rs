//! Replication topology management security tests.
//!
//! Part of A10 Phase 25: Repl topology security audit

use claudefs_repl::topology::{ReplicationRole, ReplicationTopology, SiteId, SiteInfo};

fn make_site(id: SiteId, name: &str, addrs: Vec<String>, role: ReplicationRole) -> SiteInfo {
    SiteInfo::new(id, name.to_string(), addrs, role)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_topology_new() {
        let topo = ReplicationTopology::new(1);
        assert_eq!(topo.local_site_id, 1);
        assert_eq!(topo.site_count(), 0);
        assert!(topo.active_sites().is_empty());
        assert!(topo.all_sites().is_empty());
    }

    #[test]
    fn test_upsert_and_get_site() {
        let mut topo = ReplicationTopology::new(1);
        let site = make_site(
            2,
            "us-west-2",
            vec!["grpc://1.2.3.4:50051".to_string()],
            ReplicationRole::Primary,
        );
        topo.upsert_site(site);

        assert_eq!(topo.site_count(), 1);

        let retrieved = topo.get_site(2).unwrap();
        assert_eq!(retrieved.name, "us-west-2");
        assert!(matches!(retrieved.role, ReplicationRole::Primary));
    }

    #[test]
    fn test_remove_site() {
        let mut topo = ReplicationTopology::new(1);
        let site = make_site(2, "us-west-2", vec![], ReplicationRole::Primary);
        topo.upsert_site(site);

        let removed = topo.remove_site(2);
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().site_id, 2);
        assert_eq!(topo.site_count(), 0);

        // FINDING-REPL-TOPO-01: removing nonexistent site returns None — no error, safe idempotent
        let removed_nonexistent = topo.remove_site(999);
        assert!(removed_nonexistent.is_none());
    }

    #[test]
    fn test_upsert_replaces_existing() {
        let mut topo = ReplicationTopology::new(1);
        let site1 = make_site(
            2,
            "us-west-2",
            vec!["addr1".to_string()],
            ReplicationRole::Primary,
        );
        topo.upsert_site(site1);

        let site2 = make_site(
            2,
            "us-west-2",
            vec!["addr2".to_string()],
            ReplicationRole::Primary,
        );
        topo.upsert_site(site2);

        assert_eq!(topo.site_count(), 1);

        let retrieved = topo.get_site(2).unwrap();
        assert_eq!(retrieved.conduit_addrs, vec!["addr2".to_string()]);
        // FINDING-REPL-TOPO-02: upsert replaces existing — allows live topology updates without remove/add
    }

    #[test]
    fn test_local_site_not_in_remote_list() {
        let topo = ReplicationTopology::new(1);
        // FINDING-REPL-TOPO-03: local site not tracked as remote — prevents self-replication loops
        assert!(topo.get_site(1).is_none());
    }

    #[test]
    fn test_active_sites_filtering() {
        let mut topo = ReplicationTopology::new(1);
        let site1 = make_site(2, "site1", vec![], ReplicationRole::Primary);
        let site2 = make_site(
            3,
            "site2",
            vec![],
            ReplicationRole::Replica { primary_site_id: 2 },
        );

        topo.upsert_site(site1);
        topo.upsert_site(site2);

        topo.deactivate(3);

        let active = topo.active_sites();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].site_id, 2);
        // FINDING-REPL-TOPO-04: deactivated sites excluded from active list — prevents replication to downed sites
    }

    #[test]
    fn test_deactivate_and_activate() {
        let mut topo = ReplicationTopology::new(1);
        let site = make_site(2, "us-west-2", vec![], ReplicationRole::Primary);
        topo.upsert_site(site);

        topo.deactivate(2);
        let retrieved = topo.get_site(2).unwrap();
        assert!(!retrieved.active);

        topo.activate(2);
        let retrieved = topo.get_site(2).unwrap();
        assert!(retrieved.active);
    }

    #[test]
    fn test_deactivate_nonexistent() {
        let mut topo = ReplicationTopology::new(1);
        // FINDING-REPL-TOPO-05: deactivate nonexistent site is silent no-op
        topo.deactivate(999);
    }

    #[test]
    fn test_activate_nonexistent() {
        let mut topo = ReplicationTopology::new(1);
        // FINDING-REPL-TOPO-06: activate nonexistent site is silent no-op
        topo.activate(999);
    }

    #[test]
    fn test_new_site_defaults_active() {
        let site = SiteInfo::new(1, "test".to_string(), vec![], ReplicationRole::Primary);
        assert!(site.active);
        assert!(site.lag_us.is_none());
        // FINDING-REPL-TOPO-07: new sites default to active — immediate participation in replication
    }

    #[test]
    fn test_role_primary() {
        let role = ReplicationRole::Primary;
        assert!(matches!(role, ReplicationRole::Primary));
    }

    #[test]
    fn test_role_replica() {
        let role = ReplicationRole::Replica { primary_site_id: 1 };
        if let ReplicationRole::Replica { primary_site_id } = role {
            assert_eq!(primary_site_id, 1);
        } else {
            panic!("expected Replica");
        }
    }

    #[test]
    fn test_role_bidirectional() {
        let role = ReplicationRole::Bidirectional;
        assert!(matches!(role, ReplicationRole::Bidirectional));
        // FINDING-REPL-TOPO-08: bidirectional role supports multi-master — enables LWW conflict resolution
    }

    #[test]
    fn test_role_equality() {
        assert_eq!(ReplicationRole::Primary, ReplicationRole::Primary);
        assert_ne!(ReplicationRole::Primary, ReplicationRole::Bidirectional);
        assert_eq!(
            ReplicationRole::Replica { primary_site_id: 1 },
            ReplicationRole::Replica { primary_site_id: 1 }
        );
        assert_ne!(
            ReplicationRole::Replica { primary_site_id: 1 },
            ReplicationRole::Replica { primary_site_id: 2 }
        );
    }

    #[test]
    fn test_mixed_roles_topology() {
        let mut topo = ReplicationTopology::new(1);

        topo.upsert_site(make_site(2, "primary", vec![], ReplicationRole::Primary));
        topo.upsert_site(make_site(
            3,
            "replica",
            vec![],
            ReplicationRole::Replica { primary_site_id: 2 },
        ));
        topo.upsert_site(make_site(
            4,
            "bidirectional",
            vec![],
            ReplicationRole::Bidirectional,
        ));

        assert_eq!(topo.all_sites().len(), 3);

        let primary = topo.get_site(2).unwrap();
        let replica = topo.get_site(3).unwrap();
        let bidir = topo.get_site(4).unwrap();

        assert!(matches!(primary.role, ReplicationRole::Primary));
        assert!(matches!(
            replica.role,
            ReplicationRole::Replica { primary_site_id: 2 }
        ));
        assert!(matches!(bidir.role, ReplicationRole::Bidirectional));
    }

    #[test]
    fn test_update_lag() {
        let mut topo = ReplicationTopology::new(1);
        let site = make_site(2, "us-west-2", vec![], ReplicationRole::Primary);
        topo.upsert_site(site);

        topo.update_lag(2, 5000);
        let retrieved = topo.get_site(2).unwrap();
        assert_eq!(retrieved.lag_us, Some(5000));

        topo.update_lag(2, 0);
        let retrieved = topo.get_site(2).unwrap();
        assert_eq!(retrieved.lag_us, Some(0));
        // FINDING-REPL-TOPO-09: lag tracking enables replication health monitoring
    }

    #[test]
    fn test_update_lag_nonexistent() {
        let mut topo = ReplicationTopology::new(1);
        // FINDING-REPL-TOPO-10: update lag on nonexistent site is silent no-op
        topo.update_lag(999, 5000);
        assert!(topo.get_site(999).is_none());
    }

    #[test]
    fn test_initial_lag_none() {
        let mut topo = ReplicationTopology::new(1);
        let site = make_site(2, "us-west-2", vec![], ReplicationRole::Primary);
        topo.upsert_site(site);

        let retrieved = topo.get_site(2).unwrap();
        assert!(retrieved.lag_us.is_none());
        // FINDING-REPL-TOPO-11: None lag distinguishes "never measured" from "measured as 0" — cleaner health reporting
    }

    #[test]
    fn test_lag_update_preserves_other_fields() {
        let mut topo = ReplicationTopology::new(1);
        let site = make_site(
            2,
            "us-west-2",
            vec!["addr1".to_string()],
            ReplicationRole::Primary,
        );
        topo.upsert_site(site);

        topo.update_lag(2, 1000);

        let retrieved = topo.get_site(2).unwrap();
        assert_eq!(retrieved.name, "us-west-2");
        assert_eq!(retrieved.conduit_addrs, vec!["addr1".to_string()]);
        assert!(retrieved.active);
    }

    #[test]
    fn test_lag_across_multiple_sites() {
        let mut topo = ReplicationTopology::new(1);

        topo.upsert_site(make_site(2, "site2", vec![], ReplicationRole::Primary));
        topo.upsert_site(make_site(3, "site3", vec![], ReplicationRole::Primary));
        topo.upsert_site(make_site(4, "site4", vec![], ReplicationRole::Primary));

        topo.update_lag(2, 1000);
        topo.update_lag(3, 5000);
        topo.update_lag(4, 10000);

        assert_eq!(topo.get_site(2).unwrap().lag_us, Some(1000));
        assert_eq!(topo.get_site(3).unwrap().lag_us, Some(5000));
        assert_eq!(topo.get_site(4).unwrap().lag_us, Some(10000));
    }

    #[test]
    fn test_multiple_conduit_addrs() {
        let mut topo = ReplicationTopology::new(1);
        let site = make_site(
            2,
            "us-west-2",
            vec![
                "grpc://1.2.3.4:50051".to_string(),
                "grpc://1.2.3.5:50051".to_string(),
                "grpc://1.2.3.6:50051".to_string(),
            ],
            ReplicationRole::Primary,
        );
        topo.upsert_site(site);

        let retrieved = topo.get_site(2).unwrap();
        assert_eq!(retrieved.conduit_addrs.len(), 3);
    }

    #[test]
    fn test_empty_conduit_addrs() {
        let mut topo = ReplicationTopology::new(1);
        let site = make_site(2, "us-west-2", vec![], ReplicationRole::Primary);
        topo.upsert_site(site);

        let retrieved = topo.get_site(2).unwrap();
        assert!(retrieved.conduit_addrs.is_empty());
        // FINDING-REPL-TOPO-12: empty addrs allowed — site may be discovered later or use mDNS
    }

    #[test]
    fn test_all_sites_vs_active_sites() {
        let mut topo = ReplicationTopology::new(1);

        topo.upsert_site(make_site(2, "site2", vec![], ReplicationRole::Primary));
        topo.upsert_site(make_site(3, "site3", vec![], ReplicationRole::Primary));
        topo.upsert_site(make_site(4, "site4", vec![], ReplicationRole::Primary));

        topo.deactivate(4);

        assert_eq!(topo.all_sites().len(), 3);
        assert_eq!(topo.active_sites().len(), 2);
        // FINDING-REPL-TOPO-13: all_sites includes inactive — allows admin visibility of full topology
    }

    #[test]
    fn test_topology_isolation() {
        let mut topo1 = ReplicationTopology::new(1);
        let mut topo2 = ReplicationTopology::new(2);

        topo1.upsert_site(make_site(3, "site3", vec![], ReplicationRole::Primary));
        topo2.upsert_site(make_site(
            4,
            "site4",
            vec![],
            ReplicationRole::Replica { primary_site_id: 3 },
        ));

        assert_eq!(topo1.site_count(), 1);
        assert_eq!(topo2.site_count(), 1);
        assert!(topo1.get_site(3).is_some());
        assert!(topo2.get_site(4).is_some());
        assert!(topo1.get_site(4).is_none());
        assert!(topo2.get_site(3).is_none());
    }

    #[test]
    fn test_site_info_fields() {
        let site = SiteInfo::new(
            42,
            "us-east-1".to_string(),
            vec!["grpc://10.0.0.1:50051".to_string()],
            ReplicationRole::Replica { primary_site_id: 1 },
        );

        assert_eq!(site.site_id, 42);
        assert_eq!(site.name, "us-east-1");
        assert_eq!(site.conduit_addrs.len(), 1);
        assert!(matches!(
            site.role,
            ReplicationRole::Replica { primary_site_id: 1 }
        ));
        assert!(site.active);
        assert!(site.lag_us.is_none());
    }
}
