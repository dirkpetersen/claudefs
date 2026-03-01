//! Multi-tenant namespace isolation for ClaudeFS metadata.
//!
//! Provides tenant-aware inode allocation, access control, and resource isolation.
//! Each tenant gets a dedicated namespace (subtree) with its own quota and QoS policies.
//! This is a Priority 1 feature gap per docs/agents.md.

use std::collections::HashMap;
use std::fmt;
use std::sync::RwLock;

use serde::{Deserialize, Serialize};

use crate::types::*;

/// Unique identifier for a tenant.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TenantId(String);

impl TenantId {
    /// Creates a new TenantId from a string.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Returns the tenant ID as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TenantId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Tenant configuration including resource limits and isolation boundaries.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TenantConfig {
    /// Unique tenant identifier.
    pub tenant_id: TenantId,
    /// Root directory inode for this tenant's namespace.
    pub root_inode: InodeId,
    /// Maximum number of inodes the tenant can create.
    pub max_inodes: u64,
    /// Maximum storage in bytes.
    pub max_bytes: u64,
    /// Allowed UIDs for this tenant.
    pub allowed_uids: Vec<u32>,
    /// Allowed GIDs for this tenant.
    pub allowed_gids: Vec<u32>,
    /// Whether the tenant is active (can create new files).
    pub active: bool,
    /// Creation timestamp.
    pub created_at: Timestamp,
}

impl TenantConfig {
    /// Creates a new tenant configuration.
    pub fn new(
        tenant_id: TenantId,
        root_inode: InodeId,
        max_inodes: u64,
        max_bytes: u64,
        allowed_uids: Vec<u32>,
        allowed_gids: Vec<u32>,
    ) -> Self {
        Self {
            tenant_id,
            root_inode,
            max_inodes,
            max_bytes,
            allowed_uids,
            allowed_gids,
            active: true,
            created_at: Timestamp::now(),
        }
    }
}

/// Per-tenant resource usage tracking.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TenantUsage {
    /// Current inode count.
    pub inode_count: u64,
    /// Current bytes used.
    pub bytes_used: u64,
    /// Total operations performed.
    pub total_ops: u64,
}

impl TenantUsage {
    /// Creates a new empty tenant usage.
    pub fn new() -> Self {
        Self::default()
    }

    /// Updates usage with delta values.
    pub fn update(&mut self, inode_delta: i64, bytes_delta: i64) {
        if inode_delta >= 0 {
            self.inode_count = self.inode_count.saturating_add(inode_delta as u64);
        } else {
            self.inode_count = self.inode_count.saturating_sub((-inode_delta) as u64);
        }
        if bytes_delta >= 0 {
            self.bytes_used = self.bytes_used.saturating_add(bytes_delta as u64);
        } else {
            self.bytes_used = self.bytes_used.saturating_sub((-bytes_delta) as u64);
        }
        self.total_ops = self.total_ops.saturating_add(1);
    }
}

/// Multi-tenant manager for namespace isolation.
pub struct TenantManager {
    tenants: RwLock<HashMap<TenantId, TenantConfig>>,
    usage: RwLock<HashMap<TenantId, TenantUsage>>,
    inode_to_tenant: RwLock<HashMap<InodeId, TenantId>>,
}

impl TenantManager {
    /// Creates a new tenant manager with no tenants.
    pub fn new() -> Self {
        Self {
            tenants: RwLock::new(HashMap::new()),
            usage: RwLock::new(HashMap::new()),
            inode_to_tenant: RwLock::new(HashMap::new()),
        }
    }

    /// Registers a new tenant.
    ///
    /// # Errors
    /// Returns an error if the tenant already exists.
    pub fn create_tenant(&self, config: TenantConfig) -> Result<(), MetaError> {
        let mut tenants = self
            .tenants
            .write()
            .map_err(|e| MetaError::KvError(format!("lock poisoned: {}", e)))?;

        if tenants.contains_key(&config.tenant_id) {
            return Err(MetaError::EntryExists {
                parent: config.root_inode,
                name: config.tenant_id.to_string(),
            });
        }

        let tenant_id = config.tenant_id.clone();
        tenants.insert(tenant_id.clone(), config);

        let mut usage = self
            .usage
            .write()
            .map_err(|e| MetaError::KvError(format!("lock poisoned: {}", e)))?;
        usage.insert(tenant_id, TenantUsage::default());

        tracing::info!("created tenant: {}", config.tenant_id);
        Ok(())
    }

