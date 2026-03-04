//! Catch-up state machine for replicas that fall behind.

use thiserror::Error;

/// Configuration for catch-up.
#[derive(Debug, Clone)]
pub struct CatchupConfig {
    /// Identifier for this catch-up session.
    pub consumer_id: String,
    /// Max entries per catch-up batch.
    pub max_batch_size: usize,
    /// Time to wait for catch-up completion before giving up (ms).
    pub timeout_ms: u64,
}

impl Default for CatchupConfig {
    fn default() -> Self {
        Self {
            consumer_id: "default-catchup".to_string(),
            max_batch_size: 500,
            timeout_ms: 30_000,
        }
    }
}

/// Phase of the catch-up state machine.
#[derive(Debug, Clone, PartialEq)]
#[allow(missing_docs)]
pub enum CatchupPhase {
    /// Not running.
    Idle,
    /// Request sent, waiting for first batch.
    Requested { cursor_seq: u64 },
    /// Receiving batches.
    InProgress {
        cursor_seq: u64,
        batches_received: u32,
    },
    /// All caught up.
    Complete { final_seq: u64, total_entries: u64 },
    /// Catch-up failed.
    Failed { reason: String },
}

/// Statistics for catch-up sessions.
#[derive(Debug, Default, Clone)]
pub struct CatchupStats {
    /// Number of sessions started.
    pub sessions_started: u32,
    /// Number of sessions completed.
    pub sessions_completed: u32,
    /// Number of sessions failed.
    pub sessions_failed: u32,
    /// Total entries received across all sessions.
    pub total_entries_received: u64,
    /// Total batches received across all sessions.
    pub total_batches_received: u32,
}

/// Errors for catch-up operations.
#[derive(Debug, Error)]
pub enum CatchupError {
    /// Catch-up already running.
    #[error("catch-up already running")]
    AlreadyRunning,
    /// Unexpected batch received.
    #[error("unexpected batch in phase {0:?}")]
    UnexpectedBatch(String),
}

/// State machine managing the catch-up protocol.
#[derive(Debug)]
pub struct CatchupState {
    #[allow(dead_code)]
    config: CatchupConfig,
    phase: CatchupPhase,
    stats: CatchupStats,
}

impl CatchupState {
    /// Create a new catch-up state.
    pub fn new(config: CatchupConfig) -> Self {
        Self {
            config,
            phase: CatchupPhase::Idle,
            stats: CatchupStats::default(),
        }
    }

    /// Request catch-up from the given sequence.
    pub fn request(&mut self, from_seq: u64) -> Result<(), CatchupError> {
        if !matches!(
            self.phase,
            CatchupPhase::Idle | CatchupPhase::Complete { .. } | CatchupPhase::Failed { .. }
        ) {
            return Err(CatchupError::AlreadyRunning);
        }

        self.phase = CatchupPhase::Requested {
            cursor_seq: from_seq,
        };
        self.stats.sessions_started += 1;
        Ok(())
    }

    /// Receive a batch during catch-up.
    pub fn receive_batch(
        &mut self,
        entry_count: usize,
        is_final: bool,
        final_seq: u64,
    ) -> Result<(), CatchupError> {
        match &self.phase {
            CatchupPhase::Requested { cursor_seq } => {
                self.phase = CatchupPhase::InProgress {
                    cursor_seq: *cursor_seq,
                    batches_received: 1,
                };
                self.stats.total_entries_received += entry_count as u64;
                self.stats.total_batches_received += 1;

                if is_final {
                    self.phase = CatchupPhase::Complete {
                        final_seq,
                        total_entries: self.stats.total_entries_received,
                    };
                    self.stats.sessions_completed += 1;
                }
                Ok(())
            }
            CatchupPhase::InProgress {
                cursor_seq,
                batches_received,
            } => {
                self.phase = CatchupPhase::InProgress {
                    cursor_seq: *cursor_seq,
                    batches_received: batches_received + 1,
                };
                self.stats.total_entries_received += entry_count as u64;
                self.stats.total_batches_received += 1;

                if is_final {
                    self.phase = CatchupPhase::Complete {
                        final_seq,
                        total_entries: self.stats.total_entries_received,
                    };
                    self.stats.sessions_completed += 1;
                }
                Ok(())
            }
            other => Err(CatchupError::UnexpectedBatch(format!("{:?}", other))),
        }
    }

    /// Mark catch-up as failed.
    pub fn fail(&mut self, reason: impl Into<String>) {
        self.phase = CatchupPhase::Failed {
            reason: reason.into(),
        };
        self.stats.sessions_failed += 1;
    }

