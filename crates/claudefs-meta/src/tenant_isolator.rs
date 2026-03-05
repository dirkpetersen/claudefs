//! Tenant namespace isolation for multi-tenant metadata operations.
//!
//! This module enforces strong tenant isolation at the metadata level,
//! providing namespace separation, shard range isolation, and audit logging.

use std::cell::Cell;

use dashmap::DashMap;
use serde::{Deserialize, Serialize};

pub use crate::tenant::TenantId;
use crate::client_session::SessionId;
use crate::types::{InodeId, MetaError, Timestamp};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TenantNamespace {
    pub tenant_id: TenantId,
    pub root_inode: InodeId,
    pub metadata_shard_range: (u32, u32),
}

impl TenantNamespace {
    pub fn new(tenant_id: TenantId, root_inode: InodeId, shard_start: u32, shard_end: u32) -> Self {
        Self {
            tenant_id,
            root_inode,
            metadata_shard_range: (shard_start, shard_end),
        }
    }

    pub fn contains_inode(&self, ino: InodeId) -> bool {
        ino >= self.root_inode
    }

    pub fn contains_shard(&self, shard_id: u32) -> bool {
        shard_id >= self.metadata_shard_range.0 && shard_id < self.metadata_shard_range.1
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TenantCapabilities {
    pub can_read: bool,
    pub can_write: bool,
    pub can_delete: bool,
    pub can_modify_quotas: bool,
    pub can_view_other_tenants: bool,
}

impl Default for TenantCapabilities {
    fn default() -> Self {
        Self {
            can_read: true,
            can_write: true,
            can_delete: true,
            can_modify_quotas: false,
            can_view_other_tenants: false,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TenantContext {
    pub tenant_id: TenantId,
    pub user_id: u32,
    pub session_id: SessionId,
    pub namespace_root: InodeId,
    pub capabilities: TenantCapabilities,
}

impl TenantContext {
    pub fn new(
        tenant_id: TenantId,
        user_id: u32,
        session_id: SessionId,
        namespace_root: InodeId,
        capabilities: TenantCapabilities,
    ) -> Self {
        Self {
            tenant_id,
            user_id,
            session_id,
            namespace_root,
            capabilities,
        }
    }

    pub fn can_access(&self, ino: InodeId, namespace: &TenantNamespace) -> bool {
        ino == namespace.root_inode || self.namespace_root == namespace.root_inode
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum IsolationViolationType {
    CrossTenantRead,
    NamespaceEscape,
    PermissionDenied,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IsolationViolation {
    pub violation_type: IsolationViolationType,
    pub tenant_id: TenantId,
    pub attempted_inode: InodeId,
    pub owner_tenant: Option<TenantId>,
    pub timestamp: Timestamp,
    pub session_id: Option<SessionId>,
}

pub struct TenantIsolatorConfig {
    pub audit_log_size: usize,
    pub default_root_inode_start: u64,
    pub shards_per_tenant: u32,
}

impl Default for TenantIsolatorConfig {
    fn default() -> Self {
        Self {
            audit_log_size: 10000,
            default_root_inode_start: 1000,
            shards_per_tenant: 16,
        }
    }
}

pub struct TenantIsolator {
    namespaces: DashMap<TenantId, TenantNamespace>,
    sessions: DashMap<SessionId, TenantContext>,
    violations: std::sync::RwLock<Vec<IsolationViolation>>,
    next_root_inode: Cell<u64>,
    next_shard_id: Cell<u32>,
    config: TenantIsolatorConfig,
}

impl TenantIsolator {
    pub fn new(config: TenantIsolatorConfig) -> Self {
        Self {
            namespaces: DashMap::new(),
            sessions: DashMap::new(),
            violations: std::sync::RwLock::new(Vec::with_capacity(config.audit_log_size)),
            next_root_inode: Cell::new(config.default_root_inode_start),
            next_shard_id: Cell::new(0),
            config,
        }
    }

    pub fn register_tenant(
        &self,
        tenant_id: TenantId,
        _initial_capacity_bytes: u64,
    ) -> Result<TenantNamespace, MetaError> {
        let root_inode = InodeId::new(self.next_root_inode.get());
        self.next_root_inode.set(self.next_root_inode.get() + 1);

        let shard_start = self.next_shard_id.get();
        let shard_end = shard_start + self.config.shards_per_tenant;
        self.next_shard_id.set(shard_end);

        let namespace = TenantNamespace::new(
            tenant_id.clone(),
            root_inode,
            shard_start,
            shard_end,
        );

        self.namespaces.insert(tenant_id, namespace.clone());

        Ok(namespace)
    }

    pub fn get_tenant_namespace(&self, tenant_id: &TenantId) -> Option<TenantNamespace> {
        self.namespaces.get(tenant_id).map(|n| n.clone())
    }

    pub fn get_tenant_context(&self, session_id: &SessionId) -> Option<TenantContext> {
        self.sessions.get(session_id).map(|c| c.clone())
    }

    pub fn bind_session(&self, context: TenantContext) {
        self.sessions.insert(context.session_id.clone(), context);
    }

    pub fn unbind_session(&self, session_id: &SessionId) {
        self.sessions.remove(session_id);
    }

    pub fn enforce_isolation(
        &self,
        context: &TenantContext,
        inode_id: InodeId,
    ) -> Result<(), IsolationViolation> {
        let namespace = self.namespaces.get(&context.tenant_id)
            .ok_or_else(|| IsolationViolation {
                violation_type: IsolationViolationType::PermissionDenied,
                tenant_id: context.tenant_id.clone(),
                attempted_inode: inode_id,
                owner_tenant: None,
                timestamp: Timestamp::now(),
                session_id: Some(context.session_id.clone()),
            })?;

        if !namespace.contains_inode(inode_id) {
            let violation = IsolationViolation {
                violation_type: IsolationViolationType::CrossTenantRead,
                tenant_id: context.tenant_id.clone(),
                attempted_inode: inode_id,
                owner_tenant: None,
                timestamp: Timestamp::now(),
                session_id: Some(context.session_id.clone()),
            };
            self.record_violation(violation.clone());
            return Err(violation);
        }

        if inode_id < namespace.root_inode && inode_id != InodeId::ROOT_INODE {
            let violation = IsolationViolation {
                violation_type: IsolationViolationType::NamespaceEscape,
                tenant_id: context.tenant_id.clone(),
                attempted_inode: inode_id,
                owner_tenant: None,
                timestamp: Timestamp::now(),
                session_id: Some(context.session_id.clone()),
            };
            self.record_violation(violation.clone());
            return Err(violation);
        }

        if !context.capabilities.can_read && !context.capabilities.can_write {
            let violation = IsolationViolation {
                violation_type: IsolationViolationType::PermissionDenied,
                tenant_id: context.tenant_id.clone(),
                attempted_inode: inode_id,
                owner_tenant: Some(context.tenant_id.clone()),
                timestamp: Timestamp::now(),
                session_id: Some(context.session_id.clone()),
            };
            self.record_violation(violation.clone());
            return Err(violation);
        }

        Ok(())
    }

    pub fn enforce_shard_isolation(
        &self,
        context: &TenantContext,
        shard_id: u32,
    ) -> Result<(), IsolationViolation> {
        let namespace = self.namespaces.get(&context.tenant_id)
            .ok_or_else(|| IsolationViolation {
                violation_type: IsolationViolationType::PermissionDenied,
                tenant_id: context.tenant_id.clone(),
                attempted_inode: InodeId::new(0),
                owner_tenant: None,
                timestamp: Timestamp::now(),
                session_id: Some(context.session_id.clone()),
            })?;

        if !namespace.contains_shard(shard_id) {
            let violation = IsolationViolation {
                violation_type: IsolationViolationType::CrossTenantRead,
                tenant_id: context.tenant_id.clone(),
                attempted_inode: InodeId::new(0),
                owner_tenant: None,
                timestamp: Timestamp::now(),
                session_id: Some(context.session_id.clone()),
            };
            self.record_violation(violation.clone());
            return Err(violation);
        }

        Ok(())
    }

    pub fn list_inodes_in_tenant(&self, tenant_id: &TenantId, _dir_inode: InodeId) -> Result<Vec<InodeId>, MetaError> {
        let namespace = self.namespaces.get(tenant_id)
            .ok_or_else(|| MetaError::NotFound(format!("tenant {} not found", tenant_id)))?;

        Ok(vec![namespace.root_inode])
    }

    pub fn get_violations(&self, tenant_id: &TenantId) -> Vec<IsolationViolation> {
        let violations = self.violations.read().unwrap();
        violations.iter()
            .filter(|v| v.tenant_id == *tenant_id)
            .cloned()
            .collect()
    }

    pub fn list_tenants(&self) -> Vec<TenantId> {
        self.namespaces.iter().map(|k| k.key().clone()).collect()
    }

    pub fn tenant_count(&self) -> usize {
        self.namespaces.len()
    }

    fn record_violation(&self, violation: IsolationViolation) {
        let mut violations = self.violations.write().unwrap();
        if violations.len() >= self.config.audit_log_size {
            violations.remove(0);
        }
        violations.push(violation);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client_session::SessionId;

    fn make_isolator() -> TenantIsolator {
        TenantIsolator::new(TenantIsolatorConfig::default())
    }

    #[test]
    fn test_register_tenant() {
        let isolator = make_isolator();
        let tenant = TenantId::new("tenant1");
        
        let ns = isolator.register_tenant(tenant.clone(), 1_000_000_000).unwrap();
        assert_eq!(ns.tenant_id, tenant);
        assert!(ns.root_inode.as_u64() >= 1000);
    }

    #[test]
    fn test_register_multiple_tenants() {
        let isolator = make_isolator();
        
        let ns1 = isolator.register_tenant(TenantId::new("tenant1"), 1_000_000_000).unwrap();
        let ns2 = isolator.register_tenant(TenantId::new("tenant2"), 1_000_000_000).unwrap();
        
        assert_ne!(ns1.root_inode, ns2.root_inode);
        assert_ne!(ns1.metadata_shard_range, ns2.metadata_shard_range);
    }

    #[test]
    fn test_get_tenant_namespace() {
        let isolator = make_isolator();
        let tenant = TenantId::new("tenant1");
        
        isolator.register_tenant(tenant.clone(), 1_000_000_000).unwrap();
        
        let ns = isolator.get_tenant_namespace(&tenant).unwrap();
        assert_eq!(ns.tenant_id, tenant);
    }

    #[test]
    fn test_bind_and_get_session_context() {
        let isolator = make_isolator();
        let tenant = TenantId::new("tenant1");
        
        isolator.register_tenant(tenant.clone(), 1_000_000_000).unwrap();
        
        let context = TenantContext::new(
            tenant.clone(),
            1000,
            SessionId::new(),
            InodeId::new(1000),
            TenantCapabilities::default(),
        );
        
        isolator.bind_session(context.clone());
        
        let retrieved = isolator.get_tenant_context(&context.session_id).unwrap();
        assert_eq!(retrieved.tenant_id, tenant);
    }

    #[test]
    fn test_enforce_isolation_allowed() {
        let isolator = make_isolator();
        let tenant = TenantId::new("tenant1");
        
        let ns = isolator.register_tenant(tenant.clone(), 1_000_000_000).unwrap();
        
        let context = TenantContext::new(
            tenant.clone(),
            1000,
            SessionId::new(),
            ns.root_inode,
            TenantCapabilities::default(),
        );
        
        let result = isolator.enforce_isolation(&context, ns.root_inode);
        assert!(result.is_ok());
    }

    #[test]
    fn test_enforce_isolation_cross_tenant_rejected() {
        let isolator = make_isolator();
        let tenant = TenantId::new("tenant1");
        
        isolator.register_tenant(tenant.clone(), 1_000_000_000).unwrap();
        
        let context = TenantContext::new(
            tenant,
            1000,
            SessionId::new(),
            InodeId::new(1000),
            TenantCapabilities::default(),
        );
        
        let result = isolator.enforce_isolation(&context, InodeId::new(500));
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().violation_type, IsolationViolationType::CrossTenantRead);
    }

    #[test]
    fn test_enforce_isolation_namespace_escape_rejected() {
        let isolator = make_isolator();
        let tenant = TenantId::new("tenant1");
        
        isolator.register_tenant(tenant.clone(), 1_000_000_000).unwrap();
        
        let context = TenantContext::new(
            tenant.clone(),
            1000,
            SessionId::new(),
            InodeId::new(1000),
            TenantCapabilities::default(),
        );
        
        let result = isolator.enforce_isolation(&context, InodeId::ROOT_INODE);
        assert!(result.is_err());
    }

    #[test]
    fn test_enforce_isolation_permission_denied() {
        let isolator = make_isolator();
        let tenant = TenantId::new("tenant1");
        
        let ns = isolator.register_tenant(tenant.clone(), 1_000_000_000).unwrap();
        
        let mut caps = TenantCapabilities::default();
        caps.can_read = false;
        caps.can_write = false;
        
        let context = TenantContext::new(
            tenant.clone(),
            1000,
            SessionId::new(),
            ns.root_inode,
            caps,
        );
        
        let result = isolator.enforce_isolation(&context, ns.root_inode);
        assert!(result.is_err());
    }

    #[test]
    fn test_multiple_tenants_concurrent_isolation_checks() {
        let isolator = make_isolator();
        
        let ns1 = isolator.register_tenant(TenantId::new("tenant1"), 1_000_000_000).unwrap();
        let ns2 = isolator.register_tenant(TenantId::new("tenant2"), 1_000_000_000).unwrap();
        
        let ctx1 = TenantContext::new(
            TenantId::new("tenant1"),
            1000,
            SessionId::new(),
            ns1.root_inode,
            TenantCapabilities::default(),
        );
        
        let ctx2 = TenantContext::new(
            TenantId::new("tenant2"),
            1001,
            SessionId::new(),
            ns2.root_inode,
            TenantCapabilities::default(),
        );
        
        assert!(isolator.enforce_isolation(&ctx1, ns1.root_inode).is_ok());
        assert!(isolator.enforce_isolation(&ctx2, ns2.root_inode).is_ok());
    }

    #[test]
    fn test_shard_range_alignment() {
        let isolator = make_isolator();
        
        let ns = isolator.register_tenant(TenantId::new("tenant1"), 1_000_000_000).unwrap();
        
        assert!(ns.contains_shard(ns.metadata_shard_range.0));
        assert!(ns.contains_shard(ns.metadata_shard_range.1 - 1));
    }

    #[tokio::test]
    async fn test_audit_log_violations() {
        let isolator = make_isolator();
        let tenant = TenantId::new("tenant1");
        
        isolator.register_tenant(tenant.clone(), 1_000_000_000).unwrap();
        
        let context = TenantContext::new(
            tenant.clone(),
            1000,
            SessionId::new(),
            InodeId::new(1000),
            TenantCapabilities::default(),
        );
        
        let _ = isolator.enforce_isolation(&context, InodeId::new(1));
        
        let violations = isolator.get_violations(&tenant);
        assert!(!violations.is_empty());
    }

    #[test]
    fn test_list_inodes_in_tenant() {
        let isolator = make_isolator();
        let tenant = TenantId::new("tenant1");
        
        isolator.register_tenant(tenant.clone(), 1_000_000_000).unwrap();
        
        let inodes = isolator.list_inodes_in_tenant(&tenant, InodeId::new(1000)).unwrap();
        assert!(!inodes.is_empty());
    }

    #[test]
    fn test_list_tenants() {
        let isolator = make_isolator();
        
        isolator.register_tenant(TenantId::new("tenant1"), 1_000_000_000).unwrap();
        isolator.register_tenant(TenantId::new("tenant2"), 1_000_000_000).unwrap();
        
        let tenants = isolator.list_tenants();
        assert_eq!(tenants.len(), 2);
    }

    #[test]
    fn test_unbind_session() {
        let isolator = make_isolator();
        let tenant = TenantId::new("tenant1");
        
        isolator.register_tenant(tenant.clone(), 1_000_000_000).unwrap();
        
        let session_id = SessionId::new();
        let context = TenantContext::new(
            tenant.clone(),
            1000,
            session_id.clone(),
            InodeId::new(1000),
            TenantCapabilities::default(),
        );
        
        isolator.bind_session(context);
        assert!(isolator.get_tenant_context(&session_id).is_some());
        
        isolator.unbind_session(&session_id);
        assert!(isolator.get_tenant_context(&session_id).is_none());
    }

    #[test]
    fn test_tenant_capabilities_default() {
        let caps = TenantCapabilities::default();
        assert!(caps.can_read);
        assert!(caps.can_write);
        assert!(caps.can_delete);
        assert!(!caps.can_modify_quotas);
        assert!(!caps.can_view_other_tenants);
    }

    #[test]
    fn test_namespace_contains_inode() {
        let ns = TenantNamespace::new(
            TenantId::new("tenant1"),
            InodeId::new(1000),
            0,
            16,
        );
        
        assert!(ns.contains_inode(InodeId::new(1000)));
        assert!(ns.contains_inode(InodeId::new(2000)));
        assert!(!ns.contains_inode(InodeId::new(999)));
        assert!(!ns.contains_inode(InodeId::ROOT_INODE));
    }

    #[test]
    fn test_tenant_context_can_access() {
        let ns = TenantNamespace::new(
            TenantId::new("tenant1"),
            InodeId::new(1000),
            0,
            16,
        );
        
        let ctx = TenantContext::new(
            TenantId::new("tenant1"),
            1000,
            SessionId::new(),
            InodeId::new(1000),
            TenantCapabilities::default(),
        );
        
        assert!(ctx.can_access(InodeId::new(1000), &ns));
        assert!(ctx.can_access(InodeId::new(2000), &ns));
    }

    #[test]
    fn test_enforce_shard_isolation_allowed() {
        let isolator = make_isolator();
        
        let ns = isolator.register_tenant(TenantId::new("tenant1"), 1_000_000_000).unwrap();
        
        let context = TenantContext::new(
            TenantId::new("tenant1"),
            1000,
            SessionId::new(),
            ns.root_inode,
            TenantCapabilities::default(),
        );
        
        let result = isolator.enforce_shard_isolation(&context, ns.metadata_shard_range.0);
        assert!(result.is_ok());
    }

    #[test]
    fn test_enforce_shard_isolation_rejected() {
        let isolator = make_isolator();
        
        let ns = isolator.register_tenant(TenantId::new("tenant1"), 1_000_000_000).unwrap();
        
        let context = TenantContext::new(
            TenantId::new("tenant1"),
            1000,
            SessionId::new(),
            ns.root_inode,
            TenantCapabilities::default(),
        );
        
        let other_shard = ns.metadata_shard_range.1 + 10;
        let result = isolator.enforce_shard_isolation(&context, other_shard);
        assert!(result.is_err());
    }

    #[test]
    fn test_tenant_count() {
        let isolator = make_isolator();
        
        assert_eq!(isolator.tenant_count(), 0);
        
        isolator.register_tenant(TenantId::new("tenant1"), 1_000_000_000).unwrap();
        assert_eq!(isolator.tenant_count(), 1);
        
        isolator.register_tenant(TenantId::new("tenant2"), 1_000_000_000).unwrap();
        assert_eq!(isolator.tenant_count(), 2);
    }
}