//! Journal Replication Channel for cross-site replication.
//!
//! A dedicated channel type for ordered journal replication to remote sites
//! (A6 replication agent). Cross-site replication needs: ordered delivery,
//! per-channel backpressure, reconnection, byte-counted for bandwidth limiting.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

/// Configuration for a replication channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplChannelConfig {
    /// Remote site identifier, e.g. "site-b".
    pub remote_site: String,
    /// Remote address, e.g. "10.0.2.50:9401".
    pub remote_addr: String,
    /// Maximum in-flight bytes for backpressure.
    pub max_inflight_bytes: u64,
    /// Maximum in-flight entries for backpressure.
    pub max_inflight_entries: usize,
    /// Ack timeout in milliseconds.
    pub ack_timeout_ms: u64,
    /// Base reconnect backoff in milliseconds.
    pub reconnect_backoff_ms: u64,
    /// Maximum reconnect backoff in milliseconds.
    pub max_reconnect_backoff_ms: u64,
    /// Optional bandwidth limit in bytes per second.
    pub bandwidth_limit_bps: Option<u64>,
}

impl Default for ReplChannelConfig {
    fn default() -> Self {
        Self {
            remote_site: String::new(),
            remote_addr: String::new(),
            max_inflight_bytes: 64 * 1024 * 1024,
            max_inflight_entries: 1024,
            ack_timeout_ms: 5000,
            reconnect_backoff_ms: 1000,
            max_reconnect_backoff_ms: 30000,
            bandwidth_limit_bps: None,
        }
    }
}

/// State of a replication channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReplChannelState {
    /// Channel is idle, no replication in progress.
    Idle,
    /// Channel is connecting to remote site.
    Connecting,
    /// Channel is actively replicating.
    Replicating,
    /// Channel is draining in-flight entries before shutdown.
    Draining,
    /// Channel is disconnected.
    Disconnected,
    /// Channel has failed with an error.
    Failed(String),
}

/// A journal entry to be replicated.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JournalEntry {
    /// Monotonically increasing sequence number per-channel.
    pub sequence: u64,
    /// Originating site identifier.
    pub site_id: u32,
    /// Shard this entry belongs to.
    pub shard_id: u32,
    /// Opaque serialized log entry payload.
    pub payload: Vec<u8>,
    /// Timestamp when entry was created (ms since epoch).
    pub timestamp_ms: u64,
}

/// Acknowledgement from the remote site.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReplAck {
    /// All entries with sequence <= this are confirmed applied.
    pub up_to_sequence: u64,
    /// Site ID of the acknowledger.
    pub site_id: u32,
}

/// Tracks an in-flight entry awaiting acknowledgement.
#[derive(Clone, Debug)]
pub struct InFlightEntry {
    /// The journal entry being replicated.
    pub entry: JournalEntry,
    /// Timestamp when the entry was sent (ms since epoch).
    pub sent_at_ms: u64,
}

/// Errors in replication channel operations.
#[derive(Debug, Error)]
pub enum ReplError {
    /// Channel is backpressured (too many in-flight bytes).
    #[error("Channel backpressured: {inflight_bytes} inflight bytes")]
    Backpressured {
        /// Current in-flight bytes.
        inflight_bytes: u64,
    },
    /// Channel is not ready for replication.
    #[error("Channel not ready: state is {state:?}")]
    NotReady {
        /// Current channel state as string.
        state: String,
    },
    /// Unknown sequence number.
    #[error("Unknown sequence: {sequence}")]
    UnknownSequence {
        /// The unknown sequence number.
        sequence: u64,
    },
    /// Bandwidth limit exceeded.
    #[error("Bandwidth limit exceeded")]
    BandwidthLimitExceeded,
}