    /// Deactivates and removes a tenant.
    ///
    /// # Errors
    /// Returns an error if the tenant is not found.
    pub fn remove_tenant(&self, tenant_id: &TenantId) -> Result<TenantConfig, MetaError> {
        let mut tenants = self
            .tenants
            .write()
            .map_err(|e| MetaError::KvError(format!("lock poisoned: {}", e)))?;

        let config = tenants
            .remove(tenant_id)
            .ok_or_else(|| MetaError::InodeNotFound(InodeId::new(0)))?;

        let mut usage = self
            .usage
            .write()
            .map_err(|e| MetaError::KvError(format!("lock poisoned: {}", e)))?;
        usage.remove(tenant_id);

        tracing::info!("removed tenant: {}", tenant_id);
        Ok(config)
    }

    /// Gets the tenant configuration.
    pub fn get_tenant(&self, tenant_id: &TenantId) -> Option<TenantConfig> {
        let tenants = self.tenants.read().ok()?;
        tenants.get(tenant_id).cloned()
    }

    /// Looks up which tenant owns an inode.
    pub fn tenant_for_inode(&self, ino: InodeId) -> Option<TenantId> {
        let inode_to_tenant = self.inode_to_tenant.read().ok()?;
        inode_to_tenant.get(&ino).cloned()
    }

    /// Assigns an inode to a tenant, checking quota limits.
    ///
    /// # Errors
    /// Returns an error if the tenant is not found, inactive, or over quota.
    pub fn assign_inode(&self, tenant_id: &TenantId, ino: InodeId) -> Result<(), MetaError> {
        let config = {
            let tenants = self
                .tenants
                .read()
                .map_err(|e| MetaError::KvError(format!("lock poisoned: {}", e)))?;
            tenants.get(tenant_id).cloned()
        };

        let config = config.ok_or_else(|| MetaError::InodeNotFound(InodeId::new(0)))?;

        if !config.active {
            return Err(MetaError::PermissionDenied);
        }

        {
            let usage = self
                .usage
                .read()
                .map_err(|e| MetaError::KvError(format!("lock poisoned: {}", e)))?;
            let tenant_usage = usage
                .get(tenant_id)
                .ok_or_else(|| MetaError::InodeNotFound(InodeId::new(0)))?;

            if config.max_inodes != u64::MAX && tenant_usage.inode_count >= config.max_inodes {
                return Err(MetaError::NoSpace);
            }
        }

        let mut inode_to_tenant = self
            .inode_to_tenant
            .write()
            .map_err(|e| MetaError::KvError(format!("lock poisoned: {}", e)))?;
        inode_to_tenant.insert(ino, tenant_id.clone());

        Ok(())
    }

    /// Releases an inode from tenant tracking.
    pub fn release_inode(&self, ino: InodeId) {
        let mut inode_to_tenant = match self.inode_to_tenant.write() {
            Ok(guard) => guard,
            Err(e) => {
                tracing::warn!("failed to acquire inode_to_tenant lock: {}", e);
                return;
            }
        };
        inode_to_tenant.remove(&ino);
    }

    /// Checks if the tenant has room for additional resources.
    pub fn check_tenant_quota(
        &self,
        tenant_id: &TenantId,
        additional_inodes: u64,
        additional_bytes: u64,
    ) -> bool {
        let config = match self.tenants.read() {
            Ok(tenants) => tenants.get(tenant_id).cloned(),
            Err(_) => return false,
        };

        let Some(config) = config else { return false };

        if !config.active {
            return false;
        }

        let usage = match self.usage.read() {
            Ok(usage) => usage.get(tenant_id).cloned(),
            Err(_) => return false,
        };

        let Some(usage) = usage else { return false };

        if config.max_inodes != u64::MAX {
            if usage.inode_count + additional_inodes > config.max_inodes {
                return false;
            }
        }

        if config.max_bytes != u64::MAX {
            if usage.bytes_used + additional_bytes > config.max_bytes {
                return false;
            }
        }

        true
    }

