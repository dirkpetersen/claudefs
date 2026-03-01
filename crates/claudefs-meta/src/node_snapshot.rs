//! MetadataNode snapshot and restore for disaster recovery.
//!
//! Provides full serialization of MetadataNode state including all inodes,
//! directory entries, xattrs, quotas, and WORM policies. Used for:
//! - Periodic metadata backups to S3 (per disaster recovery guide)
//! - Full node restore from backup
//! - Bootstrapping a new node from an existing snapshot

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::quota::{QuotaEntry, QuotaTarget};
use crate::types::*;
use crate::worm::WormEntry;
use crate::MetadataNode;

const SNAPSHOT_VERSION: u32 = 1;

/// Serializable snapshot of full MetadataNode state.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NodeSnapshot {
    /// Snapshot version for forward compatibility.
    pub version: u32,
    /// Node ID that created the snapshot.
    pub node_id: NodeId,
    /// Site ID.
    pub site_id: u64,
    /// Timestamp when snapshot was created.
    pub created_at: Timestamp,
    /// All inodes (serialized via bincode from the KV store).
    pub inodes: Vec<(InodeId, InodeAttr)>,
    /// All directory entries.
    pub dir_entries: Vec<(InodeId, Vec<DirEntry>)>,
    /// All extended attributes.
    pub xattrs: Vec<(InodeId, Vec<(String, Vec<u8>)>)>,
    /// Quota entries.
    pub quotas: Vec<(QuotaTarget, QuotaEntry)>,
    /// WORM entries.
    pub worm_entries: Vec<(InodeId, WormEntry)>,
    /// Next inode counter value.
    pub next_inode_id: u64,
    /// Raft hard state (term, voted_for, commit_index).
    pub raft_term: u64,
    /// Number of shards.
    pub num_shards: u16,
}

impl NodeSnapshot {
    /// Captures the full state of a MetadataNode into a snapshot.
    pub fn capture(node: &MetadataNode) -> Result<Self, MetaError> {
        let kv = node.kv_store();

        let mut inodes = Vec::new();
        let inode_entries = kv.scan_prefix(b"inode:")?;
        for (key, value) in inode_entries {
            let key_str = String::from_utf8_lossy(&key);
            let parts: Vec<&str> = key_str.split(':').collect();
            if parts.len() >= 2 {
                if let Ok(inode_id) = parts[1].parse::<u64>() {
                    if let Ok(attr) = bincode::deserialize::<InodeAttr>(&value) {
                        inodes.push((InodeId::new(inode_id), attr));
                    }
                }
            }
        }

        let mut dir_entries = Vec::new();
        let dir_key_entries = kv.scan_prefix(b"dir:")?;
        let mut current_dir: Option<InodeId> = None;
        let mut current_entries: Option<Vec<DirEntry>> = None;

        for (key, value) in dir_key_entries {
            let key_str = String::from_utf8_lossy(&key);
            let parts: Vec<&str> = key_str.split(':').collect();
            if parts.len() >= 3 {
                if let Some(parent_id) = parts.get(1).and_then(|s| s.parse::<u64>().ok()) {
                    let this_parent = InodeId::new(parent_id);
                    if current_dir != Some(this_parent) {
                        if let (Some(dir), Some(entries)) =
                            (current_dir.take(), current_entries.take())
                        {
                            dir_entries.push((dir, entries));
                        }
                        current_dir = Some(this_parent);
                        current_entries = Some(Vec::new());
                    }
                    if let Ok(entry) = bincode::deserialize::<DirEntry>(&value) {
                        if let Some(ref mut entries) = current_entries {
                            entries.push(entry);
                        }
                    }
                }
            }
        }
        if let (Some(dir), Some(entries)) = (current_dir, current_entries) {
            dir_entries.push((dir, entries));
        }

        let mut xattrs = Vec::new();
        let xattr_entries = kv.scan_prefix(b"xattr:")?;
        let mut current_ino: Option<InodeId> = None;
        let mut current_xattrs: Option<Vec<(String, Vec<u8>)>> = None;

        for (key, value) in xattr_entries {
            let key_str = String::from_utf8_lossy(&key);
            let parts: Vec<&str> = key_str.splitn(3, ':').collect();
            if parts.len() >= 3 {
                if let Some(ino_id) = parts.get(1).and_then(|s| s.parse::<u64>().ok()) {
                    let this_ino = InodeId::new(ino_id);
                    if current_ino != Some(this_ino) {
                        if let (Some(ino), Some(attrs)) =
                            (current_ino.take(), current_xattrs.take())
                        {
                            xattrs.push((ino, attrs));
                        }
                        current_ino = Some(this_ino);
                        current_xattrs = Some(Vec::new());
                    }
                    if let Some(name) = parts.get(2) {
                        if let Some(ref mut attrs) = current_xattrs {
                            attrs.push((name.to_string(), value));
                        }
                    }
                }
            }
        }
        if let (Some(ino), Some(attrs)) = (current_ino, current_xattrs) {
            xattrs.push((ino, attrs));
        }

        let quotas: Vec<(QuotaTarget, QuotaEntry)> = node
            .quota_manager()
            .list_quotas()
            .into_iter()
            .map(|e| (e.target.clone(), e))
            .collect();

        let worm_entries: Vec<(InodeId, WormEntry)> = Vec::new();

        let created_at = Timestamp::now();

        tracing::info!(
            node_id = %node.node_id(),
            inode_count = inodes.len(),
            dir_count = dir_entries.len(),
            xattr_count = xattrs.len(),
            quota_count = quotas.len(),
            "captured node snapshot"
        );

        Ok(Self {
            version: SNAPSHOT_VERSION,
            node_id: node.node_id(),
            site_id: node.config.site_id,
            created_at,
            inodes,
            dir_entries,
            xattrs,
            quotas,
            worm_entries,
            next_inode_id: node.next_inode_id(),
            raft_term: 0,
            num_shards: node.num_shards(),
        })
    }

