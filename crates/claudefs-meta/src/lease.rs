//! Lease-based metadata caching protocol.
//!
//! This module provides lease-based metadata caching for FUSE clients.
//! Clients request leases on inodes for cached reads. Leases have a configurable
//! duration (default 30 seconds). The server tracks which clients hold leases
//! on which inodes. When metadata changes, the server revokes leases (returns
//! list of clients to notify). Expired leases are automatically cleaned up.
//!
//! This reduces metadata server load: clients with valid leases serve stat/readdir
//! from local cache.

use std::collections::{HashMap, HashSet};
use std::sync::RwLock;

use crate::types::*;

/// Type of metadata lease.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeaseType {
    /// Read-only lease: client can cache stat/readdir results.
    Read,
    /// Write lease: client has exclusive write access to metadata.
    Write,
}

/// A metadata lease granted to a client.
#[derive(Clone, Debug)]
pub struct Lease {
    /// The inode being leased.
    pub ino: InodeId,
    /// The client that holds the lease.
    pub client: NodeId,
    /// Type of lease.
    pub lease_type: LeaseType,
    /// When the lease was granted.
    pub granted_at: Timestamp,
    /// When the lease expires.
    pub expires_at: Timestamp,
    /// Unique lease ID.
    pub lease_id: u64,
}

/// Manages metadata leases for distributed caching.
pub struct LeaseManager {
    /// Default lease duration in seconds.
    lease_duration_secs: u64,
    /// Active leases by inode.
    inode_leases: RwLock<HashMap<InodeId, Vec<Lease>>>,
    /// Active leases by client.
    client_leases: RwLock<HashMap<NodeId, HashSet<u64>>>,
    /// Next lease ID.
    next_lease_id: RwLock<u64>,
}

impl LeaseManager {
    /// Create a new lease manager with the given lease duration.
    pub fn new(lease_duration_secs: u64) -> Self {
        Self {
            lease_duration_secs,
            inode_leases: RwLock::new(HashMap::new()),
            client_leases: RwLock::new(HashMap::new()),
            next_lease_id: RwLock::new(1),
        }
    }

    /// Grant a lease to a client for an inode.
    /// Read leases can coexist. Write leases are exclusive (fail if any lease exists).
    /// Returns the lease ID on success.
    pub fn grant(
        &self,
        ino: InodeId,
        client: NodeId,
        lease_type: LeaseType,
    ) -> Result<u64, MetaError> {
        let inode_leases = self.inode_leases.read().unwrap();

        if let Some(leases) = inode_leases.get(&ino) {
            if lease_type == LeaseType::Write {
                if !leases.is_empty() {
                    return Err(MetaError::PermissionDenied);
                }
            } else {
                for lease in leases {
                    if lease.lease_type == LeaseType::Write {
                        return Err(MetaError::PermissionDenied);
                    }
                }
            }
        }
        drop(inode_leases);

        let now = Timestamp::now();
        let expires_at = Timestamp {
            secs: now.secs + self.lease_duration_secs,
            nanos: now.nanos,
        };

        let lease_id = {
            let mut next = self.next_lease_id.write().unwrap();
            let id = *next;
            *next += 1;
            id
        };

        let lease = Lease {
            ino,
            client,
            lease_type,
            granted_at: now,
            expires_at,
            lease_id,
        };

        {
            let mut inode_leases = self.inode_leases.write().unwrap();
            inode_leases.entry(ino).or_default().push(lease);
        }

        {
            let mut client_leases = self.client_leases.write().unwrap();
            client_leases.entry(client).or_default().insert(lease_id);
        }

        Ok(lease_id)
    }

    /// Revoke all leases on an inode. Called when metadata changes.
    /// Returns the list of clients that need to be notified of revocation.
    pub fn revoke(&self, ino: InodeId) -> Vec<NodeId> {
        let clients: Vec<NodeId> = {
            let mut inode_leases = self.inode_leases.write().unwrap();
            if let Some(leases) = inode_leases.remove(&ino) {
                leases.into_iter().map(|l| l.client).collect()
            } else {
                Vec::new()
            }
        };

        for client in &clients {
            let mut client_leases = self.client_leases.write().unwrap();
            if let Some(lease_ids) = client_leases.get_mut(client) {
                let inode_lease_ids: Vec<u64> = {
                    let leases = self.inode_leases.read().unwrap();
                    if let Some(l) = leases.get(&ino) {
                        l.iter()
                            .filter(|l| l.client == *client)
                            .map(|l| l.lease_id)
                            .collect()
                    } else {
                        Vec::new()
                    }
                };
                for id in inode_lease_ids {
                    lease_ids.remove(&id);
                }
                if lease_ids.is_empty() {
                    client_leases.remove(client);
                }
            }
        }

        clients
    }