/// Statistics for a replication channel.
#[derive(Debug, Clone, Default)]
pub struct ReplChannelStats {
    /// Current state as string.
    pub state: String,
    /// Total entries sent.
    pub entries_sent: u64,
    /// Total entries acknowledged.
    pub entries_acked: u64,
    /// Total bytes sent.
    pub bytes_sent: u64,
    /// Total bytes acknowledged.
    pub bytes_acked: u64,
    /// Current in-flight entry count.
    pub inflight_count: usize,
    /// Current in-flight byte count.
    pub inflight_bytes: u64,
    /// Highest acknowledged sequence.
    pub acked_sequence: u64,
    /// Total reconnect attempts.
    pub total_reconnects: u32,
    /// Total failures.
    pub total_failures: u32,
}

/// Immutable snapshot of replication channel statistics.
#[derive(Debug, Clone)]
pub struct ReplChannelStatsSnapshot {
    /// Current state as string.
    pub state: String,
    /// Total entries sent.
    pub entries_sent: u64,
    /// Total entries acknowledged.
    pub entries_acked: u64,
    /// Total bytes sent.
    pub bytes_sent: u64,
    /// Total bytes acknowledged.
    pub bytes_acked: u64,
    /// Current in-flight entry count.
    pub inflight_count: usize,
    /// Current in-flight byte count.
    pub inflight_bytes: u64,
    /// Highest acknowledged sequence.
    pub acked_sequence: u64,
    /// Total reconnect attempts.
    pub total_reconnects: u32,
    /// Total failures.
    pub total_failures: u32,
}

impl From<&ReplChannelStats> for ReplChannelStatsSnapshot {
    fn from(stats: &ReplChannelStats) -> Self {
        Self {
            state: stats.state.clone(),
            entries_sent: stats.entries_sent,
            entries_acked: stats.entries_acked,
            bytes_sent: stats.bytes_sent,
            bytes_acked: stats.bytes_acked,
            inflight_count: stats.inflight_count,
            inflight_bytes: stats.inflight_bytes,
            acked_sequence: stats.acked_sequence,
            total_reconnects: stats.total_reconnects,
            total_failures: stats.total_failures,
        }
    }
}

/// A replication channel to a remote site.
#[derive(Debug)]
pub struct ReplChannel {
    /// Configuration for the channel.
    config: ReplChannelConfig,
    /// Current state of the channel.
    state: ReplChannelState,
    /// Next sequence number to assign.
    next_sequence: u64,
    /// Highest sequence acknowledged by remote.
    acked_sequence: u64,
    /// Whether we have received at least one ack.
    has_received_ack: bool,
    /// In-flight entries awaiting acknowledgement (seq -> entry).
    inflight: HashMap<u64, InFlightEntry>,
    /// Total bytes currently in-flight.
    inflight_bytes: u64,
    /// Number of reconnect attempts.
    reconnect_attempts: u32,
    /// Timestamp of last reconnect attempt.
    last_reconnect_ms: u64,
    /// Total entries sent.
    entries_sent: u64,
    /// Total entries acknowledged.
    entries_acked: u64,
    /// Total bytes sent.
    bytes_sent: u64,
    /// Total bytes acknowledged.
    bytes_acked: u64,
    /// Total reconnect attempts.
    total_reconnects: u32,
    /// Total failures.
    total_failures: u32,
}

impl ReplChannel {
    /// Creates a new replication channel with the given configuration.
    pub fn new(config: ReplChannelConfig) -> Self {
        Self {
            config,
            state: ReplChannelState::Idle,
            next_sequence: 0,
            acked_sequence: 0,
            has_received_ack: false,
            inflight: HashMap::new(),
            inflight_bytes: 0,
            reconnect_attempts: 0,
            last_reconnect_ms: 0,
            entries_sent: 0,
            entries_acked: 0,
            bytes_sent: 0,
            bytes_acked: 0,
            total_reconnects: 0,
            total_failures: 0,
        }
    }

    /// Returns the remote site identifier.
    pub fn remote_site(&self) -> &str {
        &self.config.remote_site
    }

    /// Returns the current state of the channel.
    pub fn state(&self) -> &ReplChannelState {
        &self.state
    }

    /// Returns true if the channel is ready for replication.
    ///
    /// Ready means Replicating state and not backpressured.
    pub fn is_ready(&self) -> bool {
        matches!(self.state, ReplChannelState::Replicating)
            && self.inflight_bytes < self.config.max_inflight_bytes
            && self.inflight.len() < self.config.max_inflight_entries
    }

