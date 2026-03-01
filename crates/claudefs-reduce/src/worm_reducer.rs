use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WormMode {
    None,
    Immutable,
    LegalHold,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RetentionPolicy {
    pub mode: WormMode,
    pub retain_until: Option<u64>,
}

impl RetentionPolicy {
    pub fn none() -> Self {
        Self {
            mode: WormMode::None,
            retain_until: None,
        }
    }

    pub fn immutable_until(ts: u64) -> Self {
        Self {
            mode: WormMode::Immutable,
            retain_until: Some(ts),
        }
    }

    pub fn legal_hold() -> Self {
        Self {
            mode: WormMode::LegalHold,
            retain_until: None,
        }
    }

    pub fn is_expired(&self, now_ts: u64) -> bool {
        match self.mode {
            WormMode::None => true,
            WormMode::LegalHold => false,
            _ => match self.retain_until {
                Some(ts) => now_ts > ts,
                None => false,
            },
        }
    }
}

pub struct WormReducer {
    records: HashMap<u64, (RetentionPolicy, u64)>,
}

impl WormReducer {
    pub fn new() -> Self {
        Self {
            records: HashMap::new(),
        }
    }

    pub fn register(&mut self, hash: u64, policy: RetentionPolicy, size: u64) {
        self.records.insert(hash, (policy, size));
    }

    pub fn get(&self, hash: &u64) -> Option<&(RetentionPolicy, u64)> {
        self.records.get(hash)
    }

    pub fn active_count(&self, now_ts: u64) -> usize {
        self.records
            .values()
            .filter(|(policy, _)| !policy.is_expired(now_ts))
            .count()
    }

    pub fn gc_expired(&mut self, now_ts: u64) -> usize {
        let expired: Vec<_> = self
            .records
            .iter()
            .filter(|(_, (policy, _))| policy.is_expired(now_ts))
            .map(|(hash, _)| *hash)
            .collect();

        for hash in &expired {
            self.records.remove(hash);
        }
        expired.len()
    }

    pub fn total_count(&self) -> usize {
        self.records.len()
    }
}

impl Default for WormReducer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_hash(n: u64) -> u64 {
        n
    }

    #[test]
    fn test_retention_none() {
        let policy = RetentionPolicy::none();
        assert!(policy.is_expired(0));
    }

    #[test]
    fn test_retention_immutable() {
        let policy = RetentionPolicy::immutable_until(500);
        assert!(!policy.is_expired(100));
        assert!(!policy.is_expired(500));
        assert!(policy.is_expired(501));
    }

    #[test]
    fn test_retention_legal_hold() {
        let policy = RetentionPolicy::legal_hold();
        assert!(!policy.is_expired(0));
        assert!(!policy.is_expired(u64::MAX));
    }

    #[test]
    fn test_active_count() {
        let mut reducer = WormReducer::new();

        reducer.register(make_hash(1), RetentionPolicy::none(), 0);
        reducer.register(make_hash(2), RetentionPolicy::immutable_until(500), 0);
        // Immutable active (expires after the assertion time 750)
        reducer.register(make_hash(3), RetentionPolicy::immutable_until(1000), 0);

        assert_eq!(reducer.active_count(750), 3);
    }

    #[test]
    fn test_active_records() {
        let mut reducer = WormReducer::new();

        reducer.register(make_hash(1), RetentionPolicy::none(), 0);
        reducer.register(make_hash(2), RetentionPolicy::immutable_until(500), 0);
        // 3 (still active at 750)
        reducer.register(make_hash(3), RetentionPolicy::immutable_until(1000), 0);

        assert_eq!(reducer.active_count(750), 3);
    }

    #[test]
    fn test_gc_expired() {
        let mut reducer = WormReducer::new();

        // No None-mode registration — testing time-based expiry only
        reducer.register(make_hash(2), RetentionPolicy::immutable_until(500), 0);
        reducer.register(make_hash(3), RetentionPolicy::immutable_until(1000), 0);
        reducer.register(make_hash(4), RetentionPolicy::legal_hold(), 0);

        let removed = reducer.gc_expired(600);
        assert_eq!(removed, 1);
        assert_eq!(reducer.total_count(), 2);

        // Verify hash 2 is removed
        assert!(reducer.get(&make_hash(2)).is_none());
        // Verify others remain
        assert!(reducer.get(&make_hash(3)).is_some());
        assert!(reducer.get(&make_hash(4)).is_some());
    }

    #[test]
    fn test_register_and_get() {
        let mut reducer = WormReducer::new();
        reducer.register(100, RetentionPolicy::legal_hold(), 1024);

        let result = reducer.get(&100);
        assert!(result.is_some());
        assert_eq!(result.unwrap().1, 1024);
    }

    #[test]
    fn test_total_count() {
        let mut reducer = WormReducer::new();
        assert_eq!(reducer.total_count(), 0);

        reducer.register(1, RetentionPolicy::none(), 0);
        reducer.register(2, RetentionPolicy::legal_hold(), 0);
        assert_eq!(reducer.total_count(), 2);
    }

    #[test]
    fn test_multiple_immutable_blocks() {
        let mut reducer = WormReducer::new();

        for i in 1..=5 {
            reducer.register(
                make_hash(i),
                RetentionPolicy::immutable_until(1000),
                i * 512,
            );
        }

        assert_eq!(reducer.active_count(500), 5);
        assert_eq!(reducer.active_count(1001), 0);
    }

    #[test]
    fn test_legal_hold_never_expires() {
        let mut reducer = WormReducer::new();

        reducer.register(1, RetentionPolicy::legal_hold(), 0);
        reducer.register(2, RetentionPolicy::immutable_until(100), 0);

        assert_eq!(reducer.active_count(200), 1);
    }

    #[test]
    fn test_gc_removes_all_expired() {
        let mut reducer = WormReducer::new();

        reducer.register(1, RetentionPolicy::immutable_until(100), 0);
        reducer.register(2, RetentionPolicy::immutable_until(200), 0);
        reducer.register(3, RetentionPolicy::immutable_until(300), 0);

        let removed = reducer.gc_expired(250);
        assert_eq!(removed, 2);
        assert_eq!(reducer.total_count(), 1);
        assert!(reducer.get(&3).is_some());
    }

    #[test]
    fn test_none_mode_not_counted_as_active() {
        let mut reducer = WormReducer::new();

        reducer.register(1, RetentionPolicy::none(), 0);
        reducer.register(2, RetentionPolicy::immutable_until(1000), 0);

        assert_eq!(reducer.active_count(500), 1);
    }

    #[test]
    fn test_expired_at_exact_timestamp() {
        let policy = RetentionPolicy::immutable_until(500);
        assert!(!policy.is_expired(500));
        assert!(policy.is_expired(501));
    }

    #[test]
    fn test_gc_empty() {
        let mut reducer = WormReducer::new();
        let removed = reducer.gc_expired(1000);
        assert_eq!(removed, 0);
    }

    #[test]
    fn test_active_count_empty() {
        let reducer = WormReducer::new();
        assert_eq!(reducer.active_count(1000), 0);
    }

    #[test]
    fn test_mixed_policies() {
        let mut reducer = WormReducer::new();

        reducer.register(1, RetentionPolicy::none(), 0);
        reducer.register(2, RetentionPolicy::legal_hold(), 0);
        reducer.register(3, RetentionPolicy::immutable_until(500), 0);
        reducer.register(4, RetentionPolicy::immutable_until(1000), 0);

        assert_eq!(reducer.active_count(600), 2);
        assert_eq!(reducer.active_count(1100), 1);
    }

    #[test]
    fn test_retain_until_none_immutable() {
        let policy = RetentionPolicy {
            mode: WormMode::Immutable,
            retain_until: None,
        };
        assert!(!policy.is_expired(0));
        assert!(!policy.is_expired(u64::MAX));
    }

    #[test]
    fn test_gc_legal_hold_preserved() {
        let mut reducer = WormReducer::new();

        reducer.register(1, RetentionPolicy::legal_hold(), 0);
        reducer.register(2, RetentionPolicy::immutable_until(100), 0);

        let removed = reducer.gc_expired(200);
        assert_eq!(removed, 1);
        assert_eq!(reducer.total_count(), 1);
    }

    #[test]
    fn test_concurrent_gc() {
        let mut reducer = WormReducer::new();

        for i in 1..=10 {
            reducer.register(i, RetentionPolicy::immutable_until(i * 100), 0);
        }

        let removed1 = reducer.gc_expired(500);
        assert_eq!(removed1, 5);

        let removed2 = reducer.gc_expired(1000);
        assert_eq!(removed2, 5);

        assert_eq!(reducer.total_count(), 0);
    }

    #[test]
    fn test_zero_timestamp() {
        let policy = RetentionPolicy::immutable_until(0);
        assert!(!policy.is_expired(0));
        assert!(policy.is_expired(1));
    }

    #[test]
    fn test_max_timestamp() {
        let policy = RetentionPolicy::immutable_until(u64::MAX);
        assert!(!policy.is_expired(u64::MAX - 1));
        assert!(!policy.is_expired(u64::MAX));
    }

    #[test]
    fn test_very_large_gc_timestamp() {
        let mut reducer = WormReducer::new();

        reducer.register(1, RetentionPolicy::immutable_until(1000), 0);
        reducer.register(2, RetentionPolicy::legal_hold(), 0);

        let removed = reducer.gc_expired(u64::MAX);
        assert_eq!(removed, 1);
    }

    #[test]
    fn test_register_overwrites() {
        let mut reducer = WormReducer::new();

        reducer.register(1, RetentionPolicy::none(), 100);
        reducer.register(1, RetentionPolicy::legal_hold(), 200);

        let (policy, size) = reducer.get(&1).unwrap();
        assert!(matches!(policy.mode, WormMode::LegalHold));
        assert_eq!(*size, 200);
    }

    #[test]
    fn test_different_hash_sizes() {
        let mut reducer = WormReducer::new();

        reducer.register(1, RetentionPolicy::legal_hold(), 100);
        reducer.register(1000000, RetentionPolicy::legal_hold(), 200);
        reducer.register(u64::MAX, RetentionPolicy::legal_hold(), 300);

        assert_eq!(reducer.active_count(0), 3);
    }

    #[test]
    fn test_is_expired_edge_cases() {
        let none_policy = RetentionPolicy::none();
        assert!(none_policy.is_expired(0));
        assert!(none_policy.is_expired(u64::MAX));

        let immutable_policy = RetentionPolicy::immutable_until(100);
        assert!(!immutable_policy.is_expired(50));
        assert!(!immutable_policy.is_expired(100));
        assert!(immutable_policy.is_expired(101));

        let legal_hold_policy = RetentionPolicy::legal_hold();
        assert!(!legal_hold_policy.is_expired(0));
        assert!(!legal_hold_policy.is_expired(u64::MAX));
    }

    #[test]
    fn test_active_count_partial_expiry() {
        let mut reducer = WormReducer::new();

        for i in 1..=10 {
            let ts = i * 100;
            reducer.register(i, RetentionPolicy::immutable_until(ts), 0);
        }

        for check_ts in [0, 100, 250, 500, 750, 1000, 1500] {
            let active = reducer.active_count(check_ts);
            let expected = (1..=10).filter(|&i| i * 100 > check_ts).count();
            assert_eq!(active, expected, "at timestamp {}", check_ts);
        }
    }

    #[test]
    fn test_gc_idempotent() {
        let mut reducer = WormReducer::new();

        reducer.register(1, RetentionPolicy::immutable_until(100), 0);
        reducer.register(2, RetentionPolicy::immutable_until(200), 0);

        let first = reducer.gc_expired(150);
        assert_eq!(first, 1);

        let second = reducer.gc_expired(150);
        assert_eq!(second, 0);

        let third = reducer.gc_expired(250);
        assert_eq!(third, 1);
    }

    #[test]
    fn test_reducer_default() {
        let reducer: WormReducer = Default::default();
        assert_eq!(reducer.total_count(), 0);
        assert_eq!(reducer.active_count(0), 0);
    }

    #[test]
    fn test_policy_clone() {
        let policy = RetentionPolicy::legal_hold();
        let cloned = policy.clone();
        assert!(!cloned.is_expired(0));
    }

    #[test]
    fn test_worm_mode_variants() {
        assert!(matches!(RetentionPolicy::none().mode, WormMode::None));
        assert!(matches!(
            RetentionPolicy::immutable_until(100).mode,
            WormMode::Immutable
        ));
        assert!(matches!(
            RetentionPolicy::legal_hold().mode,
            WormMode::LegalHold
        ));
    }

    #[test]
    fn test_retain_until_values() {
        let p1 = RetentionPolicy::none();
        assert_eq!(p1.retain_until, None);

        let p2 = RetentionPolicy::immutable_until(500);
        assert_eq!(p2.retain_until, Some(500));

        let p3 = RetentionPolicy::legal_hold();
        assert_eq!(p3.retain_until, None);
    }

    #[test]
    fn test_gc_with_only_none_mode() {
        let mut reducer = WormReducer::new();
        reducer.register(1, RetentionPolicy::none(), 0);
        reducer.register(2, RetentionPolicy::none(), 0);

        let removed = reducer.gc_expired(1000);
        assert_eq!(removed, 2);
        assert_eq!(reducer.total_count(), 0);
    }

    #[test]
    fn test_gc_with_only_legal_hold() {
        let mut reducer = WormReducer::new();
        reducer.register(1, RetentionPolicy::legal_hold(), 0);
        reducer.register(2, RetentionPolicy::legal_hold(), 0);

        let removed = reducer.gc_expired(u64::MAX);
        assert_eq!(removed, 0);
        assert_eq!(reducer.total_count(), 2);
    }

    #[test]
    fn test_large_number_of_records() {
        let mut reducer = WormReducer::new();

        for i in 0..1000 {
            let policy = if i % 3 == 0 {
                RetentionPolicy::none()
            } else if i % 3 == 1 {
                RetentionPolicy::legal_hold()
            } else {
                RetentionPolicy::immutable_until(i as u64)
            };
            reducer.register(i, policy, i as u64);
        }

        assert_eq!(reducer.active_count(500), 500);
    }

    #[test]
    fn test_empty_hash_space() {
        let reducer = WormReducer::new();
        assert!(reducer.get(&999).is_none());
    }

    #[test]
    fn test_policy_equality() {
        let p1 = RetentionPolicy::immutable_until(100);
        let p2 = RetentionPolicy::immutable_until(100);
        let p3 = RetentionPolicy::immutable_until(200);

        assert_eq!(p1.mode, p2.mode);
        assert_eq!(p1.retain_until, p2.retain_until);
        assert_ne!(p1.retain_until, p3.retain_until);
    }
}
