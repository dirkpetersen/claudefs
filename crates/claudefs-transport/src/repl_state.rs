//! Journal Replication State Machine.
//!
//! Tracks the state of per-connection journal replication channels.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct JournalSeq(pub u64);

impl JournalSeq {
    pub fn next(self) -> Self {
        JournalSeq(self.0 + 1)
    }

    pub fn is_before(self, other: Self) -> bool {
        self.0 < other.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReplState {
    Idle,
    Syncing,
    Live,
    Disconnected,
    NeedsResync,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalEntryRecord {
    pub seq: JournalSeq,
    pub size_bytes: u32,
    pub written_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplStateConfig {
    pub max_inflight: usize,
    pub max_lag_entries: u64,
    pub connection_timeout_ms: u64,
}

impl Default for ReplStateConfig {
    fn default() -> Self {
        Self {
            max_inflight: 256,
            max_lag_entries: 10000,
            connection_timeout_ms: 10000,
        }
    }
}

pub struct JournalReplChannel {
    config: ReplStateConfig,
    peer_id: [u8; 16],
    state: ReplState,
    local_head: JournalSeq,
    peer_acked: JournalSeq,
    inflight: VecDeque<JournalEntryRecord>,
    last_activity_ms: u64,
    stats: Arc<JournalReplChannelStats>,
}

impl JournalReplChannel {
    pub fn new(peer_id: [u8; 16], config: ReplStateConfig, now_ms: u64) -> Self {
        Self {
            config,
            peer_id,
            state: ReplState::Idle,
            local_head: JournalSeq(0),
            peer_acked: JournalSeq(0),
            inflight: VecDeque::new(),
            last_activity_ms: now_ms,
            stats: Arc::new(JournalReplChannelStats::new()),
        }
    }

    pub fn advance_local(&mut self, entry: JournalEntryRecord, now_ms: u64) -> bool {
        self.last_activity_ms = now_ms;

        let lag = self.lag();
        if lag >= self.config.max_lag_entries as u64 {
            self.state = ReplState::NeedsResync;
            self.stats.resync_events.fetch_add(1, Ordering::Relaxed);
            return false;
        }

        if self.inflight.len() >= self.config.max_inflight {
            return false;
        }

        if entry.seq.0 > self.local_head.0 {
            self.local_head = entry.seq;
        }
        self.inflight.push_back(entry);
        self.stats.entries_sent.fetch_add(1, Ordering::Relaxed);
        true
    }

    pub fn ack(&mut self, seq: JournalSeq, now_ms: u64) {
        self.last_activity_ms = now_ms;

        while let Some(front) = self.inflight.front() {
            if front.seq.is_before(seq) || front.seq == seq {
                self.inflight.pop_front();
            } else {
                break;
            }
        }

        if seq.0 > self.peer_acked.0 {
            self.peer_acked = seq;
        }
        self.stats.entries_acked.fetch_add(1, Ordering::Relaxed);
    }

    pub fn check_timeout(&mut self, now_ms: u64) {
        let elapsed = now_ms.saturating_sub(self.last_activity_ms);
        if elapsed >= self.config.connection_timeout_ms && self.state != ReplState::Disconnected {
            self.state = ReplState::Disconnected;
            self.stats.disconnections.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn connect(&mut self, now_ms: u64) {
        self.last_activity_ms = now_ms;
        if self.state == ReplState::Idle || self.state == ReplState::Disconnected {
            self.state = ReplState::Syncing;
        }
    }

    pub fn mark_live(&mut self, now_ms: u64) {
        self.last_activity_ms = now_ms;
        if self.state == ReplState::Syncing {
            self.state = ReplState::Live;
        }
    }

    pub fn disconnect(&mut self, now_ms: u64) {
        self.last_activity_ms = now_ms;
        self.state = ReplState::Disconnected;
        self.stats.disconnections.fetch_add(1, Ordering::Relaxed);
    }

    pub fn state(&self) -> ReplState {
        self.state
    }

    pub fn lag(&self) -> u64 {
        self.local_head.0.saturating_sub(self.peer_acked.0)
    }

    pub fn inflight_count(&self) -> usize {
        self.inflight.len()
    }

    pub fn is_caught_up(&self) -> bool {
        self.lag() == 0
    }

    pub fn peer_id(&self) -> [u8; 16] {
        self.peer_id
    }

    pub fn stats(&self) -> Arc<JournalReplChannelStats> {
        Arc::clone(&self.stats)
    }
}

pub struct JournalReplChannelStats {
    pub entries_sent: AtomicU64,
    pub entries_acked: AtomicU64,
    pub ack_timeouts: AtomicU64,
    pub disconnections: AtomicU64,
    pub resync_events: AtomicU64,
}

impl JournalReplChannelStats {
    pub fn new() -> Self {
        Self {
            entries_sent: AtomicU64::new(0),
            entries_acked: AtomicU64::new(0),
            ack_timeouts: AtomicU64::new(0),
            disconnections: AtomicU64::new(0),
            resync_events: AtomicU64::new(0),
        }
    }

    pub fn snapshot(
        &self,
        lag: u64,
        inflight: usize,
        state: ReplState,
    ) -> JournalReplChannelStatsSnapshot {
        JournalReplChannelStatsSnapshot {
            entries_sent: self.entries_sent.load(Ordering::Relaxed),
            entries_acked: self.entries_acked.load(Ordering::Relaxed),
            ack_timeouts: self.ack_timeouts.load(Ordering::Relaxed),
            disconnections: self.disconnections.load(Ordering::Relaxed),
            resync_events: self.resync_events.load(Ordering::Relaxed),
            current_lag: lag,
            inflight_count: inflight,
            state,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalReplChannelStatsSnapshot {
    pub entries_sent: u64,
    pub entries_acked: u64,
    pub ack_timeouts: u64,
    pub disconnections: u64,
    pub resync_events: u64,
    pub current_lag: u64,
    pub inflight_count: usize,
    pub state: ReplState,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(seq: u64) -> JournalEntryRecord {
        JournalEntryRecord {
            seq: JournalSeq(seq),
            size_bytes: 1024,
            written_at_ms: 1000,
        }
    }

    #[test]
    fn test_new_channel_idle() {
        let peer_id = [0u8; 16];
        let config = ReplStateConfig::default();
        let channel = JournalReplChannel::new(peer_id, config, 1000);
        assert_eq!(channel.state(), ReplState::Idle);
    }

    #[test]
    fn test_connect_transitions_to_syncing() {
        let peer_id = [0u8; 16];
        let config = ReplStateConfig::default();
        let mut channel = JournalReplChannel::new(peer_id, config, 1000);

        channel.connect(1000);
        assert_eq!(channel.state(), ReplState::Syncing);
    }

    #[test]
    fn test_mark_live_transitions_to_live() {
        let peer_id = [0u8; 16];
        let config = ReplStateConfig::default();
        let mut channel = JournalReplChannel::new(peer_id, config, 1000);

        channel.connect(1000);
        assert_eq!(channel.state(), ReplState::Syncing);
        channel.mark_live(1000);
        assert_eq!(channel.state(), ReplState::Live);
    }

    #[test]
    fn test_advance_local_increments_lag() {
        let peer_id = [0u8; 16];
        let config = ReplStateConfig::default();
        let mut channel = JournalReplChannel::new(peer_id, config, 1000);

        let accepted = channel.advance_local(make_entry(1), 1000);
        assert!(accepted);
        assert_eq!(channel.lag(), 1);
    }

    #[test]
    fn test_ack_reduces_lag() {
        let peer_id = [0u8; 16];
        let config = ReplStateConfig::default();
        let mut channel = JournalReplChannel::new(peer_id, config, 1000);

        channel.advance_local(make_entry(1), 1000);
        assert_eq!(channel.lag(), 1);

        channel.ack(JournalSeq(1), 1000);
        assert_eq!(channel.lag(), 0);
    }

    #[test]
    fn test_ack_cumulative() {
        let peer_id = [0u8; 16];
        let config = ReplStateConfig::default();
        let mut channel = JournalReplChannel::new(peer_id, config, 1000);

        channel.advance_local(make_entry(1), 1000);
        channel.advance_local(make_entry(2), 1000);
        channel.advance_local(make_entry(3), 1000);
        assert_eq!(channel.inflight_count(), 3);

        channel.ack(JournalSeq(2), 1000);
        assert_eq!(channel.inflight_count(), 1);
        assert_eq!(channel.lag(), 1);
    }

    #[test]
    fn test_disconnect_transitions_state() {
        let peer_id = [0u8; 16];
        let config = ReplStateConfig::default();
        let mut channel = JournalReplChannel::new(peer_id, config, 1000);

        channel.connect(1000);
        assert_eq!(channel.state(), ReplState::Syncing);
        channel.disconnect(1000);
        assert_eq!(channel.state(), ReplState::Disconnected);
    }

    #[test]
    fn test_timeout_triggers_disconnect() {
        let peer_id = [0u8; 16];
        let config = ReplStateConfig {
            connection_timeout_ms: 5000,
            ..Default::default()
        };
        let mut channel = JournalReplChannel::new(peer_id, config, 1000);

        channel.connect(1000);
        channel.check_timeout(7000);

        assert_eq!(channel.state(), ReplState::Disconnected);
    }

    #[test]
    fn test_timeout_not_expired() {
        let peer_id = [0u8; 16];
        let config = ReplStateConfig {
            connection_timeout_ms: 5000,
            ..Default::default()
        };
        let mut channel = JournalReplChannel::new(peer_id, config, 1000);

        channel.connect(1000);
        channel.check_timeout(4000);

        assert_eq!(channel.state(), ReplState::Syncing);
    }

    #[test]
    fn test_max_lag_triggers_resync() {
        let peer_id = [0u8; 16];
        let config = ReplStateConfig {
            max_lag_entries: 2,
            ..Default::default()
        };
        let mut channel = JournalReplChannel::new(peer_id, config, 1000);

        channel.advance_local(make_entry(1), 1000);
        channel.advance_local(make_entry(2), 1000);

        let result = channel.advance_local(make_entry(3), 1000);
        assert!(!result);
        assert_eq!(channel.state(), ReplState::NeedsResync);
    }

    #[test]
    fn test_inflight_count() {
        let peer_id = [0u8; 16];
        let config = ReplStateConfig::default();
        let mut channel = JournalReplChannel::new(peer_id, config, 1000);

        assert_eq!(channel.inflight_count(), 0);

        channel.advance_local(make_entry(1), 1000);
        channel.advance_local(make_entry(2), 1000);
        assert_eq!(channel.inflight_count(), 2);

        channel.ack(JournalSeq(1), 1000);
        assert_eq!(channel.inflight_count(), 1);
    }

    #[test]
    fn test_is_caught_up_true() {
        let peer_id = [0u8; 16];
        let config = ReplStateConfig::default();
        let channel = JournalReplChannel::new(peer_id, config, 1000);

        assert!(channel.is_caught_up());
    }

    #[test]
    fn test_is_caught_up_false() {
        let peer_id = [0u8; 16];
        let config = ReplStateConfig::default();
        let mut channel = JournalReplChannel::new(peer_id, config, 1000);

        channel.advance_local(make_entry(1), 1000);

        assert!(!channel.is_caught_up());
    }

    #[test]
    fn test_stats_snapshot() {
        let peer_id = [0u8; 16];
        let config = ReplStateConfig::default();
        let mut channel = JournalReplChannel::new(peer_id, config, 1000);

        channel.advance_local(make_entry(1), 1000);
        channel.advance_local(make_entry(2), 1000);
        channel.ack(JournalSeq(1), 1000);

        let snapshot =
            channel
                .stats()
                .snapshot(channel.lag(), channel.inflight_count(), channel.state());
        assert_eq!(snapshot.entries_sent, 2);
        assert_eq!(snapshot.entries_acked, 1);
        assert_eq!(snapshot.current_lag, 1);
        assert_eq!(snapshot.inflight_count, 1);
    }

    #[test]
    fn test_journal_seq_ordering() {
        let s1 = JournalSeq(1);
        let s2 = JournalSeq(2);
        let s3 = JournalSeq(3);

        assert!(s1.is_before(s2));
        assert!(s1.is_before(s3));
        assert!(!s2.is_before(s1));
        assert!(s2.is_before(s3));

        assert_eq!(s1.next(), JournalSeq(2));
        assert_eq!(s2.next(), JournalSeq(3));
    }
}
