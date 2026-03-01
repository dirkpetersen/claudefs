//! Site and peer topology management for cross-site replication.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Unique identifier for a replication site (e.g., "us-west-2").
pub type SiteId = u64;

/// Unique identifier for a storage node within a site.
pub type NodeId = u64;

/// Replication role of this node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "role", content = "primary_site_id")]
pub enum ReplicationRole {
    /// Primary site — originates writes, pushes journal to replicas.
    Primary,
    /// Replica site — receives journal from primary, applies locally.
    Replica {
        /// The primary site this replica follows.
        primary_site_id: SiteId,
    },
    /// Bidirectional — both sites can write; uses LWW conflict resolution.
    Bidirectional,
}

/// Information about a remote replication site.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SiteInfo {
    /// Unique site identifier.
    pub site_id: SiteId,
    /// Human-readable name (e.g., "us-west-2").
    pub name: String,
    /// gRPC endpoints for the conduit server.
    pub conduit_addrs: Vec<String>,
    /// Replication role.
    pub role: ReplicationRole,
    /// True = replication is enabled.
    pub active: bool,
    /// Latest measured replication lag in microseconds.
    pub lag_us: Option<u64>,
}

impl SiteInfo {
    /// Create a new site info.
    pub fn new(
        site_id: SiteId,
        name: String,
        conduit_addrs: Vec<String>,
        role: ReplicationRole,
    ) -> Self {
        Self {
            site_id,
            name,
            conduit_addrs,
            role,
            active: true,
            lag_us: None,
        }
    }
}

/// Manages the topology of known replication sites and their state.
#[derive(Debug)]
pub struct ReplicationTopology {
    /// The local site ID.
    pub local_site_id: SiteId,
    sites: HashMap<SiteId, SiteInfo>,
}

impl ReplicationTopology {
    /// Create a new topology with the given local site ID.
    pub fn new(local_site_id: SiteId) -> Self {
        Self {
            local_site_id,
            sites: HashMap::new(),
        }
    }

    /// Add or update a remote site.
    pub fn upsert_site(&mut self, info: SiteInfo) {
        self.sites.insert(info.site_id, info);
    }

    /// Remove a remote site.
    pub fn remove_site(&mut self, site_id: SiteId) -> Option<SiteInfo> {
        self.sites.remove(&site_id)
    }

    /// Get info for a specific site.
    pub fn get_site(&self, site_id: SiteId) -> Option<&SiteInfo> {
        self.sites.get(&site_id)
    }

    /// List all active remote sites (not the local site).
    pub fn active_sites(&self) -> Vec<&SiteInfo> {
        self.sites.values().filter(|s| s.active).collect()
    }

    /// Update the measured replication lag for a site.
    pub fn update_lag(&mut self, site_id: SiteId, lag_us: u64) {
        if let Some(site) = self.sites.get_mut(&site_id) {
            site.lag_us = Some(lag_us);
        }
    }

    /// Mark a site as inactive (e.g., conduit is down).
    pub fn deactivate(&mut self, site_id: SiteId) {
        if let Some(site) = self.sites.get_mut(&site_id) {
            site.active = false;
        }
    }

    /// Mark a site as active.
    pub fn activate(&mut self, site_id: SiteId) {
        if let Some(site) = self.sites.get_mut(&site_id) {
            site.active = true;
        }
    }

    /// Return the number of known remote sites.
    pub fn site_count(&self) -> usize {
        self.sites.len()
    }

