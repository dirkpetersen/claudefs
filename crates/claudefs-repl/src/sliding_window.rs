//! Sliding window acknowledgment protocol for reliable in-order delivery.

use std::collections::VecDeque;
use thiserror::Error;

/// Configuration for the sliding window.
#[derive(Debug, Clone, Copy)]
pub struct WindowConfig {
    /// Max in-flight batches.
    pub window_size: usize,
    /// Ms before an in-flight batch is considered timed out.
    pub ack_timeout_ms: u64,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            window_size: 32,
            ack_timeout_ms: 5000,
        }
    }
}

/// An in-flight batch waiting for acknowledgment.
#[derive(Debug, Clone)]
pub struct InFlightBatch {
    /// Batch sequence number.
    pub batch_seq: u64,
    /// Number of entries in this batch.
    pub entry_count: usize,
    /// Unix milliseconds when batch was first sent.
    pub sent_at_ms: u64,
    /// How many times this batch has been retransmitted.
    pub retransmit_count: u32,
}

/// Window state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowState {
    /// Window has capacity.
    Ready,
    /// Window is full, cannot send more.
    Full,
    /// No batches in flight.
    Drained,
}

/// Statistics for the sliding window.
#[derive(Debug, Default, Clone)]
pub struct WindowStats {
    /// Total batches sent.
    pub total_sent: u64,
    /// Total batches acknowledged.
    pub total_acked: u64,
    /// Total batches that timed out.
    pub total_timed_out: u64,
    /// Total retransmits.
    pub total_retransmits: u64,
    /// Current in-flight count.
    pub current_in_flight: usize,
}

/// Errors for sliding window operations.
#[derive(Debug, Error)]
pub enum WindowError {
    /// Sliding window is full.
    #[error("sliding window is full: {0} batches in flight")]
    Full(usize),
    /// Batch not found.
    #[error("batch seq {0} not found in flight")]
    NotFound(u64),
}

/// Sliding window for reliable in-order batch delivery.
#[derive(Debug)]
pub struct SlidingWindow {
    config: WindowConfig,
    next_batch_seq: u64,
    in_flight: VecDeque<InFlightBatch>,
    stats: WindowStats,
}

impl SlidingWindow {
    /// Create a new sliding window.
    pub fn new(config: WindowConfig) -> Self {
        Self {
            config,
            next_batch_seq: 1,
            in_flight: VecDeque::new(),
            stats: WindowStats::default(),
        }
    }

    /// Send a batch, returning the assigned sequence number.
    pub fn send_batch(&mut self, entry_count: usize, now_ms: u64) -> Result<u64, WindowError> {
        if self.in_flight.len() >= self.config.window_size {
            return Err(WindowError::Full(self.in_flight.len()));
        }

        let batch_seq = self.next_batch_seq;
        self.next_batch_seq += 1;

        let batch = InFlightBatch {
            batch_seq,
            entry_count,
            sent_at_ms: now_ms,
            retransmit_count: 0,
        };

        self.in_flight.push_back(batch);
        self.stats.total_sent += 1;
        self.stats.current_in_flight = self.in_flight.len();

        Ok(batch_seq)
    }

    /// Acknowledge a batch, removing it from in-flight.
    pub fn acknowledge(&mut self, batch_seq: u64) -> Result<usize, WindowError> {
        let mut entry_count = 0;

        // Cumulative ACK: remove all batches with seq <= batch_seq
        while let Some(front) = self.in_flight.front() {
            if front.batch_seq <= batch_seq {
                entry_count += self.in_flight.pop_front().unwrap().entry_count;
                self.stats.total_acked += 1;
            } else {
                break;
            }
        }

        if entry_count == 0 && !self.in_flight.is_empty() {
            // Check if the specific batch exists
            let found = self.in_flight.iter().any(|b| b.batch_seq == batch_seq);
            if !found {
                return Err(WindowError::NotFound(batch_seq));
            }
        }

        self.stats.current_in_flight = self.in_flight.len();
        Ok(entry_count)
    }

    /// Get the current window state.
    pub fn window_state(&self) -> WindowState {
        if self.in_flight.is_empty() {
            WindowState::Drained
        } else if self.in_flight.len() >= self.config.window_size {
            WindowState::Full
        } else {
            WindowState::Ready
        }
    }

    /// Get batch sequences that have timed out.
    pub fn timed_out_batches(&self, now_ms: u64) -> Vec<u64> {
        self.in_flight
            .iter()
            .filter(|b| now_ms.saturating_sub(b.sent_at_ms) >= self.config.ack_timeout_ms)
            .map(|b| b.batch_seq)
            .collect()
    }

    /// Mark a batch for retransmission.
    pub fn mark_retransmit(&mut self, batch_seq: u64) {
        if let Some(batch) = self.in_flight.iter_mut().find(|b| b.batch_seq == batch_seq) {
            batch.retransmit_count += 1;
            self.stats.total_retransmits += 1;
        }
    }

    /// Number of batches in flight.
    pub fn in_flight_count(&self) -> usize {
        self.in_flight.len()
    }

