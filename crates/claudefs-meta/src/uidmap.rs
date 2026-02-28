//! UID/GID mapping for cross-site replication.
//!
//! When metadata is replicated from a remote site, UIDs must be translated
//! to local identities. Per docs/metadata.md: mapping happens at the receiving
//! site, not the source. GIDs are shared across sites (no mapping needed).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::RwLock;

use crate::types::*;

/// A UID mapping entry for cross-site replication.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UidMapping {
    /// Source site identifier.
    pub site_id: u64,
    /// Canonical UID from the source site.
    pub canonical_uid: u32,
    /// Local UID on the receiving site.
    pub local_uid: u32,
    /// Optional description for the mapping.
    pub description: Option<String>,
}

/// Manages UID/GID mappings for cross-site replication.
pub struct UidMapManager {
    /// Mapping table from (site_id, canonical_uid) to local_uid.
    mappings: RwLock<HashMap<(u64, u32), u32>>,
    /// Full mapping entries with descriptions.
    entries: RwLock<Vec<UidMapping>>,
}

impl UidMapManager {
    /// Creates a new UID map manager with no mappings.
    pub fn new() -> Self {
        Self {
            mappings: RwLock::new(HashMap::new()),
            entries: RwLock::new(Vec::new()),
        }
    }

    /// Adds a new UID mapping.
    ///
    /// # Arguments
    /// * `site_id` - Source site identifier
    /// * `canonical_uid` - Canonical UID from source site
    /// * `local_uid` - Local UID on receiving site
    /// * `description` - Optional description
    ///
    /// # Returns
    /// Ok(()) on success
    pub fn add_mapping(
        &self,
        site_id: u64,
        canonical_uid: u32,
        local_uid: u32,
        description: Option<String>,
    ) -> Result<(), MetaError> {
        let key = (site_id, canonical_uid);

        let mut mappings = self.mappings.write().unwrap();
        mappings.insert(key, local_uid);

        let mut entries = self.entries.write().unwrap();
        entries.push(UidMapping {
            site_id,
            canonical_uid,
            local_uid,
            description,
        });

        Ok(())
    }

    /// Removes a UID mapping.
    ///
    /// # Arguments
    /// * `site_id` - Source site identifier
    /// * `canonical_uid` - Canonical UID to remove
    ///
    /// # Returns
    /// Ok(true) if mapping existed, Ok(false) otherwise
    pub fn remove_mapping(&self, site_id: u64, canonical_uid: u32) -> Result<bool, MetaError> {
        let key = (site_id, canonical_uid);

        let removed = {
            let mut mappings = self.mappings.write().unwrap();
            mappings.remove(&key).is_some()
        };

        if removed {
            let mut entries = self.entries.write().unwrap();
            entries.retain(|e| !(e.site_id == site_id && e.canonical_uid == canonical_uid));
        }

        Ok(removed)
    }

    /// Maps a canonical UID to a local UID.
    ///
    /// If no mapping exists, returns the canonical_uid as passthrough.
    /// Note: UID 0 is always passed through as 0 (root never maps).
    ///
    /// # Arguments
    /// * `site_id` - Source site identifier
    /// * `canonical_uid` - Canonical UID to map
    ///
    /// # Returns
    /// The local UID
    pub fn map_uid(&self, site_id: u64, canonical_uid: u32) -> u32 {
        if canonical_uid == 0 {
            return 0;
        }

        let mappings = self.mappings.read().unwrap();
        *mappings
            .get(&(site_id, canonical_uid))
            .unwrap_or(&canonical_uid)
    }

    /// Maps a GID (passthrough - GIDs are shared across sites).
    ///
    /// # Arguments
    /// * `_site_id` - Source site identifier (unused, GIDs are shared)
    /// * `canonical_gid` - Canonical GID
    ///
    /// # Returns
    /// The same GID (passthrough)
    pub fn map_gid(&self, _site_id: u64, canonical_gid: u32) -> u32 {
        canonical_gid
    }

    /// Returns all mappings for a specific site.
    ///
    /// # Arguments
    /// * `site_id` - Site identifier
    ///
    /// # Returns
    /// Vector of UID mappings for the site
    pub fn mappings_for_site(&self, site_id: u64) -> Vec<UidMapping> {
        let entries = self.entries.read().unwrap();
        entries
            .iter()
            .filter(|e| e.site_id == site_id)
            .cloned()
            .collect()
    }

    /// Returns all mappings.
    ///
    /// # Returns
    /// Vector of all UID mappings
    pub fn all_mappings(&self) -> Vec<UidMapping> {
        let entries = self.entries.read().unwrap();
        entries.clone()
    }

    /// Returns the number of mappings.
    pub fn mapping_count(&self) -> usize {
        let entries = self.entries.read().unwrap();
        entries.len()
    }