    /// Reset to idle state.
    pub fn reset(&mut self) {
        self.phase = CatchupPhase::Idle;
    }

    /// Get the current phase.
    pub fn phase(&self) -> &CatchupPhase {
        &self.phase
    }

    /// Get catch-up statistics.
    pub fn stats(&self) -> &CatchupStats {
        &self.stats
    }

    /// Check if catch-up is currently running.
    pub fn is_running(&self) -> bool {
        matches!(
            self.phase,
            CatchupPhase::Requested { .. } | CatchupPhase::InProgress { .. }
        )
    }

    /// Check if catch-up has completed.
    pub fn is_complete(&self) -> bool {
        matches!(self.phase, CatchupPhase::Complete { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_catchup_new_default_config() {
        let state = CatchupState::new(CatchupConfig::default());
        assert_eq!(state.phase(), &CatchupPhase::Idle);
    }

    #[test]
    fn test_catchup_initial_phase_idle() {
        let state = CatchupState::new(CatchupConfig::default());
        assert!(matches!(state.phase(), CatchupPhase::Idle));
    }

    #[test]
    fn test_catchup_request_transitions_to_requested() {
        let mut state = CatchupState::new(CatchupConfig::default());
        state.request(100).unwrap();
        assert!(matches!(
            state.phase(),
            CatchupPhase::Requested { cursor_seq: 100 }
        ));
    }

    #[test]
    fn test_catchup_request_while_running_fails() {
        let mut state = CatchupState::new(CatchupConfig::default());
        state.request(100).unwrap();
        let result = state.request(200);
        assert!(result.is_err());
    }

    #[test]
    fn test_catchup_first_batch_transitions_to_in_progress() {
        let mut state = CatchupState::new(CatchupConfig::default());
        state.request(100).unwrap();
        state.receive_batch(50, false, 0).unwrap();
        assert!(matches!(
            state.phase(),
            CatchupPhase::InProgress {
                cursor_seq: 100,
                batches_received: 1
            }
        ));
    }

    #[test]
    fn test_catchup_multiple_batches_accumulate() {
        let mut state = CatchupState::new(CatchupConfig::default());
        state.request(100).unwrap();
        state.receive_batch(50, false, 0).unwrap();
        state.receive_batch(30, false, 0).unwrap();

        let stats = state.stats();
        assert_eq!(stats.total_entries_received, 80);
        assert_eq!(stats.total_batches_received, 2);
    }

    #[test]
    fn test_catchup_final_batch_completes() {
        let mut state = CatchupState::new(CatchupConfig::default());
        state.request(100).unwrap();
        state.receive_batch(50, true, 150).unwrap();

        assert!(state.is_complete());
        assert!(matches!(
            state.phase(),
            CatchupPhase::Complete {
                final_seq: 150,
                total_entries: 50
            }
        ));
    }

    #[test]
    fn test_catchup_unexpected_batch_when_idle_fails() {
        let mut state = CatchupState::new(CatchupConfig::default());
        let result = state.receive_batch(50, false, 0);
        assert!(result.is_err());
    }

    #[test]
    fn test_catchup_fail_transitions_to_failed() {
        let mut state = CatchupState::new(CatchupConfig::default());
        state.request(100).unwrap();
        state.fail("network error");

        assert!(matches!(state.phase(), CatchupPhase::Failed { reason: _ }));
        assert_eq!(state.stats().sessions_failed, 1);
    }

    #[test]
    fn test_catchup_reset_from_failed() {
        let mut state = CatchupState::new(CatchupConfig::default());
        state.request(100).unwrap();
        state.fail("error");
        state.reset();

        assert!(matches!(state.phase(), CatchupPhase::Idle));
    }

    #[test]
    fn test_catchup_reset_from_complete() {
        let mut state = CatchupState::new(CatchupConfig::default());
        state.request(100).unwrap();
        state.receive_batch(50, true, 150).unwrap();
        state.reset();

        assert!(matches!(state.phase(), CatchupPhase::Idle));
    }

    #[test]
    fn test_catchup_reset_from_in_progress() {
        let mut state = CatchupState::new(CatchupConfig::default());
        state.request(100).unwrap();
        state.receive_batch(50, false, 0).unwrap();
        state.reset();

        assert!(matches!(state.phase(), CatchupPhase::Idle));
    }

    #[test]
    fn test_catchup_is_running_true_when_active() {
        let mut state = CatchupState::new(CatchupConfig::default());
        state.request(100).unwrap();
        assert!(state.is_running());

        state.receive_batch(50, false, 0).unwrap();
        assert!(state.is_running());
    }

    #[test]
    fn test_catchup_is_running_false_when_idle() {
        let state = CatchupState::new(CatchupConfig::default());
        assert!(!state.is_running());
    }

    #[test]
    fn test_catchup_is_complete_false_when_in_progress() {
        let mut state = CatchupState::new(CatchupConfig::default());
        state.request(100).unwrap();
        state.receive_batch(50, false, 0).unwrap();

        assert!(!state.is_complete());
    }

    #[test]
    fn test_catchup_is_complete_true_after_final() {
        let mut state = CatchupState::new(CatchupConfig::default());
        state.request(100).unwrap();
        state.receive_batch(50, true, 150).unwrap();

        assert!(state.is_complete());
    }

    #[test]
    fn test_catchup_stats_sessions_started() {
        let mut state = CatchupState::new(CatchupConfig::default());
        state.request(100).unwrap();

        assert_eq!(state.stats().sessions_started, 1);
    }

    #[test]
    fn test_catchup_stats_sessions_completed() {
        let mut state = CatchupState::new(CatchupConfig::default());
        state.request(100).unwrap();
        state.receive_batch(50, true, 150).unwrap();

        assert_eq!(state.stats().sessions_completed, 1);
    }

    #[test]
    fn test_catchup_stats_sessions_failed() {
        let mut state = CatchupState::new(CatchupConfig::default());
        state.request(100).unwrap();
        state.fail("error");

        assert_eq!(state.stats().sessions_failed, 1);
    }

    #[test]
    fn test_catchup_stats_total_entries() {
        let mut state = CatchupState::new(CatchupConfig::default());
        state.request(100).unwrap();
        state.receive_batch(50, false, 0).unwrap();
        state.receive_batch(30, false, 0).unwrap();
        state.receive_batch(20, true, 200).unwrap();

        assert_eq!(state.stats().total_entries_received, 100);
    }

    #[test]
    fn test_catchup_stats_total_batches() {
        let mut state = CatchupState::new(CatchupConfig::default());
        state.request(100).unwrap();
        state.receive_batch(50, false, 0).unwrap();
        state.receive_batch(30, false, 0).unwrap();
        state.receive_batch(20, true, 200).unwrap();

        assert_eq!(state.stats().total_batches_received, 3);
    }

    #[test]
    fn test_catchup_complete_final_seq_recorded() {
        let mut state = CatchupState::new(CatchupConfig::default());
        state.request(100).unwrap();
        state.receive_batch(50, true, 150).unwrap();

        if let CatchupPhase::Complete { final_seq, .. } = state.phase() {
            assert_eq!(*final_seq, 150);
        } else {
            panic!("expected Complete phase");
        }
    }

    #[test]
    fn test_catchup_complete_total_entries_correct() {
        let mut state = CatchupState::new(CatchupConfig::default());
        state.request(100).unwrap();
        state.receive_batch(50, false, 0).unwrap();
        state.receive_batch(30, true, 180).unwrap();

        if let CatchupPhase::Complete { total_entries, .. } = state.phase() {
            assert_eq!(*total_entries, 80);
        } else {
            panic!("expected Complete phase");
        }
    }

    #[test]
    fn test_catchup_fail_reason_preserved() {
        let mut state = CatchupState::new(CatchupConfig::default());
        state.request(100).unwrap();
        state.fail("timeout waiting for primary");

        if let CatchupPhase::Failed { reason } = state.phase() {
            assert_eq!(reason, "timeout waiting for primary");
        } else {
            panic!("expected Failed phase");
        }
    }
}

#[cfg(test)]
mod proptest_catchup {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn prop_catchup_session_lifecycle(batch_count in 1u32..10) {
            let mut state = CatchupState::new(CatchupConfig::default());
            state.request(100).unwrap();

            let mut total_entries = 0u64;
            for i in 0..batch_count {
                let entries = ((i + 1) * 10) as usize;
                let is_final = i == batch_count - 1;
                let final_seq = 100 + entries as u64;
                state.receive_batch(entries, is_final, final_seq).unwrap();
                total_entries += entries as u64;
            }

            prop_assert!(state.is_complete());

            if let CatchupPhase::Complete { total_entries: stored_total, .. } = state.phase() {
                prop_assert_eq!(*stored_total, total_entries);
            } else {
                prop_assert!(false, "expected Complete phase");
            }

            prop_assert_eq!(state.stats().sessions_completed, 1);
        }
    }
}
