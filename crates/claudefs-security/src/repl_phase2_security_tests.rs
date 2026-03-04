//! Security tests for replication Phase 2 modules: journal source, sliding window, catchup
//!
//! Part of A10 Phase 3: Repl subsystem security audit — sequence attacks, replay, DoS vectors

#[cfg(test)]
mod tests {
    use claudefs_repl::{
        catchup::{CatchupConfig, CatchupState},
        journal::{JournalEntry, OpKind},
        journal_source::{JournalSource, MockJournalSource, SourceCursor, VecJournalSource},
        sliding_window::{SlidingWindow, WindowConfig, WindowError, WindowState},
    };

    fn make_entry(seq: u64) -> JournalEntry {
        JournalEntry::new(seq, 0, 1, 1000 + seq, seq, OpKind::Create, vec![])
    }

    #[test]
    fn test_mock_source_empty_poll_returns_none() {
        let mut source = MockJournalSource::new("test");
        let result = source.poll_batch(10);
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_mock_source_acknowledge_advances_cursor() {
        let mut source = MockJournalSource::new("test");
        source.push_entry(make_entry(1));
        source.poll_batch(10).unwrap();
        source.acknowledge(5).unwrap();
        let cursor = source.cursor();
        assert_eq!(cursor.last_acknowledged, 5);
    }

    #[test]
    fn test_mock_source_poll_respects_max_entries() {
        let mut source = MockJournalSource::new("test");
        for i in 1..=10 {
            source.push_entry(make_entry(i));
        }
        let result = source.poll_batch(3).unwrap();
        assert_eq!(result.unwrap().entries.len(), 3);
    }

    #[test]
    fn test_mock_source_batch_sequences_correct() {
        let mut source = MockJournalSource::new("test");
        source.push_entry(make_entry(5));
        source.push_entry(make_entry(6));
        source.push_entry(make_entry(7));
        let batch = source.poll_batch(10).unwrap().unwrap();
        assert_eq!(batch.first_seq, 5);
        assert_eq!(batch.last_seq, 7);
    }

    #[test]
    fn test_mock_source_acknowledge_arbitrary_seq() {
        let mut source = MockJournalSource::new("test");
        let result = source.acknowledge(u64::MAX);
        assert!(result.is_ok());
    }

    #[test]
    fn test_vec_source_exhaustion() {
        let entries = vec![make_entry(1), make_entry(2), make_entry(3)];
        let mut source = VecJournalSource::new("test", entries);
        assert!(source.poll_batch(10).unwrap().is_some());
        assert!(source.poll_batch(10).unwrap().is_none());
    }

    #[test]
    fn test_vec_source_acknowledge_updates_cursor() {
        let mut source = VecJournalSource::new("test", vec![make_entry(1)]);
        source.poll_batch(10).unwrap();
        source.acknowledge(1).unwrap();
        let cursor = source.cursor();
        assert_eq!(cursor.last_acknowledged, 1);
    }

    #[test]
    fn test_source_cursor_initial_state() {
        let cursor = SourceCursor::new("test-id");
        assert_eq!(cursor.last_polled, 0);
        assert_eq!(cursor.last_acknowledged, 0);
    }

    #[test]
    fn test_window_send_increments_sequence() {
        let mut window = SlidingWindow::new(WindowConfig::default());
        let seq1 = window.send_batch(1, 1000).unwrap();
        let seq2 = window.send_batch(1, 1000).unwrap();
        let seq3 = window.send_batch(1, 1000).unwrap();
        assert_eq!(seq1, 1);
        assert_eq!(seq2, 2);
        assert_eq!(seq3, 3);
    }

    #[test]
    fn test_window_full_returns_error() {
        let mut window = SlidingWindow::new(WindowConfig {
            window_size: 2,
            ack_timeout_ms: 5000,
        });
        window.send_batch(1, 1000).unwrap();
        window.send_batch(1, 1000).unwrap();
        let result = window.send_batch(1, 1000);
        assert!(matches!(result, Err(WindowError::Full(_))));
    }

    #[test]
    fn test_window_acknowledge_clears_slot() {
        let mut window = SlidingWindow::new(WindowConfig::default());
        window.send_batch(1, 1000).unwrap();
        window.send_batch(1, 1000).unwrap();
        assert_eq!(window.window_state(), WindowState::Ready);
        window.acknowledge(1).unwrap();
        assert_eq!(window.window_state(), WindowState::Ready);
    }

    #[test]
    fn test_window_ack_nonexistent_seq() {
        let mut window = SlidingWindow::new(WindowConfig::default());
        window.send_batch(1, 1000).unwrap();
        let result = window.acknowledge(0);
        assert!(matches!(result, Err(WindowError::NotFound(0))));
    }

    #[test]
    fn test_window_timed_out_detection() {
        let mut window = SlidingWindow::new(WindowConfig {
            window_size: 32,
            ack_timeout_ms: 1000,
        });
        window.send_batch(1, 0).unwrap();
        let timed = window.timed_out_batches(1001);
        assert_eq!(timed.len(), 1);
    }

    #[test]
    fn test_window_no_timeout_before_deadline() {
        let mut window = SlidingWindow::new(WindowConfig {
            window_size: 32,
            ack_timeout_ms: 5000,
        });
        window.send_batch(1, 1000).unwrap();
        let timed = window.timed_out_batches(5999);
        assert!(timed.is_empty());
    }

    #[test]
    fn test_window_retransmit_increments_count() {
        let mut window = SlidingWindow::new(WindowConfig::default());
        let seq = window.send_batch(1, 1000).unwrap();
        window.mark_retransmit(seq);
        let stats = window.stats();
        assert_eq!(stats.total_retransmits, 1);
    }

    #[test]
    fn test_window_stats_track_operations() {
        let mut window = SlidingWindow::new(WindowConfig::default());
        window.send_batch(1, 1000).unwrap();
        window.send_batch(1, 1000).unwrap();
        window.acknowledge(2).unwrap();
        let stats = window.stats();
        assert_eq!(stats.total_sent, 2);
        assert_eq!(stats.total_acked, 2);
        assert_eq!(stats.current_in_flight, 0);
    }

    #[test]
    fn test_window_state_transitions() {
        let mut window = SlidingWindow::new(WindowConfig {
            window_size: 2,
            ack_timeout_ms: 5000,
        });
        assert_eq!(window.window_state(), WindowState::Drained);
        window.send_batch(1, 1000).unwrap();
        assert_eq!(window.window_state(), WindowState::Ready);
        window.send_batch(1, 1000).unwrap();
        assert_eq!(window.window_state(), WindowState::Full);
        window.acknowledge(1).unwrap();
        assert_eq!(window.window_state(), WindowState::Ready);
    }

    #[test]
    fn test_window_cumulative_ack() {
        let mut window = SlidingWindow::new(WindowConfig::default());
        window.send_batch(1, 1000).unwrap();
        window.send_batch(1, 1000).unwrap();
        window.send_batch(1, 1000).unwrap();
        window.acknowledge(3).unwrap();
        assert_eq!(window.in_flight_count(), 0);
        assert_eq!(window.window_state(), WindowState::Drained);
    }

    #[test]
    fn test_catchup_starts_idle() {
        let state = CatchupState::new(CatchupConfig::default());
        assert!(matches!(
            state.phase(),
            claudefs_repl::catchup::CatchupPhase::Idle
        ));
    }

    #[test]
    fn test_catchup_request_transitions_to_requested() {
        let mut state = CatchupState::new(CatchupConfig::default());
        state.request(100).unwrap();
        assert!(matches!(
            state.phase(),
            claudefs_repl::catchup::CatchupPhase::Requested { cursor_seq: 100 }
        ));
    }

    #[test]
    fn test_catchup_double_request_fails() {
        let mut state = CatchupState::new(CatchupConfig::default());
        state.request(100).unwrap();
        let result = state.request(200);
        assert!(result.is_err());
    }

    #[test]
    fn test_catchup_receive_batch_transitions_to_in_progress() {
        let mut state = CatchupState::new(CatchupConfig::default());
        state.request(100).unwrap();
        state.receive_batch(50, false, 0).unwrap();
        assert!(matches!(
            state.phase(),
            claudefs_repl::catchup::CatchupPhase::InProgress {
                cursor_seq: 100,
                batches_received: 1
            }
        ));
    }

    #[test]
    fn test_catchup_final_batch_completes() {
        let mut state = CatchupState::new(CatchupConfig::default());
        state.request(100).unwrap();
        state.receive_batch(50, true, 150).unwrap();
        assert!(state.is_complete());
        assert!(matches!(
            state.phase(),
            claudefs_repl::catchup::CatchupPhase::Complete {
                final_seq: 150,
                total_entries: 50
            }
        ));
    }

    #[test]
    fn test_catchup_fail_transitions_to_failed() {
        let mut state = CatchupState::new(CatchupConfig::default());
        state.request(100).unwrap();
        state.fail("error");
        assert!(matches!(
            state.phase(),
            claudefs_repl::catchup::CatchupPhase::Failed { reason: _ }
        ));
    }

    #[test]
    fn test_catchup_reset_returns_to_idle() {
        let mut state = CatchupState::new(CatchupConfig::default());
        state.request(100).unwrap();
        state.fail("error");
        let stats_before = state.stats().sessions_failed;
        state.reset();
        assert!(matches!(
            state.phase(),
            claudefs_repl::catchup::CatchupPhase::Idle
        ));
        assert_eq!(state.stats().sessions_failed, stats_before);
    }
}
