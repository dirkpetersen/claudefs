//! WORM compliance audit trail for regulatory requirements.
//!
//! Logs WORM policy events (policy set, hold placed, hold released, expiry checked,
//! GC suppressed) for compliance audit trails. Uses an in-memory ring buffer.

use crate::worm_reducer::WormMode;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use tracing::debug;

/// Type of WORM audit event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuditEventKind {
    /// A retention policy was set on a chunk
    PolicySet {
        /// The WORM mode that was set
        mode: WormMode,
    },
    /// A legal hold was placed
    HoldPlaced,
    /// A legal hold was released
    HoldReleased,
    /// An expiry check was performed
    ExpiryChecked {
        /// Whether the chunk was found to be expired
        expired: bool,
    },
    /// GC was suppressed due to active retention
    GcSuppressed,
    /// A policy was removed (retention period ended)
    PolicyRemoved,
}

/// A single WORM audit event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    /// Monotonically increasing sequence number
    pub seq: u64,
    /// Unix timestamp when the event occurred
    pub timestamp_ts: u64,
    /// The chunk this event pertains to
    pub chunk_id: u64,
    /// What happened
    pub kind: AuditEventKind,
}

/// Configuration for the audit log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditLogConfig {
    /// Maximum number of events to retain (ring buffer)
    pub max_events: usize,
    /// Whether audit logging is enabled
    pub enabled: bool,
}

impl Default for AuditLogConfig {
    fn default() -> Self {
        Self {
            max_events: 10000,
            enabled: true,
        }
    }
}

/// Ring-buffer audit log for WORM compliance events.
pub struct AuditLog {
    config: AuditLogConfig,
    events: VecDeque<AuditEvent>,
    next_seq: u64,
}

impl AuditLog {
    /// Creates a new audit log with the given configuration.
    pub fn new(config: AuditLogConfig) -> Self {
        Self {
            config,
            events: VecDeque::with_capacity(1024),
            next_seq: 1,
        }
    }

    /// Record an audit event. No-op if disabled.
    pub fn record(&mut self, chunk_id: u64, now_ts: u64, kind: AuditEventKind) {
        if !self.config.enabled {
            return;
        }

        let event = AuditEvent {
            seq: self.next_seq,
            timestamp_ts: now_ts,
            chunk_id,
            kind,
        };
        self.next_seq += 1;

        if self.events.len() >= self.config.max_events {
            let evicted = self.events.pop_front();
            if let Some(ev) = evicted {
                debug!("ring buffer full, evicted oldest event seq {}", ev.seq);
            }
        }

        self.events.push_back(event);
        debug!(
            "recorded audit event for chunk {} at ts {}",
            chunk_id, now_ts
        );
    }

    /// Returns all events in order (oldest first).
    pub fn events(&self) -> impl Iterator<Item = &AuditEvent> {
        self.events.iter()
    }

    /// Returns events for a specific chunk (oldest first).
    pub fn events_for_chunk(&self, chunk_id: u64) -> Vec<&AuditEvent> {
        self.events
            .iter()
            .filter(|e| e.chunk_id == chunk_id)
            .collect()
    }

    /// Returns the total number of events in the log.
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Returns true if the log is empty.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Clears all events.
    pub fn clear(&mut self) {
        self.events.clear();
        debug!("audit log cleared");
    }

    /// Returns events since a given sequence number (exclusive).
    pub fn events_since(&self, seq: u64) -> Vec<&AuditEvent> {
        self.events.iter().filter(|e| e.seq > seq).collect()
    }

