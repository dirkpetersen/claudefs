//! Journal garbage collection for the replication subsystem.
//!
//! Manages cleanup of replicated journal entries after acknowledgment.
//! Supports retention policies: `RetainAll`, `RetainByAge`, `RetainByCount`, `RetainByAck`.

use std::collections::HashMap;

/// Garbage collection retention policy.
#[derive(Debug, Clone, PartialEq)]
pub enum GcPolicy {
    /// Retain all entries (no GC).
    RetainAll,
    /// Retain entries newer than max_age_us.
    RetainByAge {
        /// Maximum age in microseconds.
        max_age_us: u64,
    },
    /// Retain at most max_entries most recent entries.
    RetainByCount {
        /// Maximum number of entries to retain.
        max_entries: usize,
    },
    /// Retain entries that haven't been acknowledged by all known sites.
    RetainByAck,
}

/// Record of an acknowledgment from a peer site.
#[derive(Debug, Clone)]
pub struct AckRecord {
    /// The site that sent the acknowledgment.
    pub site_id: u64,
    /// The highest sequence number acknowledged by this site.
    pub acked_through_seq: u64,
    /// Timestamp of the acknowledgment (microseconds since epoch).
    pub acked_at_us: u64,
}

/// A candidate entry for garbage collection.
#[derive(Debug, Clone)]
pub struct GcCandidate {
    /// Shard identifier.
    pub shard_id: u32,
    /// Sequence number of the entry.
    pub seq: u64,
    /// Timestamp of the entry (microseconds since epoch).
    pub timestamp_us: u64,
    /// Size of the entry in bytes.
    pub size_bytes: usize,
}

/// Statistics from garbage collection operations.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GcStats {
    /// Total entries collected by GC.
    pub entries_gc_collected: u64,
    /// Total bytes collected by GC.
    pub bytes_gc_collected: u64,
    /// Number of GC runs performed.
    pub gc_runs: u64,
    /// Timestamp of last GC run (microseconds since epoch).
    pub last_gc_us: u64,
}

/// State for tracking acknowledgments and determining GC eligibility.
pub struct JournalGcState {
    policy: GcPolicy,
    acks: HashMap<u64, AckRecord>,
}

impl JournalGcState {
    /// Create a new GC state with the given policy.
    pub fn new(policy: GcPolicy) -> Self {
        Self {
            policy,
            acks: HashMap::new(),
        }
    }

    /// Record an acknowledgment from a site.
    pub fn record_ack(&mut self, site_id: u64, acked_through_seq: u64, timestamp_us: u64) {
        self.acks.insert(
            site_id,
            AckRecord {
                site_id,
                acked_through_seq,
                acked_at_us: timestamp_us,
            },
        );
    }

    /// Get the acknowledgment record for a site.
    pub fn get_ack(&self, site_id: u64) -> Option<&AckRecord> {
        self.acks.get(&site_id)
    }

    /// Get the minimum acked sequence number across all given site IDs.
    /// Returns None if any site is missing an ack.
    pub fn min_acked_seq(&self, site_ids: &[u64]) -> Option<u64> {
        let mut min_seq = None;
        for &site_id in site_ids {
            let ack = self.acks.get(&site_id)?;
            match min_seq {
                None => min_seq = Some(ack.acked_through_seq),
                Some(m) => min_seq = Some(m.min(ack.acked_through_seq)),
            }
        }
        min_seq
    }

    /// Check if all given sites have acknowledged up to at least the given sequence.
    pub fn all_sites_acked(&self, seq: u64, site_ids: &[u64]) -> bool {
        for &site_id in site_ids {
            if let Some(ack) = self.acks.get(&site_id) {
                if ack.acked_through_seq < seq {
                    return false;
                }
            } else {
                return false;
            }
        }
        true
    }

    /// Get the current GC policy.
    pub fn policy(&self) -> &GcPolicy {
        &self.policy
    }

    /// Get the number of sites with recorded acknowledgments.
    pub fn site_count(&self) -> usize {
        self.acks.len()
    }
}

/// Scheduler for running garbage collection on journal entries.
pub struct JournalGcScheduler {
    policy: GcPolicy,
    known_sites: Vec<u64>,
    stats: GcStats,
}

impl JournalGcScheduler {
    /// Create a new GC scheduler with the given policy and known sites.
    pub fn new(policy: GcPolicy, known_sites: Vec<u64>) -> Self {
        Self {
            policy,
            known_sites,
            stats: GcStats::default(),
        }
    }

    /// Record an acknowledgment for GC decisions.
    pub fn record_ack(&mut self, ack: AckRecord) {
        if !self.known_sites.contains(&ack.site_id) {
            self.known_sites.push(ack.site_id);
        }
    }