    /// Revoke a specific lease by ID.
    pub fn revoke_lease(&self, lease_id: u64) -> Result<(), MetaError> {
        let mut target_lease: Option<Lease> = None;

        {
            let inode_leases = self.inode_leases.read().unwrap();
            for leases in inode_leases.values() {
                for lease in leases {
                    if lease.lease_id == lease_id {
                        target_lease = Some(lease.clone());
                        break;
                    }
                }
                if target_lease.is_some() {
                    break;
                }
            }
        }

        let lease = target_lease.ok_or(MetaError::PermissionDenied)?;

        {
            let mut inode_leases = self.inode_leases.write().unwrap();
            if let Some(leases) = inode_leases.get_mut(&lease.ino) {
                leases.retain(|l| l.lease_id != lease_id);
                if leases.is_empty() {
                    inode_leases.remove(&lease.ino);
                }
            }
        }

        {
            let mut client_leases = self.client_leases.write().unwrap();
            if let Some(lease_ids) = client_leases.get_mut(&lease.client) {
                lease_ids.remove(&lease_id);
                if lease_ids.is_empty() {
                    client_leases.remove(&lease.client);
                }
            }
        }

        Ok(())
    }

    /// Revoke all leases held by a client (e.g., when client disconnects).
    pub fn revoke_client(&self, client: NodeId) -> usize {
        let lease_ids: HashSet<u64> = {
            let client_leases = self.client_leases.read().unwrap();
            client_leases.get(&client).cloned().unwrap_or_default()
        };

        let count = lease_ids.len();

        {
            let mut client_leases = self.client_leases.write().unwrap();
            client_leases.remove(&client);
        }

        let mut inodes_to_clean: Vec<InodeId> = Vec::new();
        {
            let mut inode_leases = self.inode_leases.write().unwrap();
            for (ino, leases) in inode_leases.iter_mut() {
                leases.retain(|l| l.client != client);
                if leases.is_empty() {
                    inodes_to_clean.push(*ino);
                }
            }
            for ino in &inodes_to_clean {
                inode_leases.remove(ino);
            }
        }

        count
    }

    /// Check if a client holds a valid (non-expired) lease on an inode.
    pub fn has_valid_lease(&self, ino: InodeId, client: NodeId) -> bool {
        let now = Timestamp::now();
        let inode_leases = self.inode_leases.read().unwrap();

        if let Some(leases) = inode_leases.get(&ino) {
            for lease in leases {
                if lease.client == client && lease.expires_at > now {
                    return true;
                }
            }
        }
        false
    }

    /// Renew a lease, extending its expiration.
    pub fn renew(&self, lease_id: u64) -> Result<(), MetaError> {
        let now = Timestamp::now();
        let expires_at = Timestamp {
            secs: now.secs + self.lease_duration_secs,
            nanos: now.nanos,
        };

        let mut inode_leases = self.inode_leases.write().unwrap();

        for leases in inode_leases.values_mut() {
            for lease in leases {
                if lease.lease_id == lease_id {
                    lease.expires_at = expires_at;
                    return Ok(());
                }
            }
        }

        Err(MetaError::PermissionDenied)
    }

    /// Clean up expired leases. Returns the number of leases removed.
    pub fn cleanup_expired(&self) -> usize {
        let now = Timestamp::now();
        let mut removed_count = 0;
        let mut clients_to_check: HashSet<NodeId> = HashSet::new();

        {
            let mut inode_leases = self.inode_leases.write().unwrap();

            for (_ino, leases) in inode_leases.iter_mut() {
                let before = leases.len();
                leases.retain(|l| l.expires_at > now);
                let after = leases.len();
                removed_count += before - after;

                for lease in leases.iter() {
                    clients_to_check.insert(lease.client);
                }
            }

            inode_leases.retain(|_, leases| !leases.is_empty());
        }

        let mut client_leases = self.client_leases.write().unwrap();

        for client in clients_to_check {
            if let Some(lease_ids) = client_leases.get_mut(&client) {
                let inode_leases = self.inode_leases.read().unwrap();
                let valid_ids: HashSet<u64> = inode_leases
                    .values()
                    .flat_map(|l| l.iter())
                    .filter(|l| l.client == client)
                    .map(|l| l.lease_id)
                    .collect();
                lease_ids.retain(|id| valid_ids.contains(id));
                if lease_ids.is_empty() {
                    client_leases.remove(&client);
                }
            }
        }

        removed_count
    }

    /// Get all active leases on an inode (including expired — caller should check).
    pub fn leases_on(&self, ino: InodeId) -> Vec<Lease> {
        let inode_leases = self.inode_leases.read().unwrap();
        inode_leases.get(&ino).cloned().unwrap_or_default()
    }