    /// Enqueues an entry for replication.
    ///
    /// Returns the assigned sequence number, or an error if backpressured.
    pub fn enqueue(
        &mut self,
        shard_id: u32,
        site_id: u32,
        payload: Vec<u8>,
        now_ms: u64,
    ) -> Result<u64, ReplError> {
        if !matches!(self.state, ReplChannelState::Replicating) {
            return Err(ReplError::NotReady {
                state: format!("{:?}", self.state),
            });
        }

        let payload_len = payload.len() as u64;

        if self.inflight_bytes + payload_len > self.config.max_inflight_bytes {
            return Err(ReplError::Backpressured {
                inflight_bytes: self.inflight_bytes,
            });
        }

        if self.inflight.len() >= self.config.max_inflight_entries {
            return Err(ReplError::Backpressured {
                inflight_bytes: self.inflight_bytes,
            });
        }

        let sequence = self.next_sequence;
        self.next_sequence += 1;

        let entry = JournalEntry {
            sequence,
            site_id,
            shard_id,
            payload,
            timestamp_ms: now_ms,
        };

        let inflight_entry = InFlightEntry {
            entry,
            sent_at_ms: now_ms,
        };

        self.inflight.insert(sequence, inflight_entry);
        self.inflight_bytes += payload_len;

        Ok(sequence)
    }

    /// Marks an entry as sent (confirms it was transmitted).
    ///
    /// This updates the sent_at timestamp.
    pub fn mark_sent(&mut self, sequence: u64, now_ms: u64) -> Result<(), ReplError> {
        if let Some(entry) = self.inflight.get_mut(&sequence) {
            entry.sent_at_ms = now_ms;
            self.entries_sent += 1;
            self.bytes_sent += entry.entry.payload.len() as u64;
            Ok(())
        } else {
            Err(ReplError::UnknownSequence { sequence })
        }
    }

    /// Processes an acknowledgement from the remote site.
    ///
    /// Returns the count of newly acknowledged entries.
    pub fn process_ack(&mut self, ack: ReplAck, _now_ms: u64) -> u64 {
        // Check for duplicate/stale ack
        if self.has_received_ack && ack.up_to_sequence <= self.acked_sequence {
            return 0;
        }

        let mut acked_count = 0u64;
        let mut acked_bytes = 0u64;

        // Determine which sequences to ack
        let seqs_to_remove: Vec<u64> = if self.has_received_ack {
            // Normal case: ack only entries after the last acked
            self.inflight
                .keys()
                .filter(|&&seq| seq <= ack.up_to_sequence && seq > self.acked_sequence)
                .copied()
                .collect()
        } else {
            // First ack: include all entries up to ack.up_to_sequence
            self.inflight
                .keys()
                .filter(|&&seq| seq <= ack.up_to_sequence)
                .copied()
                .collect()
        };

        for seq in seqs_to_remove {
            if let Some(entry) = self.inflight.remove(&seq) {
                acked_count += 1;
                acked_bytes += entry.entry.payload.len() as u64;
            }
        }

        self.acked_sequence = ack.up_to_sequence;
        self.has_received_ack = true;
        self.entries_acked += acked_count;
        self.bytes_acked += acked_bytes;
        self.inflight_bytes = self.inflight_bytes.saturating_sub(acked_bytes);

        acked_count
    }

    /// Returns sequence numbers of entries that have timed out.
    ///
    /// An entry times out if sent_at + ack_timeout_ms < now_ms.
    pub fn timed_out_entries(&self, now_ms: u64) -> Vec<u64> {
        self.inflight
            .iter()
            .filter(|(_, entry)| {
                now_ms.saturating_sub(entry.sent_at_ms) > self.config.ack_timeout_ms
            })
            .map(|(seq, _)| *seq)
            .collect()
    }

    /// Computes reconnect delay using exponential backoff.
    pub fn reconnect_delay_ms(&self) -> u64 {
        let backoff = self.config.reconnect_backoff_ms;
        let max_backoff = self.config.max_reconnect_backoff_ms;

        if self.reconnect_attempts == 0 {
            return backoff;
        }

        let delay = backoff.saturating_mul(1u64 << self.reconnect_attempts.min(16));
        delay.min(max_backoff)
    }