    /// Updates usage counters for a tenant.
    pub fn update_usage(&self, tenant_id: &TenantId, inode_delta: i64, bytes_delta: i64) {
        let mut usage = match self.usage.write() {
            Ok(guard) => guard,
            Err(e) => {
                tracing::warn!("failed to acquire usage lock: {}", e);
                return;
            }
        };

        let tenant_usage = usage
            .entry(tenant_id.clone())
            .or_insert_with(TenantUsage::new);
        tenant_usage.update(inode_delta, bytes_delta);
    }

    /// Gets the current usage for a tenant.
    pub fn get_usage(&self, tenant_id: &TenantId) -> Option<TenantUsage> {
        let usage = self.usage.read().ok()?;
        usage.get(tenant_id).cloned()
    }

    /// Checks if a uid/gid is authorized for this tenant.
    pub fn is_authorized(&self, tenant_id: &TenantId, uid: u32, gid: u32) -> bool {
        let tenants = match self.tenants.read() {
            Ok(guard) => guard,
            Err(_) => return false,
        };

        let Some(config) = tenants.get(tenant_id) else {
            return false;
        };

        if !config.active {
            return false;
        }

        if config.allowed_uids.is_empty() && config.allowed_gids.is_empty() {
            return true;
        }

        config.allowed_uids.contains(&uid) || config.allowed_gids.contains(&gid)
    }

    /// Lists all tenant IDs.
    pub fn list_tenants(&self) -> Vec<TenantId> {
        let tenants = match self.tenants.read() {
            Ok(guard) => guard,
            Err(_) => return Vec::new(),
        };
        tenants.keys().cloned().collect()
    }

    /// Returns the number of tenants.
    pub fn tenant_count(&self) -> usize {
        self.tenants.read().map(|t| t.len()).unwrap_or(0)
    }
}

