//! Tiering Policy Xattr Support (D5)
//!
//! Per architecture Decision D5, files and directories can have a `claudefs.tier` xattr
//! that controls flash/S3 tiering behavior.

use std::collections::HashMap;
use thiserror::Error;

/// Extended attribute name for tiering policy.
pub const XATTR_TIERING_POLICY: &str = "claudefs.tier";

/// Extended attribute name for tiering priority (0-255).
pub const XATTR_TIERING_PRIORITY: &str = "claudefs.tier.priority";

/// Tiering policy controlling how data moves between flash and S3 tiers.
#[derive(Debug, Clone, PartialEq)]
pub enum TieringPolicy {
    /// Automatic tiering based on access patterns.
    Auto,
    /// Keep data pinned in flash tier (never evict to S3).
    Flash,
    /// Force data to S3 tier immediately (cold storage).
    S3,
    /// Custom policy with explicit eviction and replication parameters.
    Custom {
        /// Seconds of inactivity before eviction to S3.
        evict_after_secs: u64,
        /// Minimum number of copies to maintain across tiers.
        min_copies: u8,
    },
}

impl TieringPolicy {
    /// Parse a tiering policy from an xattr value.
    ///
    /// Accepted formats:
    /// - `"auto"` -> [`TieringPolicy::Auto`]
    /// - `"flash"` -> [`TieringPolicy::Flash`]
    /// - `"s3"` -> [`TieringPolicy::S3`]
    /// - `"custom:<evict_after_secs>:<min_copies>"` -> [`TieringPolicy::Custom`]
    pub fn from_xattr_value(value: &[u8]) -> Result<Self, TieringError> {
        let s = String::from_utf8(value.to_vec())
            .map_err(|_| TieringError::InvalidPolicy(value.to_vec()))?;

        match s.as_str() {
            "auto" => Ok(TieringPolicy::Auto),
            "flash" => Ok(TieringPolicy::Flash),
            "s3" => Ok(TieringPolicy::S3),
            _ => {
                if let Some(rest) = s.strip_prefix("custom:") {
                    let parts: Vec<&str> = rest.split(':').collect();
                    if parts.len() == 2 {
                        let evict_after_secs: u64 = parts[0]
                            .parse()
                            .map_err(|_| TieringError::InvalidPolicy(value.to_vec()))?;
                        let min_copies: u8 = parts[1]
                            .parse()
                            .map_err(|_| TieringError::InvalidPolicy(value.to_vec()))?;
                        Ok(TieringPolicy::Custom {
                            evict_after_secs,
                            min_copies,
                        })
                    } else {
                        Err(TieringError::InvalidPolicy(value.to_vec()))
                    }
                } else {
                    Err(TieringError::InvalidPolicy(value.to_vec()))
                }
            }
        }
    }

    /// Serialize the policy to an xattr value.
    pub fn to_xattr_value(&self) -> Vec<u8> {
        match self {
            TieringPolicy::Auto => b"auto".to_vec(),
            TieringPolicy::Flash => b"flash".to_vec(),
            TieringPolicy::S3 => b"s3".to_vec(),
            TieringPolicy::Custom {
                evict_after_secs,
                min_copies,
            } => format!("custom:{}:{}", evict_after_secs, min_copies).into_bytes(),
        }
    }

    /// Returns `true` if this policy pins data to flash (never evicts).
    pub fn is_pinned(&self) -> bool {
        matches!(self, TieringPolicy::Flash)
    }

    /// Returns `true` if this policy forces data to S3 (cold tier).
    pub fn is_forced_cold(&self) -> bool {
        matches!(self, TieringPolicy::S3)
    }
}

/// Priority value for tiering decisions (0-255, higher = more likely to evict).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct TieringPriority(pub u8);

impl TieringPriority {
    /// Minimum priority (0) - least likely to be evicted.
    pub const MIN: Self = TieringPriority(0);

    /// Maximum priority (255) - most likely to be evicted.
    pub const MAX: Self = TieringPriority(255);

    /// Default priority (128).
    pub const DEFAULT: Self = TieringPriority(128);

    /// Parse a priority from an xattr value (decimal string).
    pub fn from_xattr_value(value: &[u8]) -> Result<Self, TieringError> {
        let s = String::from_utf8(value.to_vec())
            .map_err(|_| TieringError::InvalidPriority(value.to_vec()))?;
        let v: u8 = s
            .parse()
            .map_err(|_| TieringError::InvalidPriority(value.to_vec()))?;
        Ok(TieringPriority(v))
    }