    /// Records a reconnect attempt.
    pub fn record_reconnect(&mut self, now_ms: u64) {
        self.reconnect_attempts += 1;
        self.total_reconnects += 1;
        self.last_reconnect_ms = now_ms;
    }

    /// Resets reconnect attempts after successful connection.
    pub fn reset_reconnect_attempts(&mut self) {
        self.reconnect_attempts = 0;
    }

    /// Transitions the channel to a new state.
    pub fn set_state(&mut self, state: ReplChannelState) {
        if matches!(state, ReplChannelState::Failed(_)) {
            self.total_failures += 1;
        }
        self.state = state;
    }

    /// Returns the current in-flight byte count.
    pub fn inflight_bytes(&self) -> u64 {
        self.inflight_bytes
    }

    /// Returns the current in-flight entry count.
    pub fn inflight_count(&self) -> usize {
        self.inflight.len()
    }

    /// Returns current statistics.
    pub fn stats(&self) -> ReplChannelStats {
        ReplChannelStats {
            state: format!("{:?}", self.state),
            entries_sent: self.entries_sent,
            entries_acked: self.entries_acked,
            bytes_sent: self.bytes_sent,
            bytes_acked: self.bytes_acked,
            inflight_count: self.inflight.len(),
            inflight_bytes: self.inflight_bytes,
            acked_sequence: self.acked_sequence,
            total_reconnects: self.total_reconnects,
            total_failures: self.total_failures,
        }
    }

    /// Returns a snapshot of current statistics.
    pub fn snapshot(&self) -> ReplChannelStatsSnapshot {
        ReplChannelStatsSnapshot::from(&self.stats())
    }

    /// Returns the configuration.
    pub fn config(&self) -> &ReplChannelConfig {
        &self.config
    }

    /// Returns the next sequence number to be assigned.
    pub fn next_sequence(&self) -> u64 {
        self.next_sequence
    }

    /// Returns the highest acknowledged sequence.
    pub fn acked_sequence(&self) -> u64 {
        self.acked_sequence
    }

    /// Returns a reference to in-flight entries.
    pub fn inflight_entries(&self) -> &HashMap<u64, InFlightEntry> {
        &self.inflight
    }