    /// Get the number of active leases.
    pub fn active_lease_count(&self) -> usize {
        let inode_leases = self.inode_leases.read().unwrap();
        inode_leases.values().map(|v| v.len()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grant_read_lease() {
        let mgr = LeaseManager::new(30);
        let ino = InodeId::new(1);
        let client = NodeId::new(100);

        let lease_id = mgr.grant(ino, client, LeaseType::Read).unwrap();
        assert!(lease_id > 0);
        assert!(mgr.has_valid_lease(ino, client));
    }

    #[test]
    fn test_grant_multiple_read_leases() {
        let mgr = LeaseManager::new(30);
        let ino = InodeId::new(1);

        let _ = mgr.grant(ino, NodeId::new(100), LeaseType::Read).unwrap();
        let _ = mgr.grant(ino, NodeId::new(101), LeaseType::Read).unwrap();
        let _ = mgr.grant(ino, NodeId::new(102), LeaseType::Read).unwrap();

        assert!(mgr.has_valid_lease(ino, NodeId::new(100)));
        assert!(mgr.has_valid_lease(ino, NodeId::new(101)));
        assert!(mgr.has_valid_lease(ino, NodeId::new(102)));
    }

    #[test]
    fn test_grant_write_lease_exclusive() {
        let mgr = LeaseManager::new(30);
        let ino = InodeId::new(1);

        let _ = mgr.grant(ino, NodeId::new(100), LeaseType::Write).unwrap();

        let result = mgr.grant(ino, NodeId::new(101), LeaseType::Read);
        assert!(result.is_err());

        let result = mgr.grant(ino, NodeId::new(101), LeaseType::Write);
        assert!(result.is_err());
    }

    #[test]
    fn test_write_lease_blocked_by_read() {
        let mgr = LeaseManager::new(30);
        let ino = InodeId::new(1);

        let _ = mgr.grant(ino, NodeId::new(100), LeaseType::Read).unwrap();

        let result = mgr.grant(ino, NodeId::new(101), LeaseType::Write);
        assert!(result.is_err());
    }

    #[test]
    fn test_revoke_inode() {
        let mgr = LeaseManager::new(30);
        let ino = InodeId::new(1);

        let _ = mgr.grant(ino, NodeId::new(100), LeaseType::Read).unwrap();
        let _ = mgr.grant(ino, NodeId::new(101), LeaseType::Read).unwrap();

        let clients = mgr.revoke(ino);
        assert_eq!(clients.len(), 2);
        assert!(clients.contains(&NodeId::new(100)));
        assert!(clients.contains(&NodeId::new(101)));
        assert!(!mgr.has_valid_lease(ino, NodeId::new(100)));
    }

    #[test]
    fn test_revoke_client() {
        let mgr = LeaseManager::new(30);
        let ino1 = InodeId::new(1);
        let ino2 = InodeId::new(2);

        let _ = mgr.grant(ino1, NodeId::new(100), LeaseType::Read).unwrap();
        let _ = mgr.grant(ino2, NodeId::new(100), LeaseType::Read).unwrap();

        let count = mgr.revoke_client(NodeId::new(100));
        assert_eq!(count, 2);
        assert!(!mgr.has_valid_lease(ino1, NodeId::new(100)));
        assert!(!mgr.has_valid_lease(ino2, NodeId::new(100)));
    }

    #[test]
    fn test_revoke_specific_lease() {
        let mgr = LeaseManager::new(30);
        let ino = InodeId::new(1);

        let lease_id = mgr.grant(ino, NodeId::new(100), LeaseType::Read).unwrap();

        mgr.revoke_lease(lease_id).unwrap();
        assert!(!mgr.has_valid_lease(ino, NodeId::new(100)));
    }

    #[test]
    fn test_renew_lease() {
        let mgr = LeaseManager::new(30);
        let ino = InodeId::new(1);
        let client = NodeId::new(100);

        let lease_id = mgr.grant(ino, client, LeaseType::Read).unwrap();

        let original_leases = mgr.leases_on(ino);
        let original_expires = original_leases[0].expires_at;

        mgr.renew(lease_id).unwrap();

        let updated_leases = mgr.leases_on(ino);
        assert!(updated_leases[0].expires_at > original_expires);
    }

    #[test]
    fn test_active_lease_count() {
        let mgr = LeaseManager::new(30);

        assert_eq!(mgr.active_lease_count(), 0);

        let _ = mgr
            .grant(InodeId::new(1), NodeId::new(100), LeaseType::Read)
            .unwrap();
        let _ = mgr
            .grant(InodeId::new(2), NodeId::new(101), LeaseType::Read)
            .unwrap();

        assert_eq!(mgr.active_lease_count(), 2);
    }

    #[test]
    fn test_leases_on_inode() {
        let mgr = LeaseManager::new(30);
        let ino = InodeId::new(1);

        let _ = mgr.grant(ino, NodeId::new(100), LeaseType::Read).unwrap();
        let _ = mgr.grant(ino, NodeId::new(101), LeaseType::Read).unwrap();

        let leases = mgr.leases_on(ino);
        assert_eq!(leases.len(), 2);
    }
}