    /// Serialize the priority to an xattr value.
    pub fn to_xattr_value(&self) -> Vec<u8> {
        self.0.to_string().into_bytes()
    }
}

/// Tiering hint associated with an inode.
#[derive(Debug, Clone)]
pub struct TieringHint {
    /// Inode number.
    pub ino: u64,
    /// Tiering policy for this inode.
    pub policy: TieringPolicy,
    /// Priority for eviction decisions.
    pub priority: TieringPriority,
    /// Whether this hint applies to a directory.
    pub is_directory: bool,
    /// Timestamp when the hint was set (epoch seconds).
    pub set_at_secs: u64,
}

impl TieringHint {
    /// Create a new tiering hint with default priority.
    pub fn new(ino: u64, policy: TieringPolicy, is_directory: bool, now_secs: u64) -> Self {
        Self {
            ino,
            policy,
            priority: TieringPriority::DEFAULT,
            is_directory,
            set_at_secs: now_secs,
        }
    }

    /// Set a custom priority (builder pattern).
    pub fn with_priority(mut self, priority: TieringPriority) -> Self {
        self.priority = priority;
        self
    }

    /// Calculate eviction score for this hint.
    ///
    /// Higher scores indicate more suitable eviction candidates.
    /// Returns 0 for pinned data, `u64::MAX` for forced-cold data.
    pub fn evict_score(&self, last_access_age_secs: u64, size_bytes: u64) -> u64 {
        if self.policy.is_pinned() {
            return 0;
        }
        if self.policy.is_forced_cold() {
            return u64::MAX;
        }
        last_access_age_secs.saturating_mul(size_bytes)
    }
}

/// In-memory cache for tiering hints with inheritance support.
pub struct TieringHintCache {
    /// Tiering hints indexed by inode.
    hints: HashMap<u64, TieringHint>,
    /// Parent inode mappings for inheritance lookup.
    parent_hints: HashMap<u64, u64>,
    /// Maximum entries before trimming.
    max_entries: usize,
}

impl TieringHintCache {
    /// Create a new cache with the given capacity.
    pub fn new(max_entries: usize) -> Self {
        Self {
            hints: HashMap::new(),
            parent_hints: HashMap::new(),
            max_entries,
        }
    }

    /// Insert a hint into the cache, trimming if at capacity.
    pub fn insert(&mut self, hint: TieringHint) {
        if self.hints.len() >= self.max_entries {
            self.trim();
        }
        self.hints.insert(hint.ino, hint);
    }

    /// Get a hint by inode number.
    pub fn get(&self, ino: u64) -> Option<&TieringHint> {
        self.hints.get(&ino)
    }

    /// Remove a hint by inode number.
    pub fn remove(&mut self, ino: u64) -> Option<TieringHint> {
        self.hints.remove(&ino)
    }

    /// Return the number of cached hints.
    pub fn len(&self) -> usize {
        self.hints.len()
    }

    /// Returns `true` if the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.hints.is_empty()
    }

    /// Set the parent inode for inheritance lookups.
    pub fn set_parent(&mut self, ino: u64, parent_ino: u64) {
        self.parent_hints.insert(ino, parent_ino);
    }

    /// Get the effective policy for an inode, inheriting from parent directories.
    ///
    /// Returns [`TieringPolicy::Auto`] if no explicit or inherited policy exists.
    pub fn effective_policy(&self, ino: u64) -> TieringPolicy {
        if let Some(hint) = self.hints.get(&ino) {
            return hint.policy.clone();
        }

        let mut current = ino;
        while let Some(&parent_ino) = self.parent_hints.get(&current) {
            if let Some(hint) = self.hints.get(&parent_ino) {
                if hint.is_directory {
                    return hint.policy.clone();
                }
            }
            current = parent_ino;
        }

        TieringPolicy::Auto
    }

    /// Get eviction candidates sorted by score (highest first).
    ///
    /// Excludes pinned (score=0) and forced-cold (score=MAX) entries.
    /// Only includes entries with score >= `min_score`.
    pub fn eviction_candidates(
        &self,
        access_ages: &HashMap<u64, u64>,
        sizes: &HashMap<u64, u64>,
        min_score: u64,
    ) -> Vec<(u64, u64)> {
        let mut candidates: Vec<(u64, u64)> = Vec::new();

        for (ino, hint) in &self.hints {
            let age = access_ages.get(ino).copied().unwrap_or(0);
            let size = sizes.get(ino).copied().unwrap_or(0);
            let score = hint.evict_score(age, size);

            if score > 0 && score >= min_score && score < u64::MAX {
                candidates.push((*ino, score));
            }
        }

        candidates.sort_by(|a, b| b.1.cmp(&a.1));
        candidates
    }

    /// Trim the cache by removing 10 oldest entries when at capacity.
    pub fn trim(&mut self) {
        if self.hints.len() >= self.max_entries {
            let to_remove: Vec<u64> = self.hints.keys().cloned().take(10).collect();
            for ino in to_remove {
                self.hints.remove(&ino);
            }
        }
    }
}

