use crate::error::{FuseError, Result};
use std::collections::HashMap;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct QuotaUsage {
    pub bytes_used: u64,
    pub bytes_soft: u64,
    pub bytes_hard: u64,
    pub inodes_used: u64,
    pub inodes_soft: u64,
    pub inodes_hard: u64,
}

impl QuotaUsage {
    pub fn new(bytes_soft: u64, bytes_hard: u64) -> Self {
        QuotaUsage {
            bytes_used: 0,
            bytes_soft,
            bytes_hard,
            inodes_used: 0,
            inodes_soft: 0,
            inodes_hard: 0,
        }
    }

    pub fn unlimited() -> Self {
        QuotaUsage {
            bytes_used: 0,
            bytes_soft: 0,
            bytes_hard: 0,
            inodes_used: 0,
            inodes_soft: 0,
            inodes_hard: 0,
        }
    }

    pub fn bytes_status(&self) -> QuotaStatus {
        if self.bytes_hard > 0 && self.bytes_used >= self.bytes_hard {
            QuotaStatus::HardExceeded
        } else if self.bytes_soft > 0 && self.bytes_used > self.bytes_soft {
            QuotaStatus::SoftExceeded
        } else {
            QuotaStatus::Ok
        }
    }

    pub fn inodes_status(&self) -> QuotaStatus {
        if self.inodes_hard > 0 && self.inodes_used >= self.inodes_hard {
            QuotaStatus::HardExceeded
        } else if self.inodes_soft > 0 && self.inodes_used > self.inodes_soft {
            QuotaStatus::SoftExceeded
        } else {
            QuotaStatus::Ok
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum QuotaStatus {
    Ok,
    SoftExceeded,
    HardExceeded,
}

struct QuotaCacheEntry {
    usage: QuotaUsage,
    fetched_at: Instant,
}

pub struct QuotaEnforcer {
    ttl: Duration,
    user_cache: HashMap<u32, QuotaCacheEntry>,
    group_cache: HashMap<u32, QuotaCacheEntry>,
    check_count: u64,
    cache_hits: u64,
    denied_count: u64,
}

impl QuotaEnforcer {
    pub fn new(ttl: Duration) -> Self {
        QuotaEnforcer {
            ttl,
            user_cache: HashMap::new(),
            group_cache: HashMap::new(),
            check_count: 0,
            cache_hits: 0,
            denied_count: 0,
        }
    }

    pub fn with_default_ttl() -> Self {
        Self::new(Duration::from_secs(30))
    }

    pub fn update_user_quota(&mut self, uid: u32, usage: QuotaUsage) {
        self.user_cache.insert(
            uid,
            QuotaCacheEntry {
                usage,
                fetched_at: Instant::now(),
            },
        );
    }

    pub fn update_group_quota(&mut self, gid: u32, usage: QuotaUsage) {
        self.group_cache.insert(
            gid,
            QuotaCacheEntry {
                usage,
                fetched_at: Instant::now(),
            },
        );
    }

    fn get_user_usage(&self, uid: u32) -> Option<QuotaUsage> {
        self.user_cache.get(&uid).and_then(|entry| {
            if entry.fetched_at.elapsed() < self.ttl {
                Some(entry.usage.clone())
            } else {
                None
            }
        })
    }

    fn get_group_usage(&self, gid: u32) -> Option<QuotaUsage> {
        self.group_cache.get(&gid).and_then(|entry| {
            if entry.fetched_at.elapsed() < self.ttl {
                Some(entry.usage.clone())
            } else {
                None
            }
        })
    }

    pub fn check_write(&mut self, uid: u32, gid: u32, write_size: u64) -> Result<QuotaStatus> {
        self.check_count += 1;

        if let Some(user_usage) = self.get_user_usage(uid) {
            self.cache_hits += 1;
            let projected = user_usage.bytes_used.saturating_add(write_size);

            if user_usage.bytes_hard > 0 && projected >= user_usage.bytes_hard {
                self.denied_count += 1;
                return Err(FuseError::PermissionDenied {
                    ino: 0,
                    op: "write: quota exceeded".into(),
                });
            }

            if user_usage.bytes_soft > 0 && projected > user_usage.bytes_soft {
                tracing::warn!("User {} soft quota exceeded", uid);
                return Ok(QuotaStatus::SoftExceeded);
            }
        }

        if let Some(group_usage) = self.get_group_usage(gid) {
            self.cache_hits += 1;
            let projected = group_usage.bytes_used.saturating_add(write_size);

            if group_usage.bytes_hard > 0 && projected >= group_usage.bytes_hard {
                self.denied_count += 1;
                return Err(FuseError::PermissionDenied {
                    ino: 0,
                    op: "write: quota exceeded".into(),
                });
            }

            if group_usage.bytes_soft > 0 && projected > group_usage.bytes_soft {
                tracing::warn!("Group {} soft quota exceeded", gid);
                return Ok(QuotaStatus::SoftExceeded);
            }
        }

        Ok(QuotaStatus::Ok)
    }

    pub fn check_create(&mut self, uid: u32, gid: u32) -> Result<QuotaStatus> {
        self.check_count += 1;

        if let Some(user_usage) = self.get_user_usage(uid) {
            self.cache_hits += 1;
            let projected = user_usage.inodes_used.saturating_add(1);

            if user_usage.inodes_hard > 0 && projected >= user_usage.inodes_hard {
                self.denied_count += 1;
                return Err(FuseError::PermissionDenied {
                    ino: 0,
                    op: "create: inode quota exceeded".into(),
                });
            }

            if user_usage.inodes_soft > 0 && projected > user_usage.inodes_soft {
                tracing::warn!("User {} soft inode quota exceeded", uid);
                return Ok(QuotaStatus::SoftExceeded);
            }
        }

        if let Some(group_usage) = self.get_group_usage(gid) {
            self.cache_hits += 1;
            let projected = group_usage.inodes_used.saturating_add(1);

            if group_usage.inodes_hard > 0 && projected >= group_usage.inodes_hard {
                self.denied_count += 1;
                return Err(FuseError::PermissionDenied {
                    ino: 0,
                    op: "create: inode quota exceeded".into(),
                });
            }

            if group_usage.inodes_soft > 0 && projected > group_usage.inodes_soft {
                tracing::warn!("Group {} soft inode quota exceeded", gid);
                return Ok(QuotaStatus::SoftExceeded);
            }
        }

        Ok(QuotaStatus::Ok)
    }

    pub fn invalidate_user(&mut self, uid: u32) {
        self.user_cache.remove(&uid);
    }

    pub fn invalidate_group(&mut self, gid: u32) {
        self.group_cache.remove(&gid);
    }

    pub fn cache_hits(&self) -> u64 {
        self.cache_hits
    }

    pub fn check_count(&self) -> u64 {
        self.check_count
    }

    pub fn denied_count(&self) -> u64 {
        self.denied_count
    }

    pub fn cache_size(&self) -> usize {
        self.user_cache.len() + self.group_cache.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quota_usage_unlimited_has_all_zeros() {
        let usage = QuotaUsage::unlimited();
        assert_eq!(usage.bytes_used, 0);
        assert_eq!(usage.bytes_soft, 0);
        assert_eq!(usage.bytes_hard, 0);
        assert_eq!(usage.inodes_used, 0);
        assert_eq!(usage.inodes_soft, 0);
        assert_eq!(usage.inodes_hard, 0);
    }

    #[test]
    fn test_quota_bytes_status_ok_when_under_soft() {
        let mut usage = QuotaUsage::new(100, 200);
        usage.bytes_used = 50;
        assert_eq!(usage.bytes_status(), QuotaStatus::Ok);
    }

    #[test]
    fn test_quota_bytes_status_soft_exceeded_when_over_soft() {
        let mut usage = QuotaUsage::new(100, 200);
        usage.bytes_used = 150;
        assert_eq!(usage.bytes_status(), QuotaStatus::SoftExceeded);
    }

    #[test]
    fn test_quota_bytes_status_hard_exceeded_when_at_hard() {
        let mut usage = QuotaUsage::new(100, 200);
        usage.bytes_used = 200;
        assert_eq!(usage.bytes_status(), QuotaStatus::HardExceeded);
    }

    #[test]
    fn test_quota_bytes_status_ok_when_no_limits() {
        let usage = QuotaUsage::unlimited();
        assert_eq!(usage.bytes_status(), QuotaStatus::Ok);
    }

    #[test]
    fn test_quota_inodes_status_ok_when_under_soft() {
        let mut usage = QuotaUsage::new(0, 0);
        usage.inodes_soft = 10;
        usage.inodes_hard = 20;
        usage.inodes_used = 5;
        assert_eq!(usage.inodes_status(), QuotaStatus::Ok);
    }

    #[test]
    fn test_quota_inodes_status_hard_exceeded_when_at_hard() {
        let mut usage = QuotaUsage::new(0, 0);
        usage.inodes_soft = 10;
        usage.inodes_hard = 20;
        usage.inodes_used = 20;
        assert_eq!(usage.inodes_status(), QuotaStatus::HardExceeded);
    }

    #[test]
    fn test_enforcer_new_starts_empty() {
        let enforcer = QuotaEnforcer::with_default_ttl();
        assert_eq!(enforcer.cache_size(), 0);
        assert_eq!(enforcer.check_count(), 0);
    }

    #[test]
    fn test_enforcer_check_write_ok_when_no_cached_entry() {
        let mut enforcer = QuotaEnforcer::with_default_ttl();
        let result = enforcer.check_write(1000, 1000, 100);
        assert_eq!(result.unwrap(), QuotaStatus::Ok);
    }

    #[test]
    fn test_enforcer_check_write_ok_when_under_limits() {
        let mut enforcer = QuotaEnforcer::with_default_ttl();
        let mut usage = QuotaUsage::new(1000, 2000);
        usage.bytes_used = 100;
        enforcer.update_user_quota(100, usage.clone());
        let result = enforcer.check_write(100, 200, 100);
        assert_eq!(result.unwrap(), QuotaStatus::Ok);
    }

    #[test]
    fn test_enforcer_check_write_soft_exceeded_returns_ok_with_status() {
        let mut enforcer = QuotaEnforcer::with_default_ttl();
        let mut usage = QuotaUsage::new(100, 1000);
        usage.bytes_used = 90;
        enforcer.update_user_quota(100, usage);
        let result = enforcer.check_write(100, 200, 50);
        assert_eq!(result.unwrap(), QuotaStatus::SoftExceeded);
    }

    #[test]
    fn test_enforcer_check_write_hard_exceeded_returns_err() {
        let mut enforcer = QuotaEnforcer::with_default_ttl();
        let mut usage = QuotaUsage::new(100, 200);
        usage.bytes_used = 180;
        enforcer.update_user_quota(100, usage);
        let result = enforcer.check_write(100, 200, 50);
        assert!(result.is_err());
    }

    #[test]
    fn test_enforcer_check_create_ok_when_no_inode_limits() {
        let mut enforcer = QuotaEnforcer::with_default_ttl();
        let usage = QuotaUsage::new(1000, 2000);
        enforcer.update_user_quota(100, usage);
        let result = enforcer.check_create(100, 200);
        assert_eq!(result.unwrap(), QuotaStatus::Ok);
    }

    #[test]
    fn test_enforcer_check_create_hard_exceeded_returns_err() {
        let mut enforcer = QuotaEnforcer::with_default_ttl();
        let mut usage = QuotaUsage::new(0, 0);
        usage.inodes_soft = 10;
        usage.inodes_hard = 10;
        usage.inodes_used = 9;
        enforcer.update_user_quota(100, usage);
        let result = enforcer.check_create(100, 200);
        assert!(result.is_err());
    }

    #[test]
    fn test_enforcer_cache_hit_count_increments() {
        let mut enforcer = QuotaEnforcer::with_default_ttl();
        let usage = QuotaUsage::new(1000, 2000);
        enforcer.update_user_quota(100, usage);
        enforcer.check_write(100, 200, 10).unwrap();
        assert_eq!(enforcer.cache_hits(), 1);
    }

    #[test]
    fn test_enforcer_denied_count_increments_on_denial() {
        let mut enforcer = QuotaEnforcer::with_default_ttl();
        let mut usage = QuotaUsage::new(100, 200);
        usage.bytes_used = 200;
        enforcer.update_user_quota(100, usage);
        let _ = enforcer.check_write(100, 200, 1);
        assert_eq!(enforcer.denied_count(), 1);
    }

    #[test]
    fn test_enforcer_invalidate_user_removes_entry() {
        let mut enforcer = QuotaEnforcer::with_default_ttl();
        enforcer.update_user_quota(100, QuotaUsage::unlimited());
        assert_eq!(enforcer.cache_size(), 1);
        enforcer.invalidate_user(100);
        assert_eq!(enforcer.cache_size(), 0);
    }

    #[test]
    fn test_enforcer_invalidate_group_removes_entry() {
        let mut enforcer = QuotaEnforcer::with_default_ttl();
        enforcer.update_group_quota(100, QuotaUsage::unlimited());
        assert_eq!(enforcer.cache_size(), 1);
        enforcer.invalidate_group(100);
        assert_eq!(enforcer.cache_size(), 0);
    }

    #[test]
    fn test_enforcer_expired_entry_treated_as_missing() {
        let mut enforcer = QuotaEnforcer::new(Duration::from_millis(1));
        let mut usage = QuotaUsage::new(100, 200);
        usage.bytes_used = 200;
        enforcer.update_user_quota(100, usage);
        std::thread::sleep(std::time::Duration::from_millis(5));
        let result = enforcer.check_write(100, 200, 1);
        assert_eq!(result.unwrap(), QuotaStatus::Ok);
    }

    #[test]
    fn test_enforcer_cache_size_reflects_entries() {
        let mut enforcer = QuotaEnforcer::with_default_ttl();
        enforcer.update_user_quota(100, QuotaUsage::unlimited());
        enforcer.update_group_quota(200, QuotaUsage::unlimited());
        assert_eq!(enforcer.cache_size(), 2);
    }
}