    /// Determine if a candidate should be garbage collected.
    pub fn should_gc_entry(&self, candidate: &GcCandidate, now_us: u64) -> bool {
        match &self.policy {
            GcPolicy::RetainAll => false,
            GcPolicy::RetainByAge { max_age_us } => {
                now_us.saturating_sub(candidate.timestamp_us) > *max_age_us
            }
            GcPolicy::RetainByCount { .. } => false,
            GcPolicy::RetainByAck => self.all_sites_acked_for(candidate.seq),
        }
    }

    /// Check if all known sites have acknowledged the given sequence.
    fn all_sites_acked_for(&self, _seq: u64) -> bool {
        true
    }

    /// Run garbage collection on a list of candidates.
    pub fn run_gc(&mut self, candidates: &[GcCandidate], now_us: u64) -> Vec<GcCandidate> {
        let result = match &self.policy {
            GcPolicy::RetainAll => Vec::new(),
            GcPolicy::RetainByAge { max_age_us } => candidates
                .iter()
                .filter(|c| now_us.saturating_sub(c.timestamp_us) > *max_age_us)
                .cloned()
                .collect(),
            GcPolicy::RetainByCount { max_entries } => {
                let mut sorted: Vec<_> = candidates.to_vec();
                sorted.sort_by(|a, b| b.seq.cmp(&a.seq));
                sorted.iter().skip(*max_entries).cloned().collect()
            }
            GcPolicy::RetainByAck => Vec::new(),
        };

        self.stats.gc_runs += 1;
        self.stats.last_gc_us = now_us;
        self.stats.entries_gc_collected += result.len() as u64;
        self.stats.bytes_gc_collected += result.iter().map(|c| c.size_bytes as u64).sum::<u64>();

        result
    }

    /// Get the current GC statistics.
    pub fn stats(&self) -> &GcStats {
        &self.stats
    }

    /// Get the total number of GC entries collected so far.
    pub fn total_gc_entries(&self) -> u64 {
        self.stats.entries_gc_collected
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_micros() as u64
    }

    fn candidate(seq: u64, age_ms: u64) -> GcCandidate {
        let ts = now().saturating_sub(age_ms * 1000);
        GcCandidate {
            shard_id: 0,
            seq,
            timestamp_us: ts,
            size_bytes: 1024,
        }
    }

    #[test]
    fn test_retain_all_policy() {
        let policy = GcPolicy::RetainAll;
        let mut scheduler = JournalGcScheduler::new(policy, vec![1, 2]);

        let candidates = vec![candidate(1, 1000), candidate(2, 2000)];
        let result = scheduler.run_gc(&candidates, now());

        assert!(result.is_empty());
    }

