use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum TieringError {
    #[error("Invalid policy: {0}")]
    InvalidPolicy(String),
    #[error("Path not found: {0}")]
    PathNotFound(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TieringMode {
    Cache,
    Tiered,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TierTarget {
    Flash,
    S3,
    Auto,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TieringPolicy {
    pub path: String,
    pub target: TierTarget,
    pub min_age_days: Option<u64>,
    pub min_size_bytes: Option<u64>,
    pub recursive: bool,
}

impl TieringPolicy {
    pub fn new_pin(path: String) -> Self {
        Self {
            path,
            target: TierTarget::Flash,
            min_age_days: None,
            min_size_bytes: None,
            recursive: true,
        }
    }

    pub fn new_archive(path: String, age_days: u64) -> Self {
        Self {
            path,
            target: TierTarget::S3,
            min_age_days: Some(age_days),
            min_size_bytes: None,
            recursive: true,
        }
    }

    pub fn new_auto(path: String) -> Self {
        Self {
            path,
            target: TierTarget::Auto,
            min_age_days: None,
            min_size_bytes: None,
            recursive: true,
        }
    }

    pub fn is_eligible_for_eviction(&self, age_days: u64, size_bytes: u64) -> bool {
        if self.target == TierTarget::Flash {
            return false;
        }

        if let Some(min_age) = self.min_age_days {
            if age_days < min_age {
                return false;
            }
        }

        if let Some(min_size) = self.min_size_bytes {
            if size_bytes < min_size {
                return false;
            }
        }

        true
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlashUtilization {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub reserved_bytes: u64,
}

impl FlashUtilization {
    pub fn new(total_bytes: u64, used_bytes: u64, reserved_bytes: u64) -> Self {
        Self {
            total_bytes,
            used_bytes,
            reserved_bytes,
        }
    }

    pub fn usage_percent(&self) -> f64 {
        if self.total_bytes == 0 {
            return 0.0;
        }
        (self.used_bytes as f64 / self.total_bytes as f64) * 100.0
    }

    pub fn available_bytes(&self) -> u64 {
        self.total_bytes.saturating_sub(self.used_bytes)
    }

    pub fn is_high_watermark(&self) -> bool {
        self.usage_percent() > 80.0
    }

    pub fn is_low_watermark(&self) -> bool {
        self.usage_percent() < 60.0
    }

    pub fn is_critical(&self) -> bool {
        self.usage_percent() > 95.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvictionCandidate {
    pub inode: u64,
    pub path: String,
    pub size_bytes: u64,
    pub last_access_days: u64,
    pub score: f64,
    pub confirmed_in_s3: bool,
}

impl EvictionCandidate {
    pub fn new(
        inode: u64,
        path: String,
        size_bytes: u64,
        last_access_days: u64,
        confirmed_in_s3: bool,
    ) -> Self {
        let score = (last_access_days as f64) * (size_bytes as f64);
        Self {
            inode,
            path,
            size_bytes,
            last_access_days,
            score,
            confirmed_in_s3,
        }
    }

    pub fn is_evictable(&self) -> bool {
        self.confirmed_in_s3
    }
}

pub struct TieringManager {
    global_mode: TieringMode,
    policies: HashMap<String, TieringPolicy>,
    flash_utilization: FlashUtilization,
}

impl TieringManager {
    pub fn new(mode: TieringMode, total_flash_bytes: u64) -> Self {
        Self {
            global_mode: mode,
            policies: HashMap::new(),
            flash_utilization: FlashUtilization::new(total_flash_bytes, 0, 0),
        }
    }

    pub fn set_policy(&mut self, policy: TieringPolicy) {
        self.policies.insert(policy.path.clone(), policy);
    }

    pub fn remove_policy(&mut self, path: &str) -> Option<TieringPolicy> {
        self.policies.remove(path)
    }

    pub fn get_policy(&self, path: &str) -> Option<&TieringPolicy> {
        self.policies.get(path)
    }

    pub fn effective_policy(&self, path: &str) -> Option<&TieringPolicy> {
        if let Some(policy) = self.policies.get(path) {
            return Some(policy);
        }

        let mut parent = std::path::Path::new(path);
        while let Some(parent_path) = parent.parent() {
            let parent_str = parent_path.to_string_lossy().to_string();
            if let Some(policy) = self.policies.get(&parent_str) {
                if policy.recursive {
                    return Some(policy);
                }
                return None;
            }
            parent = parent_path;
        }

        None
    }

    pub fn update_utilization(&mut self, used_bytes: u64) {
        self.flash_utilization = FlashUtilization::new(
            self.flash_utilization.total_bytes,
            used_bytes,
            self.flash_utilization.reserved_bytes,
        );
    }

    pub fn flash_util(&self) -> &FlashUtilization {
        &self.flash_utilization
    }

    pub fn global_mode(&self) -> &TieringMode {
        &self.global_mode
    }

    pub fn rank_eviction_candidates(
        &self,
        mut candidates: Vec<EvictionCandidate>,
    ) -> Vec<EvictionCandidate> {
        candidates.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        candidates
    }

    pub fn filter_evictable(&self, candidates: Vec<EvictionCandidate>) -> Vec<EvictionCandidate> {
        candidates
            .into_iter()
            .filter(|c| c.is_evictable())
            .filter(|c| {
                if let Some(policy) = self.effective_policy(&c.path) {
                    if policy.target == TierTarget::Flash {
                        return false;
                    }
                }
                true
            })
            .collect()
    }

    pub fn policy_count(&self) -> usize {
        self.policies.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tiering_policy_new_pin() {
        let policy = TieringPolicy::new_pin("/data".to_string());
        assert_eq!(policy.target, TierTarget::Flash);
        assert!(policy.recursive);
    }

    #[test]
    fn test_tiering_policy_new_archive() {
        let policy = TieringPolicy::new_archive("/archive".to_string(), 30);
        assert_eq!(policy.target, TierTarget::S3);
        assert_eq!(policy.min_age_days, Some(30));
    }

    #[test]
    fn test_tiering_policy_new_auto() {
        let policy = TieringPolicy::new_auto("/data".to_string());
        assert_eq!(policy.target, TierTarget::Auto);
    }

    #[test]
    fn test_is_eligible_too_young() {
        let policy = TieringPolicy::new_archive("/data".to_string(), 30);
        assert!(!policy.is_eligible_for_eviction(10, 1000));
    }

    #[test]
    fn test_is_eligible_old_enough() {
        let policy = TieringPolicy::new_archive("/data".to_string(), 30);
        assert!(policy.is_eligible_for_eviction(40, 1000));
    }

    #[test]
    fn test_is_eligible_too_small() {
        let policy = TieringPolicy::new_archive("/data".to_string(), 30);
        let policy = TieringPolicy {
            min_size_bytes: Some(1000),
            ..policy
        };
        assert!(!policy.is_eligible_for_eviction(40, 500));
    }

    #[test]
    fn test_is_eligible_flash_pinned() {
        let policy = TieringPolicy::new_pin("/data".to_string());
        assert!(!policy.is_eligible_for_eviction(100, 1000));
    }

    #[test]
    fn test_flash_utilization_usage_percent() {
        let util = FlashUtilization::new(1000, 500, 0);
        assert!((util.usage_percent() - 50.0).abs() < 0.001);
    }

    #[test]
    fn test_flash_utilization_available_bytes() {
        let util = FlashUtilization::new(1000, 300, 0);
        assert_eq!(util.available_bytes(), 700);
    }

    #[test]
    fn test_is_high_watermark() {
        let util_81 = FlashUtilization::new(1000, 810, 0);
        let util_79 = FlashUtilization::new(1000, 790, 0);
        assert!(util_81.is_high_watermark());
        assert!(!util_79.is_high_watermark());
    }

    #[test]
    fn test_is_low_watermark() {
        let util_59 = FlashUtilization::new(1000, 590, 0);
        let util_61 = FlashUtilization::new(1000, 610, 0);
        assert!(util_59.is_low_watermark());
        assert!(!util_61.is_low_watermark());
    }

    #[test]
    fn test_is_critical() {
        let util_96 = FlashUtilization::new(1000, 960, 0);
        let util_94 = FlashUtilization::new(1000, 940, 0);
        assert!(util_96.is_critical());
        assert!(!util_94.is_critical());
    }

    #[test]
    fn test_eviction_candidate_score() {
        let candidate = EvictionCandidate::new(1, "/test".to_string(), 1000, 10, true);
        assert_eq!(candidate.score, 10000.0);
    }

    #[test]
    fn test_eviction_candidate_not_evictable() {
        let candidate = EvictionCandidate::new(1, "/test".to_string(), 1000, 10, false);
        assert!(!candidate.is_evictable());
    }

    #[test]
    fn test_eviction_candidate_evictable() {
        let candidate = EvictionCandidate::new(1, "/test".to_string(), 1000, 10, true);
        assert!(candidate.is_evictable());
    }

    #[test]
    fn test_tiering_manager_new() {
        let manager = TieringManager::new(TieringMode::Cache, 1000);
        assert_eq!(manager.global_mode(), &TieringMode::Cache);
        assert_eq!(manager.flash_util().total_bytes, 1000);
    }

    #[test]
    fn test_set_and_get_policy() {
        let mut manager = TieringManager::new(TieringMode::Cache, 1000);
        let policy = TieringPolicy::new_pin("/data".to_string());
        manager.set_policy(policy.clone());
        assert_eq!(
            manager.get_policy("/data").unwrap().target,
            TierTarget::Flash
        );
    }

    #[test]
    fn test_remove_policy() {
        let mut manager = TieringManager::new(TieringMode::Cache, 1000);
        let policy = TieringPolicy::new_pin("/data".to_string());
        manager.set_policy(policy);
        let removed = manager.remove_policy("/data");
        assert!(removed.is_some());
        assert!(manager.get_policy("/data").is_none());
    }

    #[test]
    fn test_effective_policy_finds_parent() {
        let mut manager = TieringManager::new(TieringMode::Cache, 1000);
        let policy = TieringPolicy::new_auto("/data".to_string());
        manager.set_policy(policy);
        let found = manager.effective_policy("/data/file.txt");
        assert!(found.is_some());
        assert_eq!(found.unwrap().target, TierTarget::Auto);
    }

    #[test]
    fn test_effective_policy_no_match() {
        let manager = TieringManager::new(TieringMode::Cache, 1000);
        let found = manager.effective_policy("/data/file.txt");
        assert!(found.is_none());
    }

    #[test]
    fn test_update_utilization() {
        let mut manager = TieringManager::new(TieringMode::Cache, 1000);
        manager.update_utilization(500);
        assert_eq!(manager.flash_util().used_bytes, 500);
    }

    #[test]
    fn test_rank_eviction_candidates() {
        let mut manager = TieringManager::new(TieringMode::Cache, 1000);
        let candidates = vec![
            EvictionCandidate::new(1, "/a".to_string(), 100, 10, true),
            EvictionCandidate::new(2, "/b".to_string(), 200, 10, true),
            EvictionCandidate::new(3, "/c".to_string(), 50, 10, true),
        ];
        let ranked = manager.rank_eviction_candidates(candidates);
        assert_eq!(ranked[0].path, "/b");
        assert_eq!(ranked[1].path, "/a");
        assert_eq!(ranked[2].path, "/c");
    }

    #[test]
    fn test_filter_evictable_removes_unconfirmed() {
        let manager = TieringManager::new(TieringMode::Cache, 1000);
        let candidates = vec![
            EvictionCandidate::new(1, "/a".to_string(), 100, 10, true),
            EvictionCandidate::new(2, "/b".to_string(), 200, 10, false),
        ];
        let filtered = manager.filter_evictable(candidates);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].inode, 1);
    }

    #[test]
    fn test_filter_evictable_removes_pinned() {
        let mut manager = TieringManager::new(TieringMode::Cache, 1000);
        let policy = TieringPolicy::new_pin("/pinned".to_string());
        manager.set_policy(policy);

        let candidates = vec![
            EvictionCandidate::new(1, "/pinned/data".to_string(), 100, 10, true),
            EvictionCandidate::new(2, "/other/data".to_string(), 200, 10, true),
        ];
        let filtered = manager.filter_evictable(candidates);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].inode, 2);
    }

    #[test]
    fn test_policy_count() {
        let mut manager = TieringManager::new(TieringMode::Cache, 1000);
        assert_eq!(manager.policy_count(), 0);
        manager.set_policy(TieringPolicy::new_pin("/a".to_string()));
        manager.set_policy(TieringPolicy::new_pin("/b".to_string()));
        assert_eq!(manager.policy_count(), 2);
    }
}