    /// Get all sites (for iteration).
    pub fn all_sites(&self) -> Vec<&SiteInfo> {
        self.sites.values().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_remove_sites() {
        let mut topo = ReplicationTopology::new(1);

        let site = SiteInfo::new(
            2,
            "us-west-2".to_string(),
            vec!["grpc://1.2.3.4:50051".to_string()],
            ReplicationRole::Primary,
        );
        topo.upsert_site(site);

        assert_eq!(topo.site_count(), 1);
        assert!(topo.get_site(2).is_some());

        let removed = topo.remove_site(2);
        assert!(removed.is_some());
        assert_eq!(topo.site_count(), 0);
    }

    #[test]
    fn test_active_filtering() {
        let mut topo = ReplicationTopology::new(1);

        let site1 = SiteInfo::new(2, "us-west-2".to_string(), vec![], ReplicationRole::Primary);
        let mut site2 = SiteInfo::new(
            3,
            "us-east-1".to_string(),
            vec![],
            ReplicationRole::Replica { primary_site_id: 1 },
        );
        site2.active = false;

        topo.upsert_site(site1);
        topo.upsert_site(site2);

        let active = topo.active_sites();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].site_id, 2);
    }

    #[test]
    fn test_lag_update() {
        let mut topo = ReplicationTopology::new(1);

        let site = SiteInfo::new(2, "us-west-2".to_string(), vec![], ReplicationRole::Primary);
        topo.upsert_site(site);

        topo.update_lag(2, 5000);

        let retrieved = topo.get_site(2).unwrap();
        assert_eq!(retrieved.lag_us, Some(5000));
    }

    #[test]
    fn test_deactivate_activate() {
        let mut topo = ReplicationTopology::new(1);

        let site = SiteInfo::new(2, "us-west-2".to_string(), vec![], ReplicationRole::Primary);
        topo.upsert_site(site);

        assert!(topo.active_sites().len() == 1);

        topo.deactivate(2);
        assert!(topo.active_sites().is_empty());

        topo.activate(2);
        assert_eq!(topo.active_sites().len(), 1);
    }

    #[test]
    fn test_duplicate_upsert() {
        let mut topo = ReplicationTopology::new(1);

        let site1 = SiteInfo::new(
            2,
            "us-west-2".to_string(),
            vec!["addr1".to_string()],
            ReplicationRole::Primary,
        );
        topo.upsert_site(site1);

        let site2 = SiteInfo::new(
            2,
            "us-west-2".to_string(),
            vec!["addr2".to_string()],
            ReplicationRole::Bidirectional,
        );
        topo.upsert_site(site2);

        assert_eq!(topo.site_count(), 1);
        let retrieved = topo.get_site(2).unwrap();
        assert_eq!(retrieved.conduit_addrs, vec!["addr2".to_string()]);
    }

    #[test]
    fn test_bidirectional_role() {
        let mut topo = ReplicationTopology::new(1);

        let site = SiteInfo::new(
            2,
            "us-east-1".to_string(),
            vec![],
            ReplicationRole::Bidirectional,
        );
        topo.upsert_site(site);

        let retrieved = topo.get_site(2).unwrap();
        assert_eq!(retrieved.role, ReplicationRole::Bidirectional);
    }

    #[test]
    fn test_replica_role() {
        let mut topo = ReplicationTopology::new(1);

        let site = SiteInfo::new(
            2,
            "us-east-1".to_string(),
            vec![],
            ReplicationRole::Replica { primary_site_id: 1 },
        );
        topo.upsert_site(site);

        let retrieved = topo.get_site(2).unwrap();
        if let ReplicationRole::Replica { primary_site_id } = retrieved.role {
            assert_eq!(primary_site_id, 1);
        } else {
            panic!("expected Replica role");
        }
    }

    #[test]
    fn test_local_site_not_in_remote_list() {
        let topo = ReplicationTopology::new(1);

        assert!(topo.get_site(1).is_none());
    }

    #[test]
    fn test_all_sites() {
        let mut topo = ReplicationTopology::new(1);

        topo.upsert_site(SiteInfo::new(
            2,
            "site2".to_string(),
            vec![],
            ReplicationRole::Primary,
        ));
        topo.upsert_site(SiteInfo::new(
            3,
            "site3".to_string(),
            vec![],
            ReplicationRole::Primary,
        ));

        let all = topo.all_sites();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_multiple_conduit_addrs() {
        let mut topo = ReplicationTopology::new(1);

        let site = SiteInfo::new(
            2,
            "us-west-2".to_string(),
            vec![
                "grpc://1.2.3.4:50051".to_string(),
                "grpc://1.2.3.5:50051".to_string(),
            ],
            ReplicationRole::Primary,
        );
        topo.upsert_site(site);

        let retrieved = topo.get_site(2).unwrap();
        assert_eq!(retrieved.conduit_addrs.len(), 2);
    }

    #[test]
    fn test_remove_nonexistent() {
        let mut topo = ReplicationTopology::new(1);
        let removed = topo.remove_site(999);
        assert!(removed.is_none());
    }

    #[test]
    fn test_update_lag_nonexistent() {
        let mut topo = ReplicationTopology::new(1);
        topo.update_lag(999, 5000);
        assert!(topo.get_site(999).is_none());
    }

    #[test]
    fn test_activate_deactivate_nonexistent() {
        let mut topo = ReplicationTopology::new(1);
        topo.activate(999);
        topo.deactivate(999);
    }

    #[test]
    fn test_site_info_default_active() {
        let site = SiteInfo::new(1, "test".to_string(), vec![], ReplicationRole::Primary);
        assert!(site.active);
    }

    #[test]
    fn test_site_info_default_lag_none() {
        let site = SiteInfo::new(1, "test".to_string(), vec![], ReplicationRole::Primary);
        assert!(site.lag_us.is_none());
    }

    #[test]
    fn test_local_site_id_accessible() {
        let topo = ReplicationTopology::new(42);
        assert_eq!(topo.local_site_id, 42);
    }
}