/// Errors when parsing tiering xattr values.
#[derive(Debug, Error)]
pub enum TieringError {
    /// Invalid tiering policy value.
    #[error("Invalid tiering policy value: {0:?}")]
    InvalidPolicy(Vec<u8>),
    /// Invalid priority value.
    #[error("Invalid priority value: {0:?}")]
    InvalidPriority(Vec<u8>),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_auto() {
        let policy = TieringPolicy::from_xattr_value(b"auto").unwrap();
        assert!(matches!(policy, TieringPolicy::Auto));
    }

    #[test]
    fn test_parse_flash() {
        let policy = TieringPolicy::from_xattr_value(b"flash").unwrap();
        assert!(matches!(policy, TieringPolicy::Flash));
    }

    #[test]
    fn test_parse_s3() {
        let policy = TieringPolicy::from_xattr_value(b"s3").unwrap();
        assert!(matches!(policy, TieringPolicy::S3));
    }

    #[test]
    fn test_parse_custom() {
        let policy = TieringPolicy::from_xattr_value(b"custom:3600:2").unwrap();
        match policy {
            TieringPolicy::Custom {
                evict_after_secs,
                min_copies,
            } => {
                assert_eq!(evict_after_secs, 3600);
                assert_eq!(min_copies, 2);
            }
            _ => panic!("Expected Custom"),
        }
    }

    #[test]
    fn test_invalid_policy() {
        let result = TieringPolicy::from_xattr_value(b"invalid");
        assert!(result.is_err());
    }

    #[test]
    fn test_to_xattr_value_auto() {
        let policy = TieringPolicy::Auto;
        assert_eq!(policy.to_xattr_value(), b"auto");
    }

    #[test]
    fn test_to_xattr_value_custom() {
        let policy = TieringPolicy::Custom {
            evict_after_secs: 7200,
            min_copies: 3,
        };
        let val = policy.to_xattr_value();
        assert_eq!(val, b"custom:7200:3");
    }

    #[test]
    fn test_round_trip() {
        let original = TieringPolicy::Custom {
            evict_after_secs: 3600,
            min_copies: 2,
        };
        let encoded = original.to_xattr_value();
        let parsed = TieringPolicy::from_xattr_value(&encoded).unwrap();
        assert_eq!(original, parsed);
    }

    #[test]
    fn test_is_pinned() {
        assert!(TieringPolicy::Flash.is_pinned());
        assert!(!TieringPolicy::Auto.is_pinned());
        assert!(!TieringPolicy::S3.is_pinned());
        assert!(!TieringPolicy::Custom {
            evict_after_secs: 0,
            min_copies: 1
        }
        .is_pinned());
    }

    #[test]
    fn test_is_forced_cold() {
        assert!(TieringPolicy::S3.is_forced_cold());
        assert!(!TieringPolicy::Auto.is_forced_cold());
        assert!(!TieringPolicy::Flash.is_forced_cold());
    }

    #[test]
    fn test_priority_from_xattr() {
        let p = TieringPriority::from_xattr_value(b"100").unwrap();
        assert_eq!(p.0, 100);
    }

    #[test]
    fn test_priority_to_xattr() {
        let p = TieringPriority(200);
        assert_eq!(p.to_xattr_value(), b"200");
    }

    #[test]
    fn test_priority_constants() {
        assert_eq!(TieringPriority::MIN.0, 0);
        assert_eq!(TieringPriority::MAX.0, 255);
        assert_eq!(TieringPriority::DEFAULT.0, 128);
    }

    #[test]
    fn test_evict_score_pinned() {
        let hint = TieringHint::new(1, TieringPolicy::Flash, false, 1000);
        assert_eq!(hint.evict_score(1000, 10000), 0);
    }