impl Default for TenantManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tenant_manager() -> TenantManager {
        TenantManager::new()
    }

    #[test]
    fn test_create_tenant() {
        let mgr = make_tenant_manager();
        let config = TenantConfig::new(
            TenantId::new("tenant1"),
            InodeId::new(100),
            1000,
            1_000_000,
            vec![1000, 1001],
            vec![500, 501],
        );

        mgr.create_tenant(config.clone()).unwrap();
        let retrieved = mgr.get_tenant(&TenantId::new("tenant1")).unwrap();
        assert_eq!(retrieved.tenant_id.as_str(), "tenant1");
    }

    #[test]
    fn test_create_duplicate_tenant() {
        let mgr = make_tenant_manager();
        let config = TenantConfig::new(
            TenantId::new("tenant1"),
            InodeId::new(100),
            1000,
            1_000_000,
            vec![],
            vec![],
        );

        mgr.create_tenant(config).unwrap();
        let config2 = TenantConfig::new(
            TenantId::new("tenant1"),
            InodeId::new(200),
            2000,
            2_000_000,
            vec![],
            vec![],
        );
        let result = mgr.create_tenant(config2);
        assert!(result.is_err());
    }

    #[test]
    fn test_remove_tenant() {
        let mgr = make_tenant_manager();
        let config = TenantConfig::new(
            TenantId::new("tenant1"),
            InodeId::new(100),
            1000,
            1_000_000,
            vec![],
            vec![],
        );

        mgr.create_tenant(config).unwrap();
        mgr.remove_tenant(&TenantId::new("tenant1")).unwrap();
        assert!(mgr.get_tenant(&TenantId::new("tenant1")).is_none());
    }

    #[test]
    fn test_assign_and_lookup_inode() {
        let mgr = make_tenant_manager();
        let config = TenantConfig::new(
            TenantId::new("tenant1"),
            InodeId::new(100),
            1000,
            1_000_000,
            vec![],
            vec![],
        );

        mgr.create_tenant(config).unwrap();
        mgr.assign_inode(&TenantId::new("tenant1"), InodeId::new(200))
            .unwrap();

        let tenant = mgr.tenant_for_inode(InodeId::new(200)).unwrap();
        assert_eq!(tenant.as_str(), "tenant1");
    }

    #[test]
    fn test_tenant_quota_check() {
        let mgr = make_tenant_manager();
        let config = TenantConfig::new(
            TenantId::new("tenant1"),
            InodeId::new(100),
            100,
            1_000_000,
            vec![],
            vec![],
        );

        mgr.create_tenant(config).unwrap();

        assert!(mgr.check_tenant_quota(&TenantId::new("tenant1"), 50, 500_000));
        assert!(mgr.check_tenant_quota(&TenantId::new("tenant1"), 150, 500_000));
    }

    #[test]
    fn test_tenant_quota_exceeded() {
        let mgr = make_tenant_manager();
        let config = TenantConfig::new(
            TenantId::new("tenant1"),
            InodeId::new(100),
            10,
            1_000_000,
            vec![],
            vec![],
        );

        mgr.create_tenant(config).unwrap();

        mgr.update_usage(&TenantId::new("tenant1"), 5, 0);

        assert!(!mgr.check_tenant_quota(&TenantId::new("tenant1"), 10, 0));
    }

    #[test]
    fn test_update_usage() {
        let mgr = make_tenant_manager();
        let config = TenantConfig::new(
            TenantId::new("tenant1"),
            InodeId::new(100),
            1000,
            1_000_000,
            vec![],
            vec![],
        );

        mgr.create_tenant(config).unwrap();
        mgr.update_usage(&TenantId::new("tenant1"), 10, 1000);

        let usage = mgr.get_usage(&TenantId::new("tenant1")).unwrap();
        assert_eq!(usage.inode_count, 10);
        assert_eq!(usage.bytes_used, 1000);
    }

    #[test]
    fn test_authorization_check() {
        let mgr = make_tenant_manager();
        let config = TenantConfig::new(
            TenantId::new("tenant1"),
            InodeId::new(100),
            1000,
            1_000_000,
            vec![1000, 1001],
            vec![500],
        );

        mgr.create_tenant(config).unwrap();

        assert!(mgr.is_authorized(&TenantId::new("tenant1"), 1000, 600));
        assert!(mgr.is_authorized(&TenantId::new("tenant1"), 2000, 500));
        assert!(mgr.is_authorized(&TenantId::new("tenant1"), 2000, 600));
    }

    #[test]
    fn test_unauthorized_user() {
        let mgr = make_tenant_manager();
        let config = TenantConfig::new(
            TenantId::new("tenant1"),
            InodeId::new(100),
            1000,
            1_000_000,
            vec![1000],
            vec![500],
        );

        mgr.create_tenant(config).unwrap();

        assert!(!mgr.is_authorized(&TenantId::new("tenant1"), 2000, 600));
    }

    #[test]
    fn test_inactive_tenant() {
        let mgr = make_tenant_manager();
        let mut config = TenantConfig::new(
            TenantId::new("tenant1"),
            InodeId::new(100),
            1000,
            1_000_000,
            vec![],
            vec![],
        );
        config.active = false;

        mgr.create_tenant(config).unwrap();

        let result = mgr.assign_inode(&TenantId::new("tenant1"), InodeId::new(200));
        assert!(result.is_err());
    }

    #[test]
    fn test_list_tenants() {
        let mgr = make_tenant_manager();
        let config1 = TenantConfig::new(
            TenantId::new("tenant1"),
            InodeId::new(100),
            1000,
            1_000_000,
            vec![],
            vec![],
        );
        let config2 = TenantConfig::new(
            TenantId::new("tenant2"),
            InodeId::new(200),
            2000,
            2_000_000,
            vec![],
            vec![],
        );

        mgr.create_tenant(config1).unwrap();
        mgr.create_tenant(config2).unwrap();

        let tenants = mgr.list_tenants();
        assert_eq!(tenants.len(), 2);
    }

    #[test]
    fn test_release_inode() {
        let mgr = make_tenant_manager();
        let config = TenantConfig::new(
            TenantId::new("tenant1"),
            InodeId::new(100),
            1000,
            1_000_000,
            vec![],
            vec![],
        );

        mgr.create_tenant(config).unwrap();
        mgr.assign_inode(&TenantId::new("tenant1"), InodeId::new(200))
            .unwrap();
        mgr.release_inode(InodeId::new(200));

        assert!(mgr.tenant_for_inode(InodeId::new(200)).is_none());
    }
}