    /// Returns the sequence number of the most recent event, or None.
    pub fn last_seq(&self) -> Option<u64> {
        self.events.back().map(|e| e.seq)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_log_is_empty() {
        let log = AuditLog::new(AuditLogConfig::default());
        assert!(log.is_empty());
        assert_eq!(log.len(), 0);
    }

    #[test]
    fn test_record_disabled_noop() {
        let config = AuditLogConfig {
            max_events: 100,
            enabled: false,
        };
        let mut log = AuditLog::new(config);

        log.record(
            1,
            1000,
            AuditEventKind::PolicySet {
                mode: WormMode::None,
            },
        );

        assert!(log.is_empty());
        assert_eq!(log.len(), 0);
    }

    #[test]
    fn test_record_single_event() {
        let mut log = AuditLog::new(AuditLogConfig::default());

        log.record(
            1,
            1000,
            AuditEventKind::PolicySet {
                mode: WormMode::None,
            },
        );

        assert!(!log.is_empty());
        assert_eq!(log.len(), 1);
    }

    #[test]
    fn test_record_policy_set() {
        let mut log = AuditLog::new(AuditLogConfig::default());

        log.record(
            1,
            1000,
            AuditEventKind::PolicySet {
                mode: WormMode::Immutable,
            },
        );

        let events: Vec<_> = log.events().collect();
        assert_eq!(events.len(), 1);

        let event = events[0];
        assert_eq!(event.chunk_id, 1);
        assert_eq!(event.timestamp_ts, 1000);
        assert_eq!(
            event.kind,
            AuditEventKind::PolicySet {
                mode: WormMode::Immutable
            }
        );
    }

    #[test]
    fn test_record_hold_placed_and_released() {
        let mut log = AuditLog::new(AuditLogConfig::default());

        log.record(1, 1000, AuditEventKind::HoldPlaced);
        log.record(1, 2000, AuditEventKind::HoldReleased);

        let events: Vec<_> = log.events().collect();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].kind, AuditEventKind::HoldPlaced);
        assert_eq!(events[1].kind, AuditEventKind::HoldReleased);
    }

    #[test]
    fn test_events_for_chunk_filters() {
        let mut log = AuditLog::new(AuditLogConfig::default());

        log.record(1, 1000, AuditEventKind::HoldPlaced);
        log.record(2, 1000, AuditEventKind::HoldPlaced);
        log.record(1, 2000, AuditEventKind::HoldReleased);
        log.record(
            3,
            1000,
            AuditEventKind::PolicySet {
                mode: WormMode::Immutable,
            },
        );

        let chunk1_events = log.events_for_chunk(1);
        assert_eq!(chunk1_events.len(), 2);

        let chunk2_events = log.events_for_chunk(2);
        assert_eq!(chunk2_events.len(), 1);

        let chunk4_events = log.events_for_chunk(4);
        assert!(chunk4_events.is_empty());
    }

    #[test]
    fn test_seq_monotonically_increasing() {
        let mut log = AuditLog::new(AuditLogConfig::default());

        log.record(1, 1000, AuditEventKind::HoldPlaced);
        log.record(2, 1000, AuditEventKind::HoldPlaced);
        log.record(3, 1000, AuditEventKind::HoldPlaced);

        let events: Vec<_> = log.events().collect();
        assert_eq!(events[0].seq, 1);
        assert_eq!(events[1].seq, 2);
        assert_eq!(events[2].seq, 3);
    }

    #[test]
    fn test_ring_buffer_eviction() {
        let config = AuditLogConfig {
            max_events: 3,
            enabled: true,
        };
        let mut log = AuditLog::new(config);

        log.record(1, 1000, AuditEventKind::HoldPlaced);
        log.record(2, 2000, AuditEventKind::HoldPlaced);
        log.record(3, 3000, AuditEventKind::HoldPlaced);
        log.record(4, 4000, AuditEventKind::HoldPlaced);

        assert_eq!(log.len(), 3);

        let events: Vec<_> = log.events().collect();
        assert_eq!(events[0].seq, 2);
        assert_eq!(events[1].seq, 3);
        assert_eq!(events[2].seq, 4);

        assert_eq!(events[0].chunk_id, 2);
    }

    #[test]
    fn test_events_since() {
        let mut log = AuditLog::new(AuditLogConfig::default());

        log.record(1, 1000, AuditEventKind::HoldPlaced);
        log.record(2, 2000, AuditEventKind::HoldPlaced);
        log.record(3, 3000, AuditEventKind::HoldPlaced);
        log.record(4, 4000, AuditEventKind::HoldPlaced);

        let since = log.events_since(2);
        assert_eq!(since.len(), 2);
        assert_eq!(since[0].seq, 3);
        assert_eq!(since[1].seq, 4);
    }

    #[test]
    fn test_events_since_all() {
        let mut log = AuditLog::new(AuditLogConfig::default());

        log.record(1, 1000, AuditEventKind::HoldPlaced);
        log.record(2, 2000, AuditEventKind::HoldPlaced);
        log.record(3, 3000, AuditEventKind::HoldPlaced);

        let since = log.events_since(0);
        assert_eq!(since.len(), 3);
    }

    #[test]
    fn test_clear_resets_log() {
        let mut log = AuditLog::new(AuditLogConfig::default());

        log.record(1, 1000, AuditEventKind::HoldPlaced);
        log.record(2, 2000, AuditEventKind::HoldPlaced);

        log.clear();

        assert!(log.is_empty());
        assert_eq!(log.len(), 0);
    }

    #[test]
    fn test_clear_does_not_reset_next_seq() {
        let mut log = AuditLog::new(AuditLogConfig::default());

        log.record(1, 1000, AuditEventKind::HoldPlaced);
        log.clear();
        log.record(2, 2000, AuditEventKind::HoldPlaced);

        let events: Vec<_> = log.events().collect();
        assert_eq!(events[0].seq, 2);
    }

    #[test]
    fn test_last_seq_none_when_empty() {
        let log = AuditLog::new(AuditLogConfig::default());
        assert_eq!(log.last_seq(), None);
    }

    #[test]
    fn test_last_seq_after_events() {
        let mut log = AuditLog::new(AuditLogConfig::default());

        log.record(1, 1000, AuditEventKind::HoldPlaced);
        log.record(2, 2000, AuditEventKind::HoldPlaced);
        log.record(3, 3000, AuditEventKind::HoldPlaced);

        assert_eq!(log.last_seq(), Some(3));
    }

    #[test]
    fn test_gc_suppressed_event() {
        let mut log = AuditLog::new(AuditLogConfig::default());

        log.record(1, 1000, AuditEventKind::GcSuppressed);

        let events: Vec<_> = log.events().collect();
        assert_eq!(events[0].kind, AuditEventKind::GcSuppressed);
    }

    #[test]
    fn test_expiry_checked_event() {
        let mut log = AuditLog::new(AuditLogConfig::default());

        log.record(1, 1000, AuditEventKind::ExpiryChecked { expired: true });
        log.record(2, 2000, AuditEventKind::ExpiryChecked { expired: false });

        let events: Vec<_> = log.events().collect();
        assert_eq!(
            events[0].kind,
            AuditEventKind::ExpiryChecked { expired: true }
        );
        assert_eq!(
            events[1].kind,
            AuditEventKind::ExpiryChecked { expired: false }
        );
    }

    #[test]
    fn test_audit_log_config_default() {
        let config = AuditLogConfig::default();
        assert_eq!(config.max_events, 10000);
        assert!(config.enabled);
    }

    #[test]
    fn test_policy_removed_event() {
        let mut log = AuditLog::new(AuditLogConfig::default());

        log.record(1, 1000, AuditEventKind::PolicyRemoved);

        let events: Vec<_> = log.events().collect();
        assert_eq!(events[0].kind, AuditEventKind::PolicyRemoved);
    }

    #[test]
    fn test_multiple_event_types() {
        let mut log = AuditLog::new(AuditLogConfig::default());

        log.record(
            1,
            1000,
            AuditEventKind::PolicySet {
                mode: WormMode::Immutable,
            },
        );
        log.record(1, 2000, AuditEventKind::HoldPlaced);
        log.record(1, 3000, AuditEventKind::ExpiryChecked { expired: false });
        log.record(1, 4000, AuditEventKind::GcSuppressed);
        log.record(1, 5000, AuditEventKind::HoldReleased);
        log.record(1, 6000, AuditEventKind::PolicyRemoved);

        let events: Vec<_> = log.events().collect();
        assert_eq!(events.len(), 6);
    }

    #[test]
    fn test_events_preserve_order() {
        let mut log = AuditLog::new(AuditLogConfig::default());

        log.record(1, 100, AuditEventKind::HoldPlaced);
        log.record(2, 200, AuditEventKind::HoldPlaced);
        log.record(3, 300, AuditEventKind::HoldPlaced);

        let events: Vec<_> = log.events().collect();
        assert_eq!(events[0].timestamp_ts, 100);
        assert_eq!(events[1].timestamp_ts, 200);
        assert_eq!(events[2].timestamp_ts, 300);
    }

    #[test]
    fn test_audit_event_clone() {
        let event = AuditEvent {
            seq: 1,
            timestamp_ts: 1000,
            chunk_id: 42,
            kind: AuditEventKind::HoldPlaced,
        };
        let cloned = event.clone();
        assert_eq!(cloned.seq, 1);
        assert_eq!(cloned.chunk_id, 42);
    }

    #[test]
    fn test_audit_event_kind_equality() {
        assert_eq!(
            AuditEventKind::PolicySet {
                mode: WormMode::Immutable
            },
            AuditEventKind::PolicySet {
                mode: WormMode::Immutable
            }
        );
        assert_ne!(
            AuditEventKind::PolicySet {
                mode: WormMode::Immutable
            },
            AuditEventKind::PolicySet {
                mode: WormMode::None
            }
        );
        assert_eq!(AuditEventKind::HoldPlaced, AuditEventKind::HoldPlaced);
        assert_ne!(AuditEventKind::HoldPlaced, AuditEventKind::HoldReleased);
    }

    #[test]
    fn test_ring_buffer_exact_capacity() {
        let config = AuditLogConfig {
            max_events: 5,
            enabled: true,
        };
        let mut log = AuditLog::new(config);

        for i in 1..=5 {
            log.record(i, i * 100, AuditEventKind::HoldPlaced);
        }

        assert_eq!(log.len(), 5);
        let events: Vec<_> = log.events().collect();
        assert_eq!(events[0].seq, 1);
        assert_eq!(events[4].seq, 5);
    }

    #[test]
    fn test_events_since_with_no_matches() {
        let mut log = AuditLog::new(AuditLogConfig::default());

        log.record(1, 1000, AuditEventKind::HoldPlaced);
        log.record(2, 2000, AuditEventKind::HoldPlaced);

        let since = log.events_since(100);
        assert!(since.is_empty());
    }

    #[test]
    fn test_config_clone() {
        let config = AuditLogConfig {
            max_events: 500,
            enabled: false,
        };
        let cloned = config.clone();
        assert_eq!(cloned.max_events, 500);
        assert!(!cloned.enabled);
    }

    #[test]
    fn test_large_number_of_events() {
        let mut log = AuditLog::new(AuditLogConfig::default());

        for i in 0..1000 {
            log.record(i, i, AuditEventKind::HoldPlaced);
        }

        assert_eq!(log.len(), 1000);
        assert_eq!(log.last_seq(), Some(1000));
    }

    #[test]
    fn test_worm_mode_in_policy_set() {
        let mut log = AuditLog::new(AuditLogConfig::default());

        log.record(
            1,
            1000,
            AuditEventKind::PolicySet {
                mode: WormMode::None,
            },
        );
        log.record(
            2,
            1000,
            AuditEventKind::PolicySet {
                mode: WormMode::Immutable,
            },
        );
        log.record(
            3,
            1000,
            AuditEventKind::PolicySet {
                mode: WormMode::LegalHold,
            },
        );

        let events: Vec<_> = log.events().collect();
        assert_eq!(
            events[0].kind,
            AuditEventKind::PolicySet {
                mode: WormMode::None
            }
        );
        assert_eq!(
            events[1].kind,
            AuditEventKind::PolicySet {
                mode: WormMode::Immutable
            }
        );
        assert_eq!(
            events[2].kind,
            AuditEventKind::PolicySet {
                mode: WormMode::LegalHold
            }
        );
    }
}
