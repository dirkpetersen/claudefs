//! Quota integration module for ClaudeFS metadata service.
//!
//! This module integrates `quota.rs` (per-user/group quotas) and `space_accounting.rs`
//! (per-directory usage) to enforce hard quotas on writes.

use std::sync::Arc;

use crate::kvstore::KvStore;
use crate::quota::{QuotaLimit, QuotaManager, QuotaTarget, QuotaUsage};
use crate::space_accounting::{DirUsage, SpaceAccountingStore};
use crate::types::{InodeId, MetaError};

const SOFT_LIMIT_PERCENT: u64 = 90;

#[derive(Clone, Debug)]
pub struct QuotaCheckContext {
    pub tenant: QuotaTarget,
    pub bytes_delta: i64,
    pub inodes_delta: i64,
    pub parent_dir: InodeId,
    pub uid: u32,
    pub gid: u32,
}

impl QuotaCheckContext {
    pub fn new(
        tenant: QuotaTarget,
        bytes_delta: i64,
        inodes_delta: i64,
        parent_dir: InodeId,
    ) -> Self {
        let (uid, gid) = match &tenant {
            QuotaTarget::User(u) => (*u, 0),
            QuotaTarget::Group(g) => (0, *g),
        };
        Self {
            tenant,
            bytes_delta,
            inodes_delta,
            parent_dir,
            uid,
            gid,
        }
    }

    pub fn for_user(
        uid: u32,
        gid: u32,
        bytes_delta: i64,
        inodes_delta: i64,
        parent_dir: InodeId,
    ) -> Self {
        Self {
            tenant: QuotaTarget::User(uid),
            bytes_delta,
            inodes_delta,
            parent_dir,
            uid,
            gid,
        }
    }
}

#[derive(Debug, Clone)]
pub enum QuotaCheckResult {
    AllowedWithHeadroom {
        bytes_remaining: u64,
        inodes_remaining: u64,
    },
    AllowedWithWarning {
        bytes_remaining: u64,
    },
    Denied {
        reason: String,
    },
}

#[derive(Debug, Clone)]
pub struct SoftLimitWarning {
    pub target: QuotaTarget,
    pub bytes_headroom: u64,
    pub usage_percent: f64,
}

pub struct QuotaEnforcer {
    quota_store: Arc<QuotaManager>,
    space_acct: Arc<SpaceAccountingStore>,
    kv: Arc<dyn KvStore>,
}

impl QuotaEnforcer {
    pub fn new(
        quota_store: Arc<QuotaManager>,
        space_acct: Arc<SpaceAccountingStore>,
        kv: Arc<dyn KvStore>,
    ) -> Self {
        Self {
            quota_store,
            space_acct,
            kv,
        }
    }

    fn extract_uid_gid(ctx: &QuotaCheckContext) -> (u32, u32) {
        (ctx.uid, ctx.gid)
    }

    fn calculate_headroom(limit: &QuotaLimit, usage: &QuotaUsage) -> (u64, u64) {
        let bytes_remaining = if limit.has_byte_limit() {
            limit.max_bytes.saturating_sub(usage.bytes_used)
        } else {
            u64::MAX
        };
        let inodes_remaining = if limit.has_inode_limit() {
            limit.max_inodes.saturating_sub(usage.inodes_used)
        } else {
            u64::MAX
        };
        (bytes_remaining, inodes_remaining)
    }

    fn is_soft_limit_exceeded(limit: &QuotaLimit, usage: &QuotaUsage) -> bool {
        if !limit.has_byte_limit() {
            return false;
        }
        let soft_limit_bytes = limit.max_bytes * SOFT_LIMIT_PERCENT / 100;
        usage.bytes_used >= soft_limit_bytes
    }

    fn compute_remaining_after_delta(
        limit: &QuotaLimit,
        usage: &QuotaUsage,
        bytes_delta: i64,
        inodes_delta: i64,
    ) -> (u64, u64) {
        let would_be_bytes = if bytes_delta > 0 {
            usage.bytes_used.saturating_add(bytes_delta as u64)
        } else {
            usage.bytes_used.saturating_sub((-bytes_delta) as u64)
        };
        let would_be_inodes = if inodes_delta > 0 {
            usage.inodes_used.saturating_add(inodes_delta as u64)
        } else {
            usage.inodes_used.saturating_sub((-inodes_delta) as u64)
        };

        let bytes_remaining = if limit.has_byte_limit() {
            limit.max_bytes.saturating_sub(would_be_bytes)
        } else {
            u64::MAX
        };
        let inodes_remaining = if limit.has_inode_limit() {
            limit.max_inodes.saturating_sub(would_be_inodes)
        } else {
            u64::MAX
        };
        (bytes_remaining, inodes_remaining)
    }

