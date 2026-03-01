use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use tracing::{error, info, warn};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct FencingToken(pub u64);

impl FencingToken {
    pub fn new(v: u64) -> Self {
        Self(v)
    }

    pub fn next(&self) -> Self {
        Self(self.0 + 1)
    }

    pub fn value(&self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SplitBrainState {
    Normal,
    PartitionSuspected {
        since_ns: u64,
        site_id: u64,
    },
    Confirmed {
        site_a: u64,
        site_b: u64,
        diverged_at_seq: u64,
    },
    Resolving {
        fenced_site: u64,
        active_site: u64,
        fence_token: FencingToken,
    },
    Healed {
        at_ns: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SplitBrainEvidence {
    pub site_a_last_seq: u64,
    pub site_b_last_seq: u64,
    pub site_a_diverge_seq: u64,
    pub detected_at_ns: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SplitBrainStats {
    pub partitions_detected: u64,
    pub split_brains_confirmed: u64,
    pub resolutions_completed: u64,
    pub fencing_tokens_issued: u64,
}

pub struct SplitBrainDetector {
    local_site: u64,
    current_state: SplitBrainState,
    current_fence_token: FencingToken,
    stats: SplitBrainStats,
}

static INITIAL_TOKEN: OnceLock<FencingToken> = OnceLock::new();

fn initial_fence_token() -> FencingToken {
    *INITIAL_TOKEN.get_or_init(|| FencingToken(1))
}

impl SplitBrainDetector {
    pub fn new(local_site: u64) -> Self {
        Self {
            local_site,
            current_state: SplitBrainState::Normal,
            current_fence_token: initial_fence_token(),
            stats: SplitBrainStats::default(),
        }
    }

    pub fn report_partition(&mut self, remote_site: u64, at_ns: u64) -> SplitBrainState {
        self.stats.partitions_detected += 1;
        self.current_state = SplitBrainState::PartitionSuspected {
            since_ns: at_ns,
            site_id: remote_site,
        };
        info!(
            "Partition suspected: local_site={}, remote_site={}, at_ns={}",
            self.local_site, remote_site, at_ns
        );
        self.current_state.clone()
    }

    pub fn confirm_split_brain(
        &mut self,
        evidence: SplitBrainEvidence,
        site_a: u64,
        site_b: u64,
    ) -> SplitBrainState {
        if !matches!(
            self.current_state,
            SplitBrainState::PartitionSuspected { .. }
        ) {
            warn!(
                "Cannot confirm split brain from state: {:?}",
                self.current_state
            );
            return self.current_state.clone();
        }

        self.stats.split_brains_confirmed += 1;
        self.current_state = SplitBrainState::Confirmed {
            site_a,
            site_b,
            diverged_at_seq: evidence.site_a_diverge_seq,
        };
        info!(
            "Split brain confirmed: site_a={}, site_b={}, diverged_at_seq={}",
            site_a, site_b, evidence.site_a_diverge_seq
        );
        self.current_state.clone()
    }

    pub fn issue_fence(&mut self, site_to_fence: u64, active_site: u64) -> FencingToken {
        if !matches!(self.current_state, SplitBrainState::Confirmed { .. }) {
            warn!("Cannot issue fence from state: {:?}", self.current_state);
            return self.current_fence_token;
        }

        self.current_fence_token = self.current_fence_token.next();
        self.stats.fencing_tokens_issued += 1;

        let token = self.current_fence_token;
        self.current_state = SplitBrainState::Resolving {
            fenced_site: site_to_fence,
            active_site,
            fence_token: token,
        };

        info!(
            "Fence issued: site_to_fence={}, active_site={}, token={}",
            site_to_fence, active_site, token.0
        );
        token
    }

    pub fn validate_token(&self, token: FencingToken) -> bool {
        token.0 >= self.current_fence_token.0
    }

    pub fn mark_healed(&mut self, at_ns: u64) -> SplitBrainState {
        match &self.current_state {
            SplitBrainState::Resolving { .. } => {
                self.stats.resolutions_completed += 1;
                self.current_state = SplitBrainState::Healed { at_ns };
                info!("Split brain healing initiated at ns={}", at_ns);
            }
            SplitBrainState::Healed { .. } => {
                self.current_state = SplitBrainState::Normal;
                info!("Split brain fully healed, returned to normal state");
            }
            _ => {
                warn!("Cannot heal from state: {:?}", self.current_state);
            }
        }
        self.current_state.clone()
    }

    pub fn state(&self) -> &SplitBrainState {
        &self.current_state
    }

    pub fn current_token(&self) -> FencingToken {
        self.current_fence_token
    }

    pub fn stats(&self) -> &SplitBrainStats {
        &self.stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fencing_token_new_and_value() {
        let token = FencingToken::new(42);
        assert_eq!(token.value(), 42);
    }

    #[test]
    fn test_fencing_token_next() {
        let token = FencingToken::new(5);
        let next = token.next();
        assert_eq!(next.value(), 6);
        assert_eq!(token.value(), 5);
    }

    #[test]
    fn test_fencing_token_ord() {
        let t1 = FencingToken::new(1);
        let t2 = FencingToken::new(2);
        let t3 = FencingToken::new(3);
        assert!(t1 < t2);
        assert!(t2 < t3);
        assert!(t1 < t3);
    }

    #[test]
    fn test_detector_initial_state() {
        let detector = SplitBrainDetector::new(1);
        assert!(matches!(detector.state(), SplitBrainState::Normal));
        assert_eq!(detector.current_token().value(), 1);
    }

    #[test]
    fn test_detector_initial_token_is_one() {
        let detector1 = SplitBrainDetector::new(1);
        let detector2 = SplitBrainDetector::new(2);
        assert_eq!(detector1.current_token().value(), 1);
        assert_eq!(detector2.current_token().value(), 1);
    }

    #[test]
    fn test_report_partition_from_normal() {
        let mut detector = SplitBrainDetector::new(1);
        let state = detector.report_partition(2, 1000);

        assert!(matches!(
            state,
            SplitBrainState::PartitionSuspected {
                since_ns: 1000,
                site_id: 2
            }
        ));
        assert!(matches!(
            detector.state(),
            SplitBrainState::PartitionSuspected { .. }
        ));
        assert_eq!(detector.stats().partitions_detected, 1);
    }

    #[test]
    fn test_report_partition_increments_counter() {
        let mut detector = SplitBrainDetector::new(1);
        detector.report_partition(2, 1000);
        detector.report_partition(3, 2000);
        assert_eq!(detector.stats().partitions_detected, 2);
    }

    #[test]
    fn test_confirm_split_brain_requires_partition_state() {
        let mut detector = SplitBrainDetector::new(1);
        let evidence = SplitBrainEvidence {
            site_a_last_seq: 100,
            site_b_last_seq: 99,
            site_a_diverge_seq: 50,
            detected_at_ns: 1000,
        };

        let state = detector.confirm_split_brain(evidence, 1, 2);

        assert!(matches!(state, SplitBrainState::Normal));
        assert_eq!(detector.stats().split_brains_confirmed, 0);
    }

    #[test]
    fn test_confirm_split_brain_from_partition_state() {
        let mut detector = SplitBrainDetector::new(1);
        detector.report_partition(2, 1000);

        let evidence = SplitBrainEvidence {
            site_a_last_seq: 100,
            site_b_last_seq: 99,
            site_a_diverge_seq: 50,
            detected_at_ns: 1000,
        };

        let state = detector.confirm_split_brain(evidence, 1, 2);

        assert!(matches!(
            state,
            SplitBrainState::Confirmed {
                site_a: 1,
                site_b: 2,
                diverged_at_seq: 50
            }
        ));
        assert_eq!(detector.stats().split_brains_confirmed, 1);
    }

    #[test]
    fn test_issue_fence_requires_confirmed_state() {
        let mut detector = SplitBrainDetector::new(1);
        let token = detector.issue_fence(2, 1);

        assert_eq!(token.value(), 1);
        assert_eq!(detector.stats().fencing_tokens_issued, 0);
    }

    #[test]
    fn test_issue_fence_from_confirmed_state() {
        let mut detector = SplitBrainDetector::new(1);
        detector.report_partition(2, 1000);

        let evidence = SplitBrainEvidence {
            site_a_last_seq: 100,
            site_b_last_seq: 99,
            site_a_diverge_seq: 50,
            detected_at_ns: 1000,
        };
        detector.confirm_split_brain(evidence, 1, 2);

        let token = detector.issue_fence(2, 1);

        assert_eq!(token.value(), 2);
        assert!(matches!(
            detector.state(),
            SplitBrainState::Resolving {
                fenced_site: 2,
                active_site: 1,
                fence_token: _
            }
        ));
        assert_eq!(detector.stats().fencing_tokens_issued, 1);
    }

    #[test]
    fn test_issue_fence_increments_token() {
        let mut detector = SplitBrainDetector::new(1);

        detector.report_partition(2, 1000);
        let evidence = SplitBrainEvidence {
            site_a_last_seq: 100,
            site_b_last_seq: 99,
            site_a_diverge_seq: 50,
            detected_at_ns: 1000,
        };
        detector.confirm_split_brain(evidence, 1, 2);

        let token1 = detector.issue_fence(2, 1);
        assert_eq!(token1.value(), 2);

        detector.report_partition(3, 2000);
        let evidence2 = SplitBrainEvidence {
            site_a_last_seq: 200,
            site_b_last_seq: 199,
            site_a_diverge_seq: 150,
            detected_at_ns: 2000,
        };
        detector.confirm_split_brain(evidence2, 1, 3);

        let token2 = detector.issue_fence(3, 1);
        assert_eq!(token2.value(), token1.value() + 1);
    }

    #[test]
    fn test_validate_token_returns_true_for_valid_token() {
        let mut detector = SplitBrainDetector::new(1);
        detector.report_partition(2, 1000);

        let evidence = SplitBrainEvidence {
            site_a_last_seq: 100,
            site_b_last_seq: 99,
            site_a_diverge_seq: 50,
            detected_at_ns: 1000,
        };
        detector.confirm_split_brain(evidence, 1, 2);
        let token = detector.issue_fence(2, 1);

        assert!(detector.validate_token(token));
    }

    #[test]
    fn test_validate_token_returns_true_for_higher_token() {
        let detector = SplitBrainDetector::new(1);
        let token = FencingToken::new(100);
        assert!(detector.validate_token(token));
    }

    #[test]
    fn test_validate_token_returns_false_for_lower_token() {
        let mut detector = SplitBrainDetector::new(1);
        detector.report_partition(2, 1000);

        let evidence = SplitBrainEvidence {
            site_a_last_seq: 100,
            site_b_last_seq: 99,
            site_a_diverge_seq: 50,
            detected_at_ns: 1000,
        };
        detector.confirm_split_brain(evidence, 1, 2);
        detector.issue_fence(2, 1);

        let old_token = FencingToken::new(1);
        assert!(!detector.validate_token(old_token));
    }

    #[test]
    fn test_mark_healed_from_resolving_state() {
        let mut detector = SplitBrainDetector::new(1);
        detector.report_partition(2, 1000);

        let evidence = SplitBrainEvidence {
            site_a_last_seq: 100,
            site_b_last_seq: 99,
            site_a_diverge_seq: 50,
            detected_at_ns: 1000,
        };
        detector.confirm_split_brain(evidence, 1, 2);
        detector.issue_fence(2, 1);

        let state = detector.mark_healed(5000);

        assert!(matches!(state, SplitBrainState::Healed { at_ns: 5000 }));
        assert_eq!(detector.stats().resolutions_completed, 1);
    }

    #[test]
    fn test_mark_healed_from_healed_returns_to_normal() {
        let mut detector = SplitBrainDetector::new(1);
        detector.report_partition(2, 1000);

        let evidence = SplitBrainEvidence {
            site_a_last_seq: 100,
            site_b_last_seq: 99,
            site_a_diverge_seq: 50,
            detected_at_ns: 1000,
        };
        detector.confirm_split_brain(evidence, 1, 2);
        detector.issue_fence(2, 1);
        detector.mark_healed(5000);

        let state = detector.mark_healed(6000);

        assert!(matches!(state, SplitBrainState::Normal));
    }

    #[test]
    fn test_mark_healed_from_normal_does_nothing() {
        let mut detector = SplitBrainDetector::new(1);
        let state = detector.mark_healed(5000);

        assert!(matches!(state, SplitBrainState::Normal));
        assert_eq!(detector.stats().resolutions_completed, 0);
    }

    #[test]
    fn test_full_split_brain_lifecycle() {
        let mut detector = SplitBrainDetector::new(1);

        assert!(matches!(detector.state(), SplitBrainState::Normal));

        detector.report_partition(2, 1000);
        assert!(matches!(
            detector.state(),
            SplitBrainState::PartitionSuspected { .. }
        ));

        let evidence = SplitBrainEvidence {
            site_a_last_seq: 100,
            site_b_last_seq: 99,
            site_a_diverge_seq: 50,
            detected_at_ns: 1000,
        };
        detector.confirm_split_brain(evidence, 1, 2);
        assert!(matches!(
            detector.state(),
            SplitBrainState::Confirmed { .. }
        ));

        let token = detector.issue_fence(2, 1);
        assert!(matches!(
            detector.state(),
            SplitBrainState::Resolving { .. }
        ));
        assert!(detector.validate_token(token));

        detector.mark_healed(5000);
        assert!(matches!(detector.state(), SplitBrainState::Healed { .. }));

        detector.mark_healed(6000);
        assert!(matches!(detector.state(), SplitBrainState::Normal));

        assert_eq!(detector.stats().partitions_detected, 1);
        assert_eq!(detector.stats().split_brains_confirmed, 1);
        assert_eq!(detector.stats().fencing_tokens_issued, 1);
        assert_eq!(detector.stats().resolutions_completed, 1);
    }

    #[test]
    fn test_state_returns_current_state() {
        let detector = SplitBrainDetector::new(1);
        let state = detector.state();
        assert!(matches!(state, SplitBrainState::Normal));
    }

    #[test]
    fn test_current_token_returns_current_fence_token() {
        let detector = SplitBrainDetector::new(1);
        assert_eq!(detector.current_token().value(), 1);
    }

    #[test]
    fn test_stats_returns_stats_reference() {
        let mut detector = SplitBrainDetector::new(1);
        detector.report_partition(2, 1000);

        let stats = detector.stats();
        assert_eq!(stats.partitions_detected, 1);
    }

    #[test]
    fn test_split_brain_evidence_fields() {
        let evidence = SplitBrainEvidence {
            site_a_last_seq: 100,
            site_b_last_seq: 200,
            site_a_diverge_seq: 50,
            detected_at_ns: 1000,
        };

        assert_eq!(evidence.site_a_last_seq, 100);
        assert_eq!(evidence.site_b_last_seq, 200);
        assert_eq!(evidence.site_a_diverge_seq, 50);
        assert_eq!(evidence.detected_at_ns, 1000);
    }
}
