//! Cross-Tenant Deduplication Manager.
//!
//! Handles deduplication across tenant boundaries for shared data blocks.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};
use tracing::debug;

use crate::tenant_isolator::TenantId;
use crate::error::ReduceError;

/// Entry representing a cross-tenant dedup reference.
#[derive(Debug, Clone)]
pub struct CrossTenantDedupEntry {
    /// Unique block identifier
    pub block_id: u64,
    /// Original owner tenant
    pub owner_tenant_id: TenantId,
    /// Tenants referencing this block
    pub referring_tenants: HashSet<TenantId>,
    /// Total reference count
    pub refcount: u64,
}

/// Manager for cross-tenant deduplication tracking.
pub struct CrossTenantDedupManager {
    entries: Arc<RwLock<HashMap<u64, CrossTenantDedupEntry>>>,
}

impl CrossTenantDedupManager {
    /// Create a new CrossTenantDedupManager.
    pub fn new() -> Self {
        Self {
            entries: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a cross-tenant dedup match.
    pub async fn register_match(
        &self,
        block_id: u64,
        owner_tenant_id: TenantId,
        new_referrer: TenantId,
    ) -> Result<(), ReduceError> {
        let mut entries = self.entries.write().map_err(|e| ReduceError::InvalidInput(e.to_string()))?;
        
        let entry = entries.entry(block_id).or_insert(CrossTenantDedupEntry {
            block_id,
            owner_tenant_id,
            referring_tenants: HashSet::new(),
            refcount: 1,
        });
        
        entry.referring_tenants.insert(new_referrer);
        entry.refcount += 1;
        
        debug!("Registered cross-tenant dedup: block={}, owner={:?}, referrers={}", 
            block_id, owner_tenant_id, entry.referring_tenants.len());
        
        Ok(())
    }

    /// Get all shared blocks for a tenant.
    pub async fn get_shared_blocks(&self, tenant_id: TenantId) -> Result<Vec<u64>, ReduceError> {
        let entries = self.entries.read().map_err(|e| ReduceError::InvalidInput(e.to_string()))?;
        
        let shared: Vec<u64> = entries.iter()
            .filter(|(_, e)| e.referring_tenants.contains(&tenant_id) || e.owner_tenant_id == tenant_id)
            .map(|(block_id, _)| *block_id)
            .collect();
        
        Ok(shared)
    }

    /// Handle block eviction, apportioning credits fairly.
    pub async fn on_block_eviction(
        &self,
        block_id: u64,
    ) -> Result<HashMap<TenantId, u64>, ReduceError> {
        let mut entries = self.entries.write().map_err(|e| ReduceError::InvalidInput(e.to_string()))?;
        
        let Some(entry) = entries.remove(&block_id) else {
            return Ok(HashMap::new());
        };
        
        let total_refs = entry.refcount.max(1);
        let credit_per_tenant = 1;
        
        let mut apportionment = HashMap::new();
        for tenant in entry.referring_tenants {
            *apportionment.entry(tenant).or_insert(0) += credit_per_tenant;
        }
        
        debug!("Apportioned eviction credit for block {}: {:?}", block_id, apportionment);
        Ok(apportionment)
    }

    /// Get the refcount for a specific block.
    pub async fn get_refcount(&self, block_id: u64) -> Result<u64, ReduceError> {
        let entries = self.entries.read().map_err(|e| ReduceError::InvalidInput(e.to_string()))?;
        Ok(entries.get(&block_id).map(|e| e.refcount).unwrap_or(0))
    }

    /// Clear all entries for a tenant.
    pub async fn clear_tenant(&self, tenant_id: TenantId) -> Result<(), ReduceError> {
        let mut entries = self.entries.write().map_err(|e| ReduceError::InvalidInput(e.to_string()))?;
        
        entries.retain(|_, e| {
            e.owner_tenant_id != tenant_id && !e.referring_tenants.contains(&tenant_id)
        });
        
        Ok(())
    }
}

impl Default for CrossTenantDedupManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_register_match() {
        let manager = CrossTenantDedupManager::new();
        
        manager.register_match(42, TenantId(1), TenantId(2)).await.unwrap();
        
        let refcount = manager.get_refcount(42).await.unwrap();
        assert_eq!(refcount, 2);
    }

    #[tokio::test]
    async fn test_get_shared_blocks() {
        let manager = CrossTenantDedupManager::new();
        
        manager.register_match(1, TenantId(1), TenantId(2)).await.unwrap();
        manager.register_match(2, TenantId(1), TenantId(3)).await.unwrap();
        
        let shared = manager.get_shared_blocks(TenantId(2)).await.unwrap();
        assert!(shared.contains(&1));
    }

    #[tokio::test]
    async fn test_block_eviction() {
        let manager = CrossTenantDedupManager::new();
        
        manager.register_match(100, TenantId(1), TenantId(2)).await.unwrap();
        manager.register_match(100, TenantId(1), TenantId(3)).await.unwrap();
        
        let credits = manager.on_block_eviction(100).await.unwrap();
        assert!(credits.contains_key(&TenantId(2)));
        assert!(credits.contains_key(&TenantId(3)));
    }

    #[tokio::test]
    async fn test_clear_tenant() {
        let manager = CrossTenantDedupManager::new();
        
        manager.register_match(1, TenantId(1), TenantId(2)).await.unwrap();
        manager.register_match(2, TenantId(2), TenantId(3)).await.unwrap();
        
        manager.clear_tenant(TenantId(1)).await.unwrap();
        
        let shared1 = manager.get_shared_blocks(TenantId(2)).await.unwrap();
        assert!(shared1.is_empty());
        
        let shared2 = manager.get_shared_blocks(TenantId(3)).await.unwrap();
        assert!(shared2.contains(&2));
    }
}