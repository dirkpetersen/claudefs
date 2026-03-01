use serde::{Deserialize, Serialize};
use tracing::{info, warn};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum BootstrapPhase {
    Idle,
    Enrolling {
        primary_site: u64,
    },
    SnapshotTransfer {
        primary_site: u64,
        bytes_received: u64,
        bytes_total: u64,
    },
    JournalCatchup {
        primary_site: u64,
        start_seq: u64,
        current_seq: u64,
        target_seq: u64,
    },
    Complete {
        enrolled_at_ns: u64,
    },
    Failed {
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrollmentRecord {
    pub site_id: u64,
    pub site_name: String,
    pub enrolled_at_ns: u64,
    pub initial_seq: u64,
    pub tls_fingerprint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootstrapProgress {
    pub phase: BootstrapPhase,
    pub percent_complete: u8,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BootstrapStats {
    pub bootstrap_attempts: u64,
    pub bootstrap_successes: u64,
    pub bootstrap_failures: u64,
    pub total_bytes_transferred: u64,
    pub total_journal_entries_caught_up: u64,
}

pub struct BootstrapCoordinator {
    local_site_id: u64,
    phase: BootstrapPhase,
    started_at_ns: u64,
    stats: BootstrapStats,
    enrollment: Option<EnrollmentRecord>,
}

impl BootstrapCoordinator {
    pub fn new(local_site_id: u64) -> Self {
        Self {
            local_site_id,
            phase: BootstrapPhase::Idle,
            started_at_ns: 0,
            stats: BootstrapStats::default(),
            enrollment: None,
        }
    }

    pub fn start_enroll(&mut self, primary_site: u64, started_at_ns: u64) {
        self.phase = BootstrapPhase::Enrolling { primary_site };
        self.started_at_ns = started_at_ns;
        self.stats.bootstrap_attempts += 1;
        self.enrollment = Some(EnrollmentRecord {
            site_id: primary_site,
            site_name: String::new(),
            enrolled_at_ns: 0,
            initial_seq: 0,
            tls_fingerprint: None,
        });
    }

    pub fn begin_snapshot(&mut self, primary_site: u64, bytes_total: u64) {
        self.phase = BootstrapPhase::SnapshotTransfer {
            primary_site,
            bytes_received: 0,
            bytes_total,
        };
    }

    pub fn update_snapshot_progress(&mut self, bytes_received: u64) {
        if let BootstrapPhase::SnapshotTransfer {
            bytes_received: old_received,
            bytes_total,
            ..
        } = &mut self.phase
        {
            let delta = bytes_received.saturating_sub(*old_received);
            *old_received = bytes_received;
            self.stats.total_bytes_transferred += delta;
        }
    }

    pub fn begin_journal_catchup(&mut self, primary_site: u64, start_seq: u64, target_seq: u64) {
        self.phase = BootstrapPhase::JournalCatchup {
            primary_site,
            start_seq,
            current_seq: start_seq,
            target_seq,
        };
    }

    pub fn update_catchup_progress(&mut self, current_seq: u64) {
        if let BootstrapPhase::JournalCatchup {
            current_seq: old_seq,
            target_seq,
            ..
        } = &mut self.phase
        {
            if current_seq > *old_seq {
                let delta = current_seq.saturating_sub(*old_seq);
                self.stats.total_journal_entries_caught_up += delta;
            }
            *old_seq = current_seq;
        }
    }

    pub fn complete(&mut self, at_ns: u64, tls_fingerprint: Option<String>) {
        self.stats.bootstrap_successes += 1;
        if let Some(enrollment) = &mut self.enrollment {
            enrollment.enrolled_at_ns = at_ns;
            enrollment.tls_fingerprint = tls_fingerprint.clone();
        }
        self.phase = BootstrapPhase::Complete {
            enrolled_at_ns: at_ns,
        };
        if let Some(enrollment) = &mut self.enrollment {
            enrollment.tls_fingerprint = tls_fingerprint;
        }
    }

    pub fn fail(&mut self, reason: String) {
        self.stats.bootstrap_failures += 1;
        self.phase = BootstrapPhase::Failed { reason };
    }

    pub fn progress(&self, now_ns: u64) -> BootstrapProgress {
        let percent_complete = match &self.phase {
            BootstrapPhase::Idle => 0,
            BootstrapPhase::Enrolling { .. } => 5,
            BootstrapPhase::SnapshotTransfer {
                bytes_received,
                bytes_total,
            } => {
                if *bytes_total > 0 {
                    5 + (90 * bytes_received / bytes_total) as u8
                } else {
                    5
                }
            }
            BootstrapPhase::JournalCatchup { .. } => 95,
            BootstrapPhase::Complete { .. } => 100,
            BootstrapPhase::Failed { .. } => 0,
        };
        let elapsed_ms = (now_ns.saturating_sub(self.started_at_ns)) / 1_000_000;
        BootstrapProgress {
            phase: self.phase.clone(),
            percent_complete,
            elapsed_ms,
        }
    }

    pub fn phase(&self) -> &BootstrapPhase {
        &self.phase
    }

    pub fn enrollment(&self) -> Option<&EnrollmentRecord> {
        self.enrollment.as_ref()
    }

    pub fn stats(&self) -> &BootstrapStats {
        &self.stats
    }

    pub fn is_active(&self) -> bool {
        matches!(
            self.phase,
            BootstrapPhase::Enrolling { .. }
                | BootstrapPhase::SnapshotTransfer { .. }
                | BootstrapPhase::JournalCatchup { .. }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_coordinator_idle_phase() {
        let coord = BootstrapCoordinator::new(1);
        assert!(matches!(coord.phase(), BootstrapPhase::Idle));
    }

    #[test]
    fn test_new_coordinator_stats_zero() {
        let coord = BootstrapCoordinator::new(1);
        let stats = coord.stats();
        assert_eq!(stats.bootstrap_attempts, 0);
        assert_eq!(stats.bootstrap_successes, 0);
        assert_eq!(stats.bootstrap_failures, 0);
    }

    #[test]
    fn test_start_enroll_sets_phase() {
        let mut coord = BootstrapCoordinator::new(1);
        coord.start_enroll(2, 1000);
        assert!(matches!(
            coord.phase(),
            BootstrapPhase::Enrolling { primary_site: 2 }
        ));
    }

    #[test]
    fn test_start_enroll_increments_attempts() {
        let mut coord = BootstrapCoordinator::new(1);
        coord.start_enroll(2, 1000);
        assert_eq!(coord.stats().bootstrap_attempts, 1);
    }

    #[test]
    fn test_start_enroll_creates_enrollment() {
        let mut coord = BootstrapCoordinator::new(1);
        coord.start_enroll(2, 1000);
        let enrollment = coord.enrollment().expect("should have enrollment");
        assert_eq!(enrollment.site_id, 2);
    }

    #[test]
    fn test_begin_snapshot_sets_phase() {
        let mut coord = BootstrapCoordinator::new(1);
        coord.begin_snapshot(2, 1000);
        assert!(matches!(
            coord.phase(),
            BootstrapPhase::SnapshotTransfer {
                primary_site: 2,
                bytes_total: 1000,
                ..
            }
        ));
    }

    #[test]
    fn test_update_snapshot_progress() {
        let mut coord = BootstrapCoordinator::new(1);
        coord.begin_snapshot(2, 1000);
        coord.update_snapshot_progress(500);
        if let BootstrapPhase::SnapshotTransfer { bytes_received, .. } = coord.phase() {
            assert_eq!(*bytes_received, 500);
        } else {
            panic!("expected SnapshotTransfer");
        }
    }

    #[test]
    fn test_update_snapshot_progress_increments_bytes() {
        let mut coord = BootstrapCoordinator::new(1);
        coord.begin_snapshot(2, 1000);
        coord.update_snapshot_progress(300);
        assert_eq!(coord.stats().total_bytes_transferred, 300);
    }

    #[test]
    fn test_begin_journal_catchup_sets_phase() {
        let mut coord = BootstrapCoordinator::new(1);
        coord.begin_journal_catchup(2, 100, 500);
        assert!(matches!(
            coord.phase(),
            BootstrapPhase::JournalCatchup {
                primary_site: 2,
                start_seq: 100,
                target_seq: 500,
                ..
            }
        ));
    }

    #[test]
    fn test_update_catchup_progress() {
        let mut coord = BootstrapCoordinator::new(1);
        coord.begin_journal_catchup(2, 100, 500);
        coord.update_catchup_progress(300);
        if let BootstrapPhase::JournalCatchup { current_seq, .. } = coord.phase() {
            assert_eq!(*current_seq, 300);
        } else {
            panic!("expected JournalCatchup");
        }
    }

    #[test]
    fn test_complete_sets_phase() {
        let mut coord = BootstrapCoordinator::new(1);
        coord.start_enroll(2, 1000);
        coord.complete(5000, Some("fp".to_string()));
        assert!(matches!(
            coord.phase(),
            BootstrapPhase::Complete {
                enrolled_at_ns: 5000
            }
        ));
    }

    #[test]
    fn test_complete_increments_successes() {
        let mut coord = BootstrapCoordinator::new(1);
        coord.start_enroll(2, 1000);
        coord.complete(5000, None);
        assert_eq!(coord.stats().bootstrap_successes, 1);
    }

    #[test]
    fn test_complete_sets_tls_fingerprint() {
        let mut coord = BootstrapCoordinator::new(1);
        coord.start_enroll(2, 1000);
        coord.complete(5000, Some("abc123".to_string()));
        let enrollment = coord.enrollment().expect("should have enrollment");
        assert_eq!(enrollment.tls_fingerprint, Some("abc123".to_string()));
    }

    #[test]
    fn test_fail_sets_phase() {
        let mut coord = BootstrapCoordinator::new(1);
        coord.start_enroll(2, 1000);
        coord.fail("network error".to_string());
        assert!(matches!(
            coord.phase(),
            BootstrapPhase::Failed { reason: s } if s == "network error"
        ));
    }

    #[test]
    fn test_fail_increments_failures() {
        let mut coord = BootstrapCoordinator::new(1);
        coord.start_enroll(2, 1000);
        coord.fail("timeout".to_string());
        assert_eq!(coord.stats().bootstrap_failures, 1);
    }

    #[test]
    fn test_progress_idle_zero_percent() {
        let coord = BootstrapCoordinator::new(1);
        let progress = coord.progress(1000);
        assert_eq!(progress.percent_complete, 0);
    }

    #[test]
    fn test_progress_enrolling_five_percent() {
        let mut coord = BootstrapCoordinator::new(1);
        coord.start_enroll(2, 1000);
        let progress = coord.progress(2000);
        assert_eq!(progress.percent_complete, 5);
    }

    #[test]
    fn test_progress_snapshot_calculates_percent() {
        let mut coord = BootstrapCoordinator::new(1);
        coord.begin_snapshot(2, 1000);
        coord.update_snapshot_progress(500);
        let progress = coord.progress(2000);
        assert_eq!(progress.percent_complete, 50);
    }

    #[test]
    fn test_progress_journal_catchup_ninety_five_percent() {
        let mut coord = BootstrapCoordinator::new(1);
        coord.begin_journal_catchup(2, 100, 500);
        let progress = coord.progress(2000);
        assert_eq!(progress.percent_complete, 95);
    }

    #[test]
    fn test_progress_complete_one_hundred_percent() {
        let mut coord = BootstrapCoordinator::new(1);
        coord.start_enroll(2, 1000);
        coord.complete(5000, None);
        let progress = coord.progress(6000);
        assert_eq!(progress.percent_complete, 100);
    }

    #[test]
    fn test_progress_failed_zero_percent() {
        let mut coord = BootstrapCoordinator::new(1);
        coord.start_enroll(2, 1000);
        coord.fail("error".to_string());
        let progress = coord.progress(5000);
        assert_eq!(progress.percent_complete, 0);
    }

    #[test]
    fn test_is_active_true_for_enrolling() {
        let mut coord = BootstrapCoordinator::new(1);
        coord.start_enroll(2, 1000);
        assert!(coord.is_active());
    }

    #[test]
    fn test_is_active_false_for_idle() {
        let coord = BootstrapCoordinator::new(1);
        assert!(!coord.is_active());
    }

    #[test]
    fn test_is_active_false_for_complete() {
        let mut coord = BootstrapCoordinator::new(1);
        coord.start_enroll(2, 1000);
        coord.complete(5000, None);
        assert!(!coord.is_active());
    }
}