    #[test]
    fn test_evict_score_s3() {
        let hint = TieringHint::new(1, TieringPolicy::S3, false, 1000);
        assert_eq!(hint.evict_score(1000, 10000), u64::MAX);
    }

    #[test]
    fn test_evict_score_auto() {
        let hint = TieringHint::new(1, TieringPolicy::Auto, false, 1000);
        assert_eq!(hint.evict_score(100, 1000), 100000);
    }

    #[test]
    fn test_hint_cache_insert_get() {
        let mut cache = TieringHintCache::new(100);
        let hint = TieringHint::new(1, TieringPolicy::Flash, false, 1000);
        cache.insert(hint);

        let retrieved = cache.get(1).unwrap();
        assert!(matches!(retrieved.policy, TieringPolicy::Flash));
    }

    #[test]
    fn test_hint_cache_remove() {
        let mut cache = TieringHintCache::new(100);
        let hint = TieringHint::new(1, TieringPolicy::Flash, false, 1000);
        cache.insert(hint);

        let removed = cache.remove(1);
        assert!(removed.is_some());
        assert!(cache.get(1).is_none());
    }

    #[test]
    fn test_hint_cache_len() {
        let mut cache = TieringHintCache::new(100);
        assert_eq!(cache.len(), 0);

        cache.insert(TieringHint::new(1, TieringPolicy::Auto, false, 1000));
        cache.insert(TieringHint::new(2, TieringPolicy::Flash, false, 1000));

        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn test_effective_policy_explicit() {
        let mut cache = TieringHintCache::new(100);
        cache.insert(TieringHint::new(1, TieringPolicy::Flash, false, 1000));

        assert!(matches!(cache.effective_policy(1), TieringPolicy::Flash));
    }

    #[test]
    fn test_effective_policy_inherits_from_parent() {
        let mut cache = TieringHintCache::new(100);
        cache.insert(TieringHint::new(1, TieringPolicy::S3, true, 1000));
        cache.set_parent(2, 1);

        assert!(matches!(cache.effective_policy(2), TieringPolicy::S3));
    }

    #[test]
    fn test_effective_policy_explicit_beats_parent() {
        let mut cache = TieringHintCache::new(100);
        cache.insert(TieringHint::new(1, TieringPolicy::S3, true, 1000));
        cache.insert(TieringHint::new(2, TieringPolicy::Flash, false, 1000));
        cache.set_parent(2, 1);

        assert!(matches!(cache.effective_policy(2), TieringPolicy::Flash));
    }

    #[test]
    fn test_effective_policy_default_auto() {
        let cache = TieringHintCache::new(100);

        assert!(matches!(cache.effective_policy(999), TieringPolicy::Auto));
    }

    #[test]
    fn test_eviction_candidates_sorted() {
        let mut cache = TieringHintCache::new(100);

        cache.insert(TieringHint::new(1, TieringPolicy::Auto, false, 1000));
        cache.insert(TieringHint::new(2, TieringPolicy::Auto, false, 1000));
        cache.insert(TieringHint::new(3, TieringPolicy::Auto, false, 1000));

        let mut ages = HashMap::new();
        ages.insert(1, 10);
        ages.insert(2, 50);
        ages.insert(3, 100);

        let mut sizes = HashMap::new();
        sizes.insert(1, 1000);
        sizes.insert(2, 1000);
        sizes.insert(3, 1000);

        let candidates = cache.eviction_candidates(&ages, &sizes, 0);

        assert_eq!(candidates.len(), 3);
        assert!(candidates[0].1 >= candidates[1].1);
        assert!(candidates[1].1 >= candidates[2].1);
    }

    #[test]
    fn test_eviction_candidates_excludes_pinned() {
        let mut cache = TieringHintCache::new(100);

        cache.insert(TieringHint::new(1, TieringPolicy::Flash, false, 1000));

        let mut ages = HashMap::new();
        ages.insert(1, 100);

        let mut sizes = HashMap::new();
        sizes.insert(1, 1000);

        let candidates = cache.eviction_candidates(&ages, &sizes, 0);

        assert!(candidates.is_empty());
    }

    #[test]
    fn test_trim_respects_max() {
        let mut cache = TieringHintCache::new(2);

        cache.insert(TieringHint::new(1, TieringPolicy::Auto, false, 1000));
        cache.insert(TieringHint::new(2, TieringPolicy::Auto, false, 1000));
        assert_eq!(cache.len(), 2);

        cache.insert(TieringHint::new(3, TieringPolicy::Auto, false, 1000));
        assert!(cache.len() <= 2);
    }
}