    pub fn check_write_allowed(
        &self,
        ctx: &QuotaCheckContext,
    ) -> Result<QuotaCheckResult, MetaError> {
        let (uid, gid) = Self::extract_uid_gid(ctx);

        let user_target = QuotaTarget::User(uid);
        let group_target = QuotaTarget::Group(gid);

        let user_quota = self.quota_store.get_quota(&user_target);
        let group_quota = self.quota_store.get_quota(&group_target);

        if user_quota.is_none() && group_quota.is_none() {
            return Ok(QuotaCheckResult::AllowedWithHeadroom {
                bytes_remaining: u64::MAX,
                inodes_remaining: u64::MAX,
            });
        }

        let mut min_bytes_remaining = u64::MAX;
        let mut min_inodes_remaining = u64::MAX;
        let mut user_at_soft_limit = false;
        let mut group_at_soft_limit = false;

        if let Some(ref entry) = user_quota {
            let (bytes_remaining, inodes_remaining) = Self::compute_remaining_after_delta(
                &entry.limit,
                &entry.usage,
                ctx.bytes_delta,
                ctx.inodes_delta,
            );
            min_bytes_remaining = min_bytes_remaining.min(bytes_remaining);
            min_inodes_remaining = min_inodes_remaining.min(inodes_remaining);

            if entry.limit.has_byte_limit() && bytes_remaining == 0 {
                return Ok(QuotaCheckResult::Denied {
                    reason: format!("User {} exceeded byte quota", uid),
                });
            }
            if entry.limit.has_inode_limit() && inodes_remaining == 0 {
                return Ok(QuotaCheckResult::Denied {
                    reason: format!("User {} exceeded inode quota", uid),
                });
            }

            user_at_soft_limit = Self::is_soft_limit_exceeded(&entry.limit, &entry.usage);
        }

        if let Some(ref entry) = group_quota {
            let (bytes_remaining, inodes_remaining) = Self::compute_remaining_after_delta(
                &entry.limit,
                &entry.usage,
                ctx.bytes_delta,
                ctx.inodes_delta,
            );
            min_bytes_remaining = min_bytes_remaining.min(bytes_remaining);
            min_inodes_remaining = min_inodes_remaining.min(inodes_remaining);

            if entry.limit.has_byte_limit() && bytes_remaining == 0 {
                return Ok(QuotaCheckResult::Denied {
                    reason: format!("Group {} exceeded byte quota", gid),
                });
            }
            if entry.limit.has_inode_limit() && inodes_remaining == 0 {
                return Ok(QuotaCheckResult::Denied {
                    reason: format!("Group {} exceeded inode quota", gid),
                });
            }

            group_at_soft_limit = Self::is_soft_limit_exceeded(&entry.limit, &entry.usage);
        }

        if user_at_soft_limit || group_at_soft_limit {
            tracing::warn!(
                "Soft limit warning: uid={}, gid={}, bytes_remaining={}",
                uid,
                gid,
                min_bytes_remaining
            );
            Ok(QuotaCheckResult::AllowedWithWarning {
                bytes_remaining: min_bytes_remaining,
            })
        } else {
            Ok(QuotaCheckResult::AllowedWithHeadroom {
                bytes_remaining: min_bytes_remaining,
                inodes_remaining: min_inodes_remaining,
            })
        }
    }

    pub fn apply_write(&self, ctx: &QuotaCheckContext) -> Result<(), MetaError> {
        let (uid, gid) = Self::extract_uid_gid(ctx);

        self.quota_store
            .update_usage(uid, gid, ctx.bytes_delta, ctx.inodes_delta);

        if ctx.bytes_delta != 0 || ctx.inodes_delta != 0 {
            self.space_acct
                .add_delta(ctx.parent_dir, ctx.bytes_delta, ctx.inodes_delta)?;
        }

        Ok(())
    }