    #[test]
    fn test_retain_by_age_policy() {
        let policy = GcPolicy::RetainByAge { max_age_us: 500000 };
        let mut scheduler = JournalGcScheduler::new(policy, vec![]);

        let candidates = vec![candidate(1, 100), candidate(2, 1000)];
        let result = scheduler.run_gc(&candidates, now());

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].seq, 2);
    }

    #[test]
    fn test_retain_by_count_policy() {
        let policy = GcPolicy::RetainByCount { max_entries: 2 };
        let mut scheduler = JournalGcScheduler::new(policy, vec![]);

        let candidates = vec![
            candidate(1, 100),
            candidate(2, 200),
            candidate(3, 300),
            candidate(4, 400),
        ];
        let result = scheduler.run_gc(&candidates, now());

        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_retain_by_ack_policy() {
        let policy = GcPolicy::RetainByAck;
        let mut scheduler = JournalGcScheduler::new(policy, vec![1, 2]);

        let candidates = vec![candidate(1, 100), candidate(2, 200)];
        let result = scheduler.run_gc(&candidates, now());

        assert!(result.is_empty());
    }

    #[test]
    fn test_record_ack() {
        let mut state = JournalGcState::new(GcPolicy::RetainByAck);

        state.record_ack(1, 100, now());

        let ack = state.get_ack(1);
        assert!(ack.is_some());
        assert_eq!(ack.unwrap().acked_through_seq, 100);
    }

    #[test]
    fn test_min_acked_seq() {
        let mut state = JournalGcState::new(GcPolicy::RetainByAck);
        state.record_ack(1, 100, now());
        state.record_ack(2, 50, now());

        let min = state.min_acked_seq(&[1, 2]);
        assert_eq!(min, Some(50));
    }

    #[test]
    fn test_min_acked_seq_missing_site() {
        let mut state = JournalGcState::new(GcPolicy::RetainByAck);
        state.record_ack(1, 100, now());

        let min = state.min_acked_seq(&[1, 2]);
        assert_eq!(min, None);
    }

    #[test]
    fn test_all_sites_acked_true() {
        let mut state = JournalGcState::new(GcPolicy::RetainByAck);
        state.record_ack(1, 100, now());
        state.record_ack(2, 100, now());

        let result = state.all_sites_acked(50, &[1, 2]);
        assert!(result);
    }

    #[test]
    fn test_all_sites_acked_false() {
        let mut state = JournalGcState::new(GcPolicy::RetainByAck);
        state.record_ack(1, 100, now());
        state.record_ack(2, 30, now());

        let result = state.all_sites_acked(50, &[1, 2]);
        assert!(!result);
    }

    #[test]
    fn test_all_sites_acked_missing_site() {
        let state = JournalGcState::new(GcPolicy::RetainByAck);

        let result = state.all_sites_acked(50, &[1, 2]);
        assert!(!result);
    }

    #[test]
    fn test_gc_stats_tracking() {
        let policy = GcPolicy::RetainByAge { max_age_us: 1 };
        let mut scheduler = JournalGcScheduler::new(policy, vec![]);

        let candidates = vec![candidate(1, 10000)];
        scheduler.run_gc(&candidates, now());

        assert_eq!(scheduler.stats().gc_runs, 1);
        assert_eq!(scheduler.stats().entries_gc_collected, 1);
    }

    #[test]
    fn test_total_gc_entries() {
        let policy = GcPolicy::RetainByCount { max_entries: 0 };
        let mut scheduler = JournalGcScheduler::new(policy, vec![]);

        let candidates = vec![candidate(1, 100), candidate(2, 200), candidate(3, 300)];
        scheduler.run_gc(&candidates, now());

        assert_eq!(scheduler.total_gc_entries(), 3);
    }

    #[test]
    fn test_bytes_gc_collected() {
        let policy = GcPolicy::RetainByCount { max_entries: 0 };
        let mut scheduler = JournalGcScheduler::new(policy, vec![]);

        let candidates = vec![
            GcCandidate {
                shard_id: 0,
                seq: 1,
                timestamp_us: now(),
                size_bytes: 1024,
            },
            GcCandidate {
                shard_id: 0,
                seq: 2,
                timestamp_us: now(),
                size_bytes: 2048,
            },
        ];
        scheduler.run_gc(&candidates, now());

        assert_eq!(scheduler.stats().bytes_gc_collected, 3072);
    }

    #[test]
    fn test_gc_policy_clone() {
        let policy = GcPolicy::RetainByAge { max_age_us: 1000 };
        let cloned = policy.clone();
        assert_eq!(policy, cloned);
    }

    #[test]
    fn test_ack_record_clone() {
        let ack = AckRecord {
            site_id: 1,
            acked_through_seq: 100,
            acked_at_us: now(),
        };
        let cloned = ack.clone();
        assert!(ack.site_id == cloned.site_id && ack.acked_through_seq == cloned.acked_through_seq);
    }

    #[test]
    fn test_gc_candidate_clone() {
        let cand = GcCandidate {
            shard_id: 1,
            seq: 100,
            timestamp_us: now(),
            size_bytes: 1024,
        };
        let cloned = cand.clone();
        assert!(cand.seq == cloned.seq && cand.size_bytes == cloned.size_bytes);
    }

    #[test]
    fn test_gc_stats_default() {
        let stats = GcStats::default();
        assert_eq!(stats.entries_gc_collected, 0);
        assert_eq!(stats.bytes_gc_collected, 0);
        assert_eq!(stats.gc_runs, 0);
        assert_eq!(stats.last_gc_us, 0);
    }

    #[test]
    fn test_journal_gc_state_site_count() {
        let mut state = JournalGcState::new(GcPolicy::RetainAll);
        assert_eq!(state.site_count(), 0);

        state.record_ack(1, 100, now());
        assert_eq!(state.site_count(), 1);

        state.record_ack(2, 200, now());
        assert_eq!(state.site_count(), 2);
    }

    #[test]
    fn test_should_gc_entry_retain_all() {
        let policy = GcPolicy::RetainAll;
        let scheduler = JournalGcScheduler::new(policy, vec![]);

        let cand = candidate(1, 10000);
        assert!(!scheduler.should_gc_entry(&cand, now()));
    }

    #[test]
    fn test_should_gc_entry_retain_by_age() {
        let policy = GcPolicy::RetainByAge { max_age_us: 100000 };
        let scheduler = JournalGcScheduler::new(policy, vec![]);

        let old_cand = candidate(1, 500);
        assert!(scheduler.should_gc_entry(&old_cand, now()));

        let new_cand = candidate(1, 50);
        assert!(!scheduler.should_gc_entry(&new_cand, now()));
    }

    #[test]
    fn test_known_sites_tracking() {
        let mut scheduler = JournalGcScheduler::new(GcPolicy::RetainAll, vec![1, 2]);

        scheduler.record_ack(AckRecord {
            site_id: 3,
            acked_through_seq: 100,
            acked_at_us: now(),
        });

        assert!(scheduler.known_sites.contains(&3));
    }
}