    /// Maps a UID with explicit root passthrough.
    ///
    /// UID 0 always maps to 0, regardless of any mapping table entry.
    ///
    /// # Arguments
    /// * `site_id` - Source site identifier
    /// * `canonical_uid` - Canonical UID to map
    ///
    /// # Returns
    /// The local UID (0 if canonical_uid is 0)
    pub fn map_uid_root_passthrough(&self, site_id: u64, canonical_uid: u32) -> u32 {
        if canonical_uid == 0 {
            return 0;
        }

        let mappings = self.mappings.read().unwrap();
        *mappings
            .get(&(site_id, canonical_uid))
            .unwrap_or(&canonical_uid)
    }
}

impl Default for UidMapManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_mapping() {
        let mgr = UidMapManager::new();

        mgr.add_mapping(1, 1000, 2000, Some("user1".to_string()))
            .expect("add_mapping failed");

        assert_eq!(mgr.mapping_count(), 1);
    }

    #[test]
    fn test_map_uid() {
        let mgr = UidMapManager::new();

        mgr.add_mapping(1, 1000, 2000, None)
            .expect("add_mapping failed");

        let local = mgr.map_uid(1, 1000);
        assert_eq!(local, 2000);
    }

    #[test]
    fn test_map_uid_passthrough() {
        let mgr = UidMapManager::new();

        mgr.add_mapping(1, 1000, 2000, None)
            .expect("add_mapping failed");

        let local = mgr.map_uid(1, 999);
        assert_eq!(local, 999);
    }

    #[test]
    fn test_map_uid_root_always_passthrough() {
        let mgr = UidMapManager::new();

        mgr.add_mapping(1, 0, 65534, Some("root maps to nobody".to_string()))
            .expect("add_mapping failed");

        let local = mgr.map_uid(1, 0);
        assert_eq!(local, 0);
    }

    #[test]
    fn test_map_uid_different_sites() {
        let mgr = UidMapManager::new();

        mgr.add_mapping(1, 1000, 2000, None)
            .expect("add_mapping failed");
        mgr.add_mapping(2, 1000, 3000, None)
            .expect("add_mapping failed");

        assert_eq!(mgr.map_uid(1, 1000), 2000);
        assert_eq!(mgr.map_uid(2, 1000), 3000);
    }

    #[test]
    fn test_remove_mapping() {
        let mgr = UidMapManager::new();

        mgr.add_mapping(1, 1000, 2000, None)
            .expect("add_mapping failed");

        let removed = mgr.remove_mapping(1, 1000).expect("remove_mapping failed");
        assert!(removed);

        assert_eq!(mgr.mapping_count(), 0);
        assert_eq!(mgr.map_uid(1, 1000), 1000);
    }

    #[test]
    fn test_remove_mapping_not_found() {
        let mgr = UidMapManager::new();

        let result = mgr.remove_mapping(1, 1000).expect("remove_mapping failed");
        assert!(!result);
    }

    #[test]
    fn test_map_gid_passthrough() {
        let mgr = UidMapManager::new();

        mgr.add_mapping(1, 1000, 2000, None)
            .expect("add_mapping failed");

        let gid = mgr.map_gid(1, 500);
        assert_eq!(gid, 500);

        let gid2 = mgr.map_gid(2, 500);
        assert_eq!(gid2, 500);
    }

    #[test]
    fn test_mappings_for_site() {
        let mgr = UidMapManager::new();

        mgr.add_mapping(1, 1000, 2000, None)
            .expect("add_mapping failed");
        mgr.add_mapping(1, 1001, 2001, None)
            .expect("add_mapping failed");
        mgr.add_mapping(2, 1000, 3000, None)
            .expect("add_mapping failed");

        let site1_mappings = mgr.mappings_for_site(1);
        assert_eq!(site1_mappings.len(), 2);

        let site2_mappings = mgr.mappings_for_site(2);
        assert_eq!(site2_mappings.len(), 1);
    }

    #[test]
    fn test_all_mappings() {
        let mgr = UidMapManager::new();

        mgr.add_mapping(1, 1000, 2000, Some("desc1".to_string()))
            .expect("add_mapping failed");
        mgr.add_mapping(2, 1001, 2001, Some("desc2".to_string()))
            .expect("add_mapping failed");

        let all = mgr.all_mappings();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_map_uid_root_passthrough_explicit() {
        let mgr = UidMapManager::new();

        mgr.add_mapping(1, 0, 65534, None)
            .expect("add_mapping failed");

        let local = mgr.map_uid_root_passthrough(1, 0);
        assert_eq!(local, 0);

        let local2 = mgr.map_uid_root_passthrough(1, 1000);
        assert_eq!(local2, 1000);
    }
}