    pub fn check_soft_limit(
        &self,
        target: QuotaTarget,
    ) -> Result<Option<SoftLimitWarning>, MetaError> {
        let entry = match self.quota_store.get_quota(&target) {
            Some(e) => e,
            None => return Ok(None),
        };

        if !entry.limit.has_byte_limit() {
            return Ok(None);
        }

        let soft_limit_bytes = entry.limit.max_bytes * SOFT_LIMIT_PERCENT / 100;
        if entry.usage.bytes_used >= soft_limit_bytes {
            let bytes_headroom = entry.limit.max_bytes.saturating_sub(entry.usage.bytes_used);
            let usage_percent = if entry.limit.max_bytes > 0 {
                (entry.usage.bytes_used as f64 / entry.limit.max_bytes as f64) * 100.0
            } else {
                0.0
            };
            Ok(Some(SoftLimitWarning {
                target,
                bytes_headroom,
                usage_percent,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn compute_tenant_usage(&self, target: QuotaTarget) -> Result<QuotaUsage, MetaError> {
        match self.quota_store.get_usage(&target) {
            Some(usage) => Ok(usage),
            None => Ok(QuotaUsage::new()),
        }
    }

    pub fn get_dir_usage(&self, dir_ino: InodeId) -> Result<DirUsage, MetaError> {
        self.space_acct.get_usage(dir_ino)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kvstore::MemoryKvStore;
    use std::sync::Arc;

    fn setup() -> (QuotaEnforcer, Arc<MemoryKvStore>) {
        let kv = Arc::new(MemoryKvStore::new());
        let quota_store = Arc::new(QuotaManager::with_store(kv.clone()));
        let space_acct = Arc::new(SpaceAccountingStore::new(kv.clone()));
        let enforcer = QuotaEnforcer::new(quota_store, space_acct, kv.clone());
        (enforcer, kv)
    }

    #[test]
    fn test_check_allows_write_within_quota() {
        let (enforcer, _kv) = setup();

        enforcer
            .quota_store
            .set_quota(QuotaTarget::User(1000), QuotaLimit::new(1_000_000, 1000));

        let ctx = QuotaCheckContext::for_user(1000, 0, 1000, 1, InodeId::new(2));
        let result = enforcer.check_write_allowed(&ctx).unwrap();

        match result {
            QuotaCheckResult::AllowedWithHeadroom {
                bytes_remaining,
                inodes_remaining,
            } => {
                assert_eq!(bytes_remaining, 999_000);
                assert_eq!(inodes_remaining, 999);
            }
            _ => panic!("Expected AllowedWithHeadroom"),
        }
    }

    #[test]
    fn test_check_denies_write_over_hard_limit() {
        let (enforcer, _kv) = setup();

        enforcer
            .quota_store
            .set_quota(QuotaTarget::User(1000), QuotaLimit::new(1000, 100));
        enforcer.quota_store.update_usage(1000, 0, 900, 50);

        let ctx = QuotaCheckContext::for_user(1000, 0, 200, 0, InodeId::new(2));
        let result = enforcer.check_write_allowed(&ctx).unwrap();

        match result {
            QuotaCheckResult::Denied { reason } => {
                assert!(reason.contains("exceeded"));
            }
            _ => panic!("Expected Denied"),
        }
    }

    #[test]
    fn test_soft_limit_warning() {
        let (enforcer, _kv) = setup();

        enforcer
            .quota_store
            .set_quota(QuotaTarget::User(1000), QuotaLimit::new(1_000_000, 1000));
        enforcer.quota_store.update_usage(1000, 0, 910_000, 100);

        let ctx = QuotaCheckContext::for_user(1000, 0, 1000, 1, InodeId::new(2));
        let result = enforcer.check_write_allowed(&ctx).unwrap();

        match result {
            QuotaCheckResult::AllowedWithWarning { bytes_remaining } => {
                assert_eq!(bytes_remaining, 89_000);
            }
            _ => panic!("Expected AllowedWithWarning"),
        }
    }

    #[test]
    fn test_apply_write_updates_quota_and_accounting() {
        let (enforcer, _kv) = setup();

        enforcer
            .quota_store
            .set_quota(QuotaTarget::User(1000), QuotaLimit::new(1_000_000, 1000));

        let ctx = QuotaCheckContext::for_user(1000, 0, 1000, 1, InodeId::new(2));
        enforcer.apply_write(&ctx).unwrap();

        let usage = enforcer
            .quota_store
            .get_usage(&QuotaTarget::User(1000))
            .unwrap();
        assert_eq!(usage.bytes_used, 1000);
        assert_eq!(usage.inodes_used, 1);

        let dir_usage = enforcer.get_dir_usage(InodeId::new(2)).unwrap();
        assert_eq!(dir_usage.bytes, 1000);
        assert_eq!(dir_usage.inodes, 1);
    }

    #[test]
    fn test_group_quota_limits_user_write() {
        let (enforcer, _kv) = setup();

        enforcer
            .quota_store
            .set_quota(QuotaTarget::User(1000), QuotaLimit::new(1_000_000, 1000));
        enforcer
            .quota_store
            .set_quota(QuotaTarget::Group(500), QuotaLimit::new(500_000, 100));

        let ctx = QuotaCheckContext::for_user(1000, 500, 600_000, 10, InodeId::new(2));
        let result = enforcer.check_write_allowed(&ctx).unwrap();

        match result {
            QuotaCheckResult::Denied { reason } => {
                assert!(reason.contains("Group"));
            }
            _ => panic!("Expected Denied"),
        }
    }

    #[test]
    fn test_delete_refunds_quota() {
        let (enforcer, _kv) = setup();

        enforcer
            .quota_store
            .set_quota(QuotaTarget::User(1000), QuotaLimit::new(1_000_000, 1000));
        enforcer.quota_store.update_usage(1000, 0, 1000, 1);

        let ctx = QuotaCheckContext::for_user(1000, 0, -500, -1, InodeId::new(2));
        enforcer.apply_write(&ctx).unwrap();

        let usage = enforcer
            .quota_store
            .get_usage(&QuotaTarget::User(1000))
            .unwrap();
        assert_eq!(usage.bytes_used, 500);
        assert_eq!(usage.inodes_used, 0);
    }

    #[test]
    fn test_sequential_writes_accumulate_quota() {
        let (enforcer, _kv) = setup();

        enforcer
            .quota_store
            .set_quota(QuotaTarget::User(1000), QuotaLimit::new(1000, 10));

        let ctx1 = QuotaCheckContext::for_user(1000, 0, 500, 5, InodeId::new(2));
        enforcer.apply_write(&ctx1).unwrap();

        let ctx2 = QuotaCheckContext::for_user(1000, 0, 300, 3, InodeId::new(2));
        enforcer.apply_write(&ctx2).unwrap();

        let usage = enforcer
            .quota_store
            .get_usage(&QuotaTarget::User(1000))
            .unwrap();
        assert_eq!(usage.bytes_used, 800);
        assert_eq!(usage.inodes_used, 8);
    }

    #[test]
    fn test_compute_tenant_usage_matches_actual() {
        let (enforcer, _kv) = setup();

        enforcer
            .quota_store
            .set_quota(QuotaTarget::User(1000), QuotaLimit::new(1_000_000, 1000));
        enforcer.quota_store.update_usage(1000, 0, 500, 10);

        let usage = enforcer
            .compute_tenant_usage(QuotaTarget::User(1000))
            .unwrap();
        assert_eq!(usage.bytes_used, 500);
        assert_eq!(usage.inodes_used, 10);
    }

    #[test]
    fn test_no_quota_allows_unlimited() {
        let (enforcer, _kv) = setup();

        let ctx = QuotaCheckContext::for_user(1000, 0, 1_000_000, 1000, InodeId::new(2));
        let result = enforcer.check_write_allowed(&ctx).unwrap();

        match result {
            QuotaCheckResult::AllowedWithHeadroom {
                bytes_remaining,
                inodes_remaining,
            } => {
                assert_eq!(bytes_remaining, u64::MAX);
                assert_eq!(inodes_remaining, u64::MAX);
            }
            _ => panic!("Expected AllowedWithHeadroom"),
        }
    }

    #[test]
    fn test_inode_quota_enforcement() {
        let (enforcer, _kv) = setup();

        enforcer
            .quota_store
            .set_quota(QuotaTarget::User(1000), QuotaLimit::new(1_000_000, 10));
        enforcer.quota_store.update_usage(1000, 0, 0, 9);

        let ctx = QuotaCheckContext::for_user(1000, 0, 0, 2, InodeId::new(2));
        let result = enforcer.check_write_allowed(&ctx).unwrap();

        match result {
            QuotaCheckResult::Denied { reason } => {
                assert!(reason.contains("inode"));
            }
            _ => panic!("Expected Denied"),
        }
    }

    #[test]
    fn test_check_soft_limit_returns_warning() {
        let (enforcer, _kv) = setup();

        enforcer
            .quota_store
            .set_quota(QuotaTarget::User(1000), QuotaLimit::new(1_000_000, 1000));
        enforcer.quota_store.update_usage(1000, 0, 950_000, 100);

        let warning = enforcer.check_soft_limit(QuotaTarget::User(1000)).unwrap();

        assert!(warning.is_some());
        let w = warning.unwrap();
        assert_eq!(w.target, QuotaTarget::User(1000));
        assert!(w.usage_percent >= 90.0);
    }

    #[test]
    fn test_check_soft_limit_returns_none_below_limit() {
        let (enforcer, _kv) = setup();

        enforcer
            .quota_store
            .set_quota(QuotaTarget::User(1000), QuotaLimit::new(1_000_000, 1000));
        enforcer.quota_store.update_usage(1000, 0, 500_000, 100);

        let warning = enforcer.check_soft_limit(QuotaTarget::User(1000)).unwrap();

        assert!(warning.is_none());
    }

    #[test]
    fn test_get_dir_usage_returns_empty() {
        let (enforcer, _kv) = setup();

        let usage = enforcer.get_dir_usage(InodeId::new(999)).unwrap();

        assert_eq!(usage.bytes, 0);
        assert_eq!(usage.inodes, 0);
    }
}