    /// Next batch sequence to be assigned.
    pub fn next_seq(&self) -> u64 {
        self.next_batch_seq
    }

    /// Get window statistics.
    pub fn stats(&self) -> &WindowStats {
        &self.stats
    }
}

#[allow(dead_code)]
fn current_time_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_window_new_default_config() {
        let window = SlidingWindow::new(WindowConfig::default());
        assert_eq!(window.window_state(), WindowState::Drained);
        assert_eq!(window.next_seq(), 1);
    }

    #[test]
    fn test_window_send_batch_assigns_seq() {
        let mut window = SlidingWindow::new(WindowConfig::default());
        let seq = window.send_batch(10, current_time_ms()).unwrap();
        assert_eq!(seq, 1);
    }

    #[test]
    fn test_window_send_increments_next_seq() {
        let mut window = SlidingWindow::new(WindowConfig::default());
        window.send_batch(10, current_time_ms()).unwrap();
        assert_eq!(window.next_seq(), 2);
        window.send_batch(5, current_time_ms()).unwrap();
        assert_eq!(window.next_seq(), 3);
    }

    #[test]
    fn test_window_acknowledge_removes_batch() {
        let mut window = SlidingWindow::new(WindowConfig::default());
        let seq = window.send_batch(10, current_time_ms()).unwrap();
        window.acknowledge(seq).unwrap();
        assert_eq!(window.in_flight_count(), 0);
    }

    #[test]
    fn test_window_cumulative_ack() {
        let mut window = SlidingWindow::new(WindowConfig::default());
        window.send_batch(5, current_time_ms()).unwrap();
        window.send_batch(3, current_time_ms()).unwrap();
        window.send_batch(2, current_time_ms()).unwrap();

        // Ack seq 2 removes batches 1 and 2 (cumulative: 5 + 3 = 8)
        let count = window.acknowledge(2).unwrap();
        assert_eq!(count, 8); // 5 + 3
        assert_eq!(window.in_flight_count(), 1);
    }

    #[test]
    fn test_window_full_rejects_send() {
        let mut window = SlidingWindow::new(WindowConfig {
            window_size: 2,
            ack_timeout_ms: 5000,
        });

        window.send_batch(1, current_time_ms()).unwrap();
        window.send_batch(1, current_time_ms()).unwrap();

        let result = window.send_batch(1, current_time_ms());
        assert!(result.is_err());
    }

    #[test]
    fn test_window_state_transitions() {
        let mut window = SlidingWindow::new(WindowConfig {
            window_size: 2,
            ack_timeout_ms: 5000,
        });

        assert_eq!(window.window_state(), WindowState::Drained);

        window.send_batch(1, current_time_ms()).unwrap();
        assert_eq!(window.window_state(), WindowState::Ready);

        window.send_batch(1, current_time_ms()).unwrap();
        assert_eq!(window.window_state(), WindowState::Full);

        window.acknowledge(1).unwrap();
        assert_eq!(window.window_state(), WindowState::Ready);
    }

    #[test]
    fn test_window_state_ready_when_below_limit() {
        let mut window = SlidingWindow::new(WindowConfig {
            window_size: 5,
            ack_timeout_ms: 5000,
        });

        window.send_batch(1, current_time_ms()).unwrap();
        window.send_batch(1, current_time_ms()).unwrap();

        assert_eq!(window.window_state(), WindowState::Ready);
    }

    #[test]
    fn test_window_state_drained_when_empty() {
        let window = SlidingWindow::new(WindowConfig::default());
        assert_eq!(window.window_state(), WindowState::Drained);
    }

    #[test]
    fn test_window_timed_out_batches_empty_when_fresh() {
        let window = SlidingWindow::new(WindowConfig::default());
        let now = current_time_ms();
        let timed = window.timed_out_batches(now);
        assert!(timed.is_empty());
    }

    #[test]
    fn test_window_timed_out_batches_after_timeout() {
        let mut window = SlidingWindow::new(WindowConfig {
            window_size: 32,
            ack_timeout_ms: 1000,
        });
        let now = current_time_ms();

        window.send_batch(5, now - 2000).unwrap();

        let timed = window.timed_out_batches(now);
        assert_eq!(timed.len(), 1);
        assert_eq!(timed[0], 1);
    }

    #[test]
    fn test_window_mark_retransmit() {
        let mut window = SlidingWindow::new(WindowConfig::default());
        let seq = window.send_batch(5, current_time_ms()).unwrap();

        window.mark_retransmit(seq);

        let stats = window.stats();
        assert_eq!(stats.total_retransmits, 1);
    }

    #[test]
    fn test_window_in_flight_count() {
        let mut window = SlidingWindow::new(WindowConfig::default());
        assert_eq!(window.in_flight_count(), 0);

        window.send_batch(1, current_time_ms()).unwrap();
        assert_eq!(window.in_flight_count(), 1);

        window.send_batch(1, current_time_ms()).unwrap();
        assert_eq!(window.in_flight_count(), 2);
    }

    #[test]
    fn test_window_stats_sent() {
        let mut window = SlidingWindow::new(WindowConfig::default());
        window.send_batch(1, current_time_ms()).unwrap();
        window.send_batch(1, current_time_ms()).unwrap();

        let stats = window.stats();
        assert_eq!(stats.total_sent, 2);
    }

    #[test]
    fn test_window_stats_acked() {
        let mut window = SlidingWindow::new(WindowConfig::default());
        window.send_batch(5, current_time_ms()).unwrap();
        window.send_batch(3, current_time_ms()).unwrap();
        // Ack seq 2 removes both batches (2 batches acknowledged)
        window.acknowledge(2).unwrap();

        let stats = window.stats();
        assert_eq!(stats.total_acked, 2);
    }

    #[test]
    fn test_window_ack_unknown_returns_error() {
        let mut window = SlidingWindow::new(WindowConfig::default());
        window.send_batch(1, current_time_ms()).unwrap();

        // Ack 0 is less than any in-flight seq (1), so nothing is removed
        // and 0 is not found in in_flight, so return error
        let result = window.acknowledge(0);
        assert!(result.is_err());
    }

    #[test]
    fn test_window_ack_cumulative_removes_older() {
        let mut window = SlidingWindow::new(WindowConfig::default());
        window.send_batch(2, current_time_ms()).unwrap(); // seq 1
        window.send_batch(3, current_time_ms()).unwrap(); // seq 2
        window.send_batch(4, current_time_ms()).unwrap(); // seq 3

        window.acknowledge(2).unwrap();

        assert_eq!(window.in_flight_count(), 1);
    }

    #[test]
    fn test_window_send_ack_cycle() {
        let mut window = SlidingWindow::new(WindowConfig::default());

        for i in 1..=5 {
            let seq = window.send_batch(i as usize, current_time_ms()).unwrap();
            window.acknowledge(seq).unwrap();
        }

        assert_eq!(window.window_state(), WindowState::Drained);
    }

    #[test]
    fn test_window_multiple_timeouts() {
        let mut window = SlidingWindow::new(WindowConfig {
            window_size: 32,
            ack_timeout_ms: 1000,
        });
        let now = current_time_ms();

        window.send_batch(1, now - 2000).unwrap(); // seq 1
        window.send_batch(1, now - 1500).unwrap(); // seq 2
        window.send_batch(1, now - 500).unwrap(); // seq 3

        let timed = window.timed_out_batches(now);
        assert_eq!(timed.len(), 2);
    }

    #[test]
    fn test_window_retransmit_count_increments() {
        let mut window = SlidingWindow::new(WindowConfig::default());
        let seq = window.send_batch(5, current_time_ms()).unwrap();

        window.mark_retransmit(seq);
        window.mark_retransmit(seq);

        let stats = window.stats();
        assert_eq!(stats.total_retransmits, 2);
    }

    #[test]
    fn test_window_stats_timed_out_count() {
        let mut window = SlidingWindow::new(WindowConfig {
            window_size: 32,
            ack_timeout_ms: 1000,
        });
        let now = current_time_ms();

        window.send_batch(1, now - 2000).unwrap();
        window.send_batch(1, now - 2000).unwrap();

        // Simulate timeout detection (but don't remove them)
        let timed = window.timed_out_batches(now);
        assert_eq!(timed.len(), 2);
    }

    #[test]
    fn test_window_drain_all_via_ack() {
        let mut window = SlidingWindow::new(WindowConfig::default());

        window.send_batch(1, current_time_ms()).unwrap();
        window.send_batch(1, current_time_ms()).unwrap();
        window.send_batch(1, current_time_ms()).unwrap();

        window.acknowledge(3).unwrap();

        assert_eq!(window.window_state(), WindowState::Drained);
    }

    #[test]
    fn test_window_next_seq_starts_at_one() {
        let window = SlidingWindow::new(WindowConfig::default());
        assert_eq!(window.next_seq(), 1);
    }

    #[test]
    fn test_window_in_flight_entry_count_preserved() {
        let mut window = SlidingWindow::new(WindowConfig::default());

        window.send_batch(100, current_time_ms()).unwrap();
        window.send_batch(200, current_time_ms()).unwrap();

        let count = window.acknowledge(2).unwrap();
        assert_eq!(count, 300);
    }
}

#[cfg(test)]
mod proptest_sliding_window {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn prop_window_send_ack_order_preserved(batch_count in 1u32..20) {
            let mut window = SlidingWindow::new(WindowConfig::default());
            let mut seqs = Vec::new();

            for i in 0..batch_count {
                let seq = window.send_batch((i + 1) as usize, current_time_ms()).unwrap();
                seqs.push(seq);
            }

            let mut total_acked = 0;
            for seq in seqs {
                let count = window.acknowledge(seq).unwrap();
                total_acked += count;
            }

            // Sum of 1..batch_count = batch_count * (batch_count + 1) / 2
            let expected_total = (batch_count as usize) * ((batch_count as usize) + 1) / 2;
            prop_assert_eq!(total_acked, expected_total);
            prop_assert_eq!(window.in_flight_count(), 0);
        }
    }
}