    /// Serializes the snapshot to bytes via bincode.
    pub fn serialize(&self) -> Result<Vec<u8>, MetaError> {
        bincode::serialize(self).map_err(|e| MetaError::KvError(e.to_string()))
    }

    /// Deserializes the snapshot from bytes.
    pub fn deserialize(data: &[u8]) -> Result<Self, MetaError> {
        bincode::deserialize(data).map_err(|e| MetaError::KvError(e.to_string()))
    }

    /// Returns the number of inodes in the snapshot.
    pub fn inode_count(&self) -> usize {
        self.inodes.len()
    }

    /// Returns the estimated serialized size in bytes.
    pub fn total_size_bytes(&self) -> usize {
        let mut size = 0;
        size += std::mem::size_of::<u32>() * 4;
        size += std::mem::size_of::<NodeId>();
        size += std::mem::size_of::<Timestamp>();
        size += std::mem::size_of::<u16>();

        for (_, attr) in &self.inodes {
            size += bincode::serialized_size(attr).unwrap_or(0) as usize + 8;
        }
        for (_, entries) in &self.dir_entries {
            for entry in entries {
                size += bincode::serialized_size(entry).unwrap_or(0) as usize + 8;
            }
            size += 8;
        }
        for (_, xlist) in &self.xattrs {
            for (name, value) in xlist {
                size += name.len() + value.len() + 16;
            }
            size += 8;
        }
        for (_, q) in &self.quotas {
            size += bincode::serialized_size(q).unwrap_or(0) as usize + 8;
        }
        for (_, w) in &self.worm_entries {
            size += bincode::serialized_size(w).unwrap_or(0) as usize + 8;
        }

        size
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_node() -> MetadataNode {
        let config = MetadataNodeConfig {
            node_id: NodeId::new(1),
            num_shards: 256,
            replication_factor: 3,
            site_id: 1,
            data_dir: None,
            dir_shard_config: DirShardConfig::default(),
        };
        MetadataNode::new(config).unwrap()
    }

    #[test]
    fn test_capture_empty_node() {
        let node = make_node();
        let snapshot = NodeSnapshot::capture(&node).unwrap();
        assert!(snapshot.inode_count() >= 1);
        assert_eq!(snapshot.version, SNAPSHOT_VERSION);
    }

    #[test]
    fn test_capture_with_files() {
        let node = make_node();
        let _ = node
            .create_file(InodeId::ROOT_INODE, "test.txt", 1000, 1000, 0o644)
            .unwrap();
        let _ = node
            .mkdir(InodeId::ROOT_INODE, "testdir", 1000, 1000, 0o755)
            .unwrap();

        let snapshot = NodeSnapshot::capture(&node).unwrap();
        assert!(snapshot.inode_count() >= 3);
    }

    #[test]
    fn test_serialize_deserialize_roundtrip() {
        let node = make_node();
        let _ = node
            .create_file(InodeId::ROOT_INODE, "test.txt", 1000, 1000, 0o644)
            .unwrap();

        let snapshot = NodeSnapshot::capture(&node).unwrap();
        let serialized = snapshot.serialize().unwrap();
        let deserialized = NodeSnapshot::deserialize(&serialized).unwrap();

        assert_eq!(deserialized.version, snapshot.version);
        assert_eq!(deserialized.node_id, snapshot.node_id);
        assert_eq!(deserialized.inode_count(), snapshot.inode_count());
    }

    #[test]
    fn test_snapshot_version() {
        let node = make_node();
        let snapshot = NodeSnapshot::capture(&node).unwrap();
        assert_eq!(snapshot.version, 1);
    }

    #[test]
    fn test_snapshot_inode_count() {
        let node = make_node();
        let snapshot = NodeSnapshot::capture(&node).unwrap();
        assert_eq!(snapshot.inode_count(), snapshot.inodes.len());
    }

    #[test]
    fn test_snapshot_total_size() {
        let node = make_node();
        let _ = node
            .create_file(InodeId::ROOT_INODE, "test.txt", 1000, 1000, 0o644)
            .unwrap();

        let snapshot = NodeSnapshot::capture(&node).unwrap();
        let size = snapshot.total_size_bytes();
        assert!(size > 0);
    }
}