    /// Removes a timed-out entry from in-flight.
    pub fn remove_timed_out(&mut self, sequence: u64) -> Option<InFlightEntry> {
        let entry = self.inflight.remove(&sequence)?;
        self.inflight_bytes = self
            .inflight_bytes
            .saturating_sub(entry.entry.payload.len() as u64);
        Some(entry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = ReplChannelConfig::default();
        assert!(config.remote_site.is_empty());
        assert!(config.remote_addr.is_empty());
        assert_eq!(config.max_inflight_bytes, 64 * 1024 * 1024);
        assert_eq!(config.max_inflight_entries, 1024);
        assert_eq!(config.ack_timeout_ms, 5000);
        assert_eq!(config.reconnect_backoff_ms, 1000);
        assert_eq!(config.max_reconnect_backoff_ms, 30000);
        assert!(config.bandwidth_limit_bps.is_none());
    }

    #[test]
    fn test_channel_new() {
        let config = ReplChannelConfig {
            remote_site: "site-b".to_string(),
            remote_addr: "10.0.2.50:9401".to_string(),
            ..Default::default()
        };
        let channel = ReplChannel::new(config);
        assert_eq!(channel.remote_site(), "site-b");
        assert_eq!(channel.state(), &ReplChannelState::Idle);
        assert_eq!(channel.next_sequence(), 0);
        assert_eq!(channel.acked_sequence(), 0);
        assert!(!channel.is_ready());
    }

    #[test]
    fn test_channel_set_state() {
        let config = ReplChannelConfig::default();
        let mut channel = ReplChannel::new(config);

        channel.set_state(ReplChannelState::Connecting);
        assert_eq!(channel.state(), &ReplChannelState::Connecting);

        channel.set_state(ReplChannelState::Replicating);
        assert_eq!(channel.state(), &ReplChannelState::Replicating);
        assert!(channel.is_ready());
    }

    #[test]
    fn test_enqueue_success() {
        let config = ReplChannelConfig::default();
        let mut channel = ReplChannel::new(config);
        channel.set_state(ReplChannelState::Replicating);

        let seq = channel.enqueue(1, 42, vec![1, 2, 3], 1000).unwrap();
        assert_eq!(seq, 0);
        assert_eq!(channel.next_sequence(), 1);
        assert_eq!(channel.inflight_count(), 1);
        assert_eq!(channel.inflight_bytes(), 3);
    }

    #[test]
    fn test_enqueue_multiple() {
        let config = ReplChannelConfig::default();
        let mut channel = ReplChannel::new(config);
        channel.set_state(ReplChannelState::Replicating);

        let seq0 = channel.enqueue(1, 42, vec![1; 100], 1000).unwrap();
        let seq1 = channel.enqueue(2, 42, vec![2; 200], 1000).unwrap();
        let seq2 = channel.enqueue(3, 42, vec![3; 300], 1000).unwrap();

        assert_eq!(seq0, 0);
        assert_eq!(seq1, 1);
        assert_eq!(seq2, 2);
        assert_eq!(channel.inflight_count(), 3);
        assert_eq!(channel.inflight_bytes(), 600);
    }

    #[test]
    fn test_enqueue_wrong_state() {
        let config = ReplChannelConfig::default();
        let mut channel = ReplChannel::new(config);
        channel.set_state(ReplChannelState::Idle);

        let result = channel.enqueue(1, 42, vec![1, 2, 3], 1000);
        assert!(matches!(result, Err(ReplError::NotReady { .. })));
    }

    #[test]
    fn test_enqueue_backpressured_bytes() {
        let config = ReplChannelConfig {
            max_inflight_bytes: 10,
            ..Default::default()
        };
        let mut channel = ReplChannel::new(config);
        channel.set_state(ReplChannelState::Replicating);

        channel.enqueue(1, 42, vec![1; 5], 1000).unwrap();
        let result = channel.enqueue(2, 42, vec![2; 6], 1000);
        assert!(matches!(result, Err(ReplError::Backpressured { .. })));
    }

    #[test]
    fn test_enqueue_backpressured_entries() {
        let config = ReplChannelConfig {
            max_inflight_entries: 2,
            ..Default::default()
        };
        let mut channel = ReplChannel::new(config);
        channel.set_state(ReplChannelState::Replicating);

        channel.enqueue(1, 42, vec![1], 1000).unwrap();
        channel.enqueue(2, 42, vec![2], 1000).unwrap();
        let result = channel.enqueue(3, 42, vec![3], 1000);
        assert!(matches!(result, Err(ReplError::Backpressured { .. })));
    }

    #[test]
    fn test_mark_sent() {
        let config = ReplChannelConfig::default();
        let mut channel = ReplChannel::new(config);
        channel.set_state(ReplChannelState::Replicating);

        let seq = channel.enqueue(1, 42, vec![1, 2, 3], 1000).unwrap();
        channel.mark_sent(seq, 1500).unwrap();

        let entry = channel.inflight_entries().get(&seq).unwrap();
        assert_eq!(entry.sent_at_ms, 1500);
        assert_eq!(channel.stats().entries_sent, 1);
        assert_eq!(channel.stats().bytes_sent, 3);
    }

    #[test]
    fn test_mark_sent_unknown_sequence() {
        let config = ReplChannelConfig::default();
        let mut channel = ReplChannel::new(config);
        channel.set_state(ReplChannelState::Replicating);

        let result = channel.mark_sent(999, 1000);
        assert!(matches!(
            result,
            Err(ReplError::UnknownSequence { sequence: 999 })
        ));
    }

    #[test]
    fn test_process_ack_full() {
        let config = ReplChannelConfig::default();
        let mut channel = ReplChannel::new(config);
        channel.set_state(ReplChannelState::Replicating);

        channel.enqueue(1, 42, vec![1; 100], 1000).unwrap();
        channel.enqueue(2, 42, vec![2; 100], 1000).unwrap();
        channel.enqueue(3, 42, vec![3; 100], 1000).unwrap();

        let ack = ReplAck {
            up_to_sequence: 2,
            site_id: 43,
        };
        let count = channel.process_ack(ack, 2000);

        assert_eq!(count, 3);
        assert_eq!(channel.acked_sequence(), 2);
        assert_eq!(channel.inflight_count(), 0);
        assert_eq!(channel.inflight_bytes(), 0);
        assert_eq!(channel.stats().entries_acked, 3);
        assert_eq!(channel.stats().bytes_acked, 300);
    }

    #[test]
    fn test_process_ack_partial() {
        let config = ReplChannelConfig::default();
        let mut channel = ReplChannel::new(config);
        channel.set_state(ReplChannelState::Replicating);

        channel.enqueue(1, 42, vec![1; 100], 1000).unwrap();
        channel.enqueue(2, 42, vec![2; 100], 1000).unwrap();
        channel.enqueue(3, 42, vec![3; 100], 1000).unwrap();

        let ack = ReplAck {
            up_to_sequence: 1,
            site_id: 43,
        };
        let count = channel.process_ack(ack, 2000);

        assert_eq!(count, 2);
        assert_eq!(channel.acked_sequence(), 1);
        assert_eq!(channel.inflight_count(), 1);
        assert_eq!(channel.inflight_bytes(), 100);
    }

    #[test]
    fn test_process_ack_already_acked() {
        let config = ReplChannelConfig::default();
        let mut channel = ReplChannel::new(config);
        channel.set_state(ReplChannelState::Replicating);

        channel.enqueue(1, 42, vec![1; 100], 1000).unwrap();

        let ack = ReplAck {
            up_to_sequence: 0,
            site_id: 43,
        };
        let count = channel.process_ack(ack, 2000);
        assert_eq!(count, 1);
        assert_eq!(channel.acked_sequence(), 0);

        let ack2 = ReplAck {
            up_to_sequence: 0,
            site_id: 43,
        };
        let count2 = channel.process_ack(ack2, 3000);
        assert_eq!(count2, 0);
    }

    #[test]
    fn test_timed_out_entries() {
        let config = ReplChannelConfig {
            ack_timeout_ms: 1000,
            ..Default::default()
        };
        let mut channel = ReplChannel::new(config);
        channel.set_state(ReplChannelState::Replicating);

        channel.enqueue(1, 42, vec![1], 1000).unwrap();
        channel.mark_sent(0, 1000).unwrap();

        channel.enqueue(2, 42, vec![2], 1000).unwrap();
        channel.mark_sent(1, 2000).unwrap();

        channel.enqueue(3, 42, vec![3], 1000).unwrap();
        channel.mark_sent(2, 3000).unwrap();

        let timed_out = channel.timed_out_entries(3000);
        assert_eq!(timed_out, vec![0]);

        let timed_out = channel.timed_out_entries(4000);
        let mut sorted = timed_out;
        sorted.sort();
        assert_eq!(sorted, vec![0, 1]);
    }

    #[test]
    fn test_timed_out_entries_none() {
        let config = ReplChannelConfig {
            ack_timeout_ms: 5000,
            ..Default::default()
        };
        let mut channel = ReplChannel::new(config);
        channel.set_state(ReplChannelState::Replicating);

        channel.enqueue(1, 42, vec![1], 1000).unwrap();
        channel.mark_sent(0, 1000).unwrap();

        let timed_out = channel.timed_out_entries(2000);
        assert!(timed_out.is_empty());
    }

    #[test]
    fn test_reconnect_delay_initial() {
        let config = ReplChannelConfig {
            reconnect_backoff_ms: 1000,
            max_reconnect_backoff_ms: 30000,
            ..Default::default()
        };
        let channel = ReplChannel::new(config);

        assert_eq!(channel.reconnect_delay_ms(), 1000);
    }

    #[test]
    fn test_reconnect_delay_exponential() {
        let config = ReplChannelConfig {
            reconnect_backoff_ms: 1000,
            max_reconnect_backoff_ms: 30000,
            ..Default::default()
        };
        let mut channel = ReplChannel::new(config);

        channel.record_reconnect(1000);
        assert_eq!(channel.reconnect_delay_ms(), 2000);

        channel.record_reconnect(2000);
        assert_eq!(channel.reconnect_delay_ms(), 4000);

        channel.record_reconnect(3000);
        assert_eq!(channel.reconnect_delay_ms(), 8000);

        channel.record_reconnect(4000);
        assert_eq!(channel.reconnect_delay_ms(), 16000);
    }

    #[test]
    fn test_reconnect_delay_max() {
        let config = ReplChannelConfig {
            reconnect_backoff_ms: 1000,
            max_reconnect_backoff_ms: 10000,
            ..Default::default()
        };
        let mut channel = ReplChannel::new(config);

        for _ in 0..10 {
            channel.record_reconnect(1000);
        }

        assert_eq!(channel.reconnect_delay_ms(), 10000);
    }

    #[test]
    fn test_record_reconnect() {
        let config = ReplChannelConfig::default();
        let mut channel = ReplChannel::new(config);

        channel.record_reconnect(1000);
        assert_eq!(channel.reconnect_attempts, 1);
        assert_eq!(channel.total_reconnects, 1);
        assert_eq!(channel.last_reconnect_ms, 1000);

        channel.record_reconnect(2000);
        assert_eq!(channel.reconnect_attempts, 2);
        assert_eq!(channel.total_reconnects, 2);
    }

    #[test]
    fn test_reset_reconnect_attempts() {
        let config = ReplChannelConfig::default();
        let mut channel = ReplChannel::new(config);

        channel.record_reconnect(1000);
        channel.record_reconnect(2000);
        assert_eq!(channel.reconnect_attempts, 2);

        channel.reset_reconnect_attempts();
        assert_eq!(channel.reconnect_attempts, 0);
        assert_eq!(channel.total_reconnects, 2);
    }

    #[test]
    fn test_set_state_failed() {
        let config = ReplChannelConfig::default();
        let mut channel = ReplChannel::new(config);

        channel.set_state(ReplChannelState::Failed("connection refused".to_string()));
        assert_eq!(
            channel.state(),
            &ReplChannelState::Failed("connection refused".to_string())
        );
        assert_eq!(channel.stats().total_failures, 1);
    }

    #[test]
    fn test_remove_timed_out() {
        let config = ReplChannelConfig::default();
        let mut channel = ReplChannel::new(config);
        channel.set_state(ReplChannelState::Replicating);

        channel.enqueue(1, 42, vec![1; 100], 1000).unwrap();
        assert_eq!(channel.inflight_bytes(), 100);

        let removed = channel.remove_timed_out(0);
        assert!(removed.is_some());
        assert_eq!(channel.inflight_count(), 0);
        assert_eq!(channel.inflight_bytes(), 0);
    }

    #[test]
    fn test_stats() {
        let config = ReplChannelConfig::default();
        let mut channel = ReplChannel::new(config);
        channel.set_state(ReplChannelState::Replicating);

        channel.enqueue(1, 42, vec![1; 100], 1000).unwrap();
        channel.mark_sent(0, 1000).unwrap();

        let stats = channel.stats();
        assert_eq!(stats.state, "Replicating");
        assert_eq!(stats.entries_sent, 1);
        assert_eq!(stats.bytes_sent, 100);
        assert_eq!(stats.inflight_count, 1);
        assert_eq!(stats.inflight_bytes, 100);
    }

    #[test]
    fn test_stats_snapshot() {
        let config = ReplChannelConfig::default();
        let mut channel = ReplChannel::new(config);
        channel.set_state(ReplChannelState::Replicating);

        channel.enqueue(1, 42, vec![1; 50], 1000).unwrap();

        let snapshot = channel.snapshot();
        assert_eq!(snapshot.inflight_count, 1);
        assert_eq!(snapshot.inflight_bytes, 50);
    }

    #[test]
    fn test_is_ready() {
        let config = ReplChannelConfig {
            max_inflight_bytes: 100,
            ..Default::default()
        };
        let mut channel = ReplChannel::new(config);

        assert!(!channel.is_ready());

        channel.set_state(ReplChannelState::Replicating);
        assert!(channel.is_ready());

        channel.enqueue(1, 42, vec![1; 50], 1000).unwrap();
        assert!(channel.is_ready());

        // 50 + 60 = 110 > 100, should fail backpressure
        let result = channel.enqueue(2, 42, vec![2; 60], 1000);
        assert!(matches!(result, Err(ReplError::Backpressured { .. })));

        // After failed enqueue, should still be ready since we're under limit
        assert!(channel.is_ready());

        // Now enqueue something that fits but brings us close to limit
        channel.enqueue(2, 42, vec![2; 49], 1000).unwrap();
        assert!(channel.is_ready());

        // This would exceed: 50 + 49 + 2 = 101 > 100
        let result = channel.enqueue(3, 42, vec![3; 2], 1000);
        assert!(matches!(result, Err(ReplError::Backpressured { .. })));
    }

    #[test]
    fn test_journal_entry_serde() {
        let entry = JournalEntry {
            sequence: 42,
            site_id: 1,
            shard_id: 5,
            payload: vec![1, 2, 3, 4],
            timestamp_ms: 12345,
        };
        let json = serde_json::to_string(&entry).unwrap();
        let decoded: JournalEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(entry.sequence, decoded.sequence);
        assert_eq!(entry.site_id, decoded.site_id);
        assert_eq!(entry.shard_id, decoded.shard_id);
        assert_eq!(entry.payload, decoded.payload);
        assert_eq!(entry.timestamp_ms, decoded.timestamp_ms);
    }

    #[test]
    fn test_repl_ack_serde() {
        let ack = ReplAck {
            up_to_sequence: 100,
            site_id: 42,
        };
        let json = serde_json::to_string(&ack).unwrap();
        let decoded: ReplAck = serde_json::from_str(&json).unwrap();
        assert_eq!(ack.up_to_sequence, decoded.up_to_sequence);
        assert_eq!(ack.site_id, decoded.site_id);
    }

    #[test]
    fn test_repl_channel_state_serde() {
        let state = ReplChannelState::Replicating;
        let json = serde_json::to_string(&state).unwrap();
        let decoded: ReplChannelState = serde_json::from_str(&json).unwrap();
        assert_eq!(state, decoded);

        let state = ReplChannelState::Failed("test".to_string());
        let json = serde_json::to_string(&state).unwrap();
        let decoded: ReplChannelState = serde_json::from_str(&json).unwrap();
        assert_eq!(state, decoded);
    }

    #[test]
    fn test_config_serde() {
        let config = ReplChannelConfig {
            remote_site: "site-c".to_string(),
            remote_addr: "10.0.3.50:9401".to_string(),
            max_inflight_bytes: 128 * 1024 * 1024,
            max_inflight_entries: 2048,
            ack_timeout_ms: 10000,
            reconnect_backoff_ms: 2000,
            max_reconnect_backoff_ms: 60000,
            bandwidth_limit_bps: Some(100_000_000),
        };
        let json = serde_json::to_string(&config).unwrap();
        let decoded: ReplChannelConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config.remote_site, decoded.remote_site);
        assert_eq!(config.bandwidth_limit_bps, decoded.bandwidth_limit_bps);
    }

    #[test]
    fn test_error_display() {
        let err = ReplError::Backpressured {
            inflight_bytes: 1024,
        };
        assert_eq!(
            format!("{}", err),
            "Channel backpressured: 1024 inflight bytes"
        );

        let err = ReplError::NotReady {
            state: "Idle".to_string(),
        };
        assert_eq!(format!("{}", err), "Channel not ready: state is \"Idle\"");

        let err = ReplError::UnknownSequence { sequence: 42 };
        assert_eq!(format!("{}", err), "Unknown sequence: 42");

        let err = ReplError::BandwidthLimitExceeded;
        assert_eq!(format!("{}", err), "Bandwidth limit exceeded");
    }
}
