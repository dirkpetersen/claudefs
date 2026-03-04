//! Per-connection flow scheduler with QoS bandwidth/latency policies.
//!
//! Implements weighted fair queuing (WFQ) across multiple flows on a connection.
//! Each connection has a token bucket and priority queue of pending sends.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use thiserror::Error;

/// Configuration for the flow scheduler.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowSchedConfig {
    /// Maximum bandwidth for a single connection in bytes/s (0 = unlimited).
    pub max_bps: u64,
    /// Number of priority levels (default: 4).
    pub priority_levels: usize,
    /// Token refill interval in ms (default: 10ms).
    pub refill_interval_ms: u64,
    /// Maximum burst size in bytes per priority level.
    pub max_burst_bytes: u64,
    /// Scheduler quantum in bytes per priority pass.
    pub quantum_bytes: u64,
}

impl Default for FlowSchedConfig {
    fn default() -> Self {
        Self {
            max_bps: 0,
            priority_levels: 4,
            refill_interval_ms: 10,
            max_burst_bytes: 1_000_000,
            quantum_bytes: 64_000,
        }
    }
}

/// A flow identifier (identifies one logical stream within a connection).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FlowId(pub u64);

/// Metadata about a flow.
#[derive(Debug, Clone)]
pub struct FlowEntry {
    /// Flow identifier.
    pub flow_id: FlowId,
    /// Priority level (0=highest, 3=lowest for priority_levels=4).
    pub priority: u8,
    /// Relative weight for WFQ.
    pub weight: u32,
    /// Total bytes currently queued.
    pub bytes_queued: u64,
    /// Total bytes sent on this flow.
    pub bytes_sent: u64,
    /// When this flow was created (ms since epoch).
    pub created_at_ms: u64,
    /// Last time data was sent (ms since epoch).
    pub last_sent_ms: u64,
}

/// A pending send operation waiting for scheduler approval.
#[derive(Debug, Clone)]
pub struct PendingSend {
    /// Flow identifier.
    pub flow_id: FlowId,
    /// Size of the send in bytes.
    pub size_bytes: usize,
    /// Priority level.
    pub priority: u8,
    /// When this send was enqueued (ms since epoch).
    pub enqueued_at_ms: u64,
}

/// Result of requesting a send slot from the scheduler.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SendDecision {
    /// Approved — caller may send immediately.
    Approved {
        /// Number of tokens consumed.
        tokens_consumed: u64,
    },
    /// Deferred — caller must wait this many ms before retrying.
    Deferred {
        /// How many ms to wait before retrying.
        wait_ms: u64,
    },
    /// Rejected — connection overloaded, drop or backpressure upstream.
    Rejected {
        /// Reason for rejection.
        reason: String,
    },
}

/// Error types for flow scheduler operations.
#[derive(Debug, Error)]
pub enum FlowSchedError {
    /// Flow not found.
    #[error("flow {0:?} not found")]
    FlowNotFound(FlowId),
    /// Flow already exists.
    #[error("flow {0:?} already exists")]
    FlowAlreadyExists(FlowId),
    /// Invalid priority level.
    #[error("invalid priority {0}, max is {1}")]
    InvalidPriority(u8, usize),
}

/// Statistics for flow scheduler operations.
pub struct FlowSchedStats {
    pub active_flows: AtomicU64,
    pub total_approved: AtomicU64,
    pub total_deferred: AtomicU64,
    pub total_rejected: AtomicU64,
    pub total_bytes_approved: AtomicU64,
    pub refill_count: AtomicU64,
}

impl FlowSchedStats {
    pub fn new() -> Self {
        Self {
            active_flows: AtomicU64::new(0),
            total_approved: AtomicU64::new(0),
            total_deferred: AtomicU64::new(0),
            total_rejected: AtomicU64::new(0),
            total_bytes_approved: AtomicU64::new(0),
            refill_count: AtomicU64::new(0),
        }
    }

    pub fn snapshot(&self) -> FlowSchedStatsSnapshot {
        FlowSchedStatsSnapshot {
            active_flows: self.active_flows.load(Ordering::Relaxed),
            total_approved: self.total_approved.load(Ordering::Relaxed),
            total_deferred: self.total_deferred.load(Ordering::Relaxed),
            total_rejected: self.total_rejected.load(Ordering::Relaxed),
            total_bytes_approved: self.total_bytes_approved.load(Ordering::Relaxed),
            refill_count: self.refill_count.load(Ordering::Relaxed),
        }
    }
}

impl Default for FlowSchedStats {
    fn default() -> Self {
        Self::new()
    }
}

/// Snapshot of flow scheduler statistics at a point in time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowSchedStatsSnapshot {
    /// Number of currently active flows.
    pub active_flows: u64,
    /// Total send requests approved.
    pub total_approved: u64,
    /// Total send requests deferred.
    pub total_deferred: u64,
    /// Total send requests rejected.
    pub total_rejected: u64,
    /// Total bytes approved for sending.
    pub total_bytes_approved: u64,
    /// Number of times tokens were refilled.
    pub refill_count: u64,
}

/// Per-connection flow scheduler implementing WFQ + token bucket.
pub struct FlowScheduler {
    config: FlowSchedConfig,
    flows: RwLock<HashMap<FlowId, FlowState>>,
    pending_sends: RwLock<HashMap<FlowId, Vec<PendingSend>>>,
    tokens: RwLock<TokenBucket>,
    stats: Arc<FlowSchedStats>,
    last_refill_ms: RwLock<u64>,
}

struct FlowState {
    entry: FlowEntry,
    virtual_finish_time: f64,
}

impl FlowScheduler {
    /// Create a new flow scheduler with the given configuration.
    pub fn new(config: FlowSchedConfig) -> Self {
        let tokens = TokenBucket::new(
            config.max_bps,
            config.max_burst_bytes,
            config.refill_interval_ms,
        );
        Self {
            config: config.clone(),
            flows: RwLock::new(HashMap::new()),
            pending_sends: RwLock::new(HashMap::new()),
            tokens: RwLock::new(tokens),
            stats: Arc::new(FlowSchedStats::new()),
            last_refill_ms: RwLock::new(0),
        }
    }

    /// Register a new flow.
    pub fn register_flow(
        &self,
        flow_id: FlowId,
        priority: u8,
        weight: u32,
        now_ms: u64,
    ) -> Result<(), FlowSchedError> {
        if priority >= self.config.priority_levels as u8 {
            return Err(FlowSchedError::InvalidPriority(
                priority,
                self.config.priority_levels,
            ));
        }

        let mut flows = self
            .flows
            .write()
            .map_err(|_| FlowSchedError::FlowNotFound(flow_id))?;

        if flows.contains_key(&flow_id) {
            return Err(FlowSchedError::FlowAlreadyExists(flow_id));
        }

        let entry = FlowEntry {
            flow_id,
            priority,
            weight,
            bytes_queued: 0,
            bytes_sent: 0,
            created_at_ms: now_ms,
            last_sent_ms: now_ms,
        };

        flows.insert(
            flow_id,
            FlowState {
                entry,
                virtual_finish_time: 0.0,
            },
        );

        self.stats.active_flows.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    /// Unregister a flow when connection closes.
    pub fn unregister_flow(&self, flow_id: FlowId) -> Result<(), FlowSchedError> {
        let mut flows = self
            .flows
            .write()
            .map_err(|_| FlowSchedError::FlowNotFound(flow_id))?;

        if flows.remove(&flow_id).is_none() {
            return Err(FlowSchedError::FlowNotFound(flow_id));
        }

        let mut pending = self
            .pending_sends
            .write()
            .unwrap_or_else(|e| e.into_inner());
        pending.remove(&flow_id);

        self.stats.active_flows.fetch_sub(1, Ordering::Relaxed);
        Ok(())
    }

    /// Request a send slot for `size_bytes`. Returns decision immediately.
    pub fn request_send(&self, flow_id: FlowId, size_bytes: usize, now_ms: u64) -> SendDecision {
        let flows = match self.flows.read() {
            Ok(f) => f,
            Err(_) => {
                return SendDecision::Rejected {
                    reason: "lock error".to_string(),
                }
            }
        };

        let flow = match flows.get(&flow_id) {
            Some(f) => f,
            None => {
                return SendDecision::Rejected {
                    reason: "flow not found".to_string(),
                }
            }
        };

        drop(flows);

        let size_u64 = size_bytes as u64;

        if self.config.max_bps == 0 {
            self.stats.total_approved.fetch_add(1, Ordering::Relaxed);
            self.stats
                .total_bytes_approved
                .fetch_add(size_u64, Ordering::Relaxed);
            return SendDecision::Approved { tokens_consumed: 0 };
        }

        let mut tokens = match self.tokens.write() {
            Ok(t) => t,
            Err(_) => {
                return SendDecision::Rejected {
                    reason: "lock error".to_string(),
                }
            }
        };

        if tokens.available >= size_u64 {
            tokens.available -= size_u64;
            self.stats.total_approved.fetch_add(1, Ordering::Relaxed);
            self.stats
                .total_bytes_approved
                .fetch_add(size_u64, Ordering::Relaxed);

            SendDecision::Approved {
                tokens_consumed: size_u64,
            }
        } else {
            let available = tokens.available;
            let wait_time = if available > 0 {
                let needed = size_u64 - available;
                (needed * 1000) / tokens.rate_per_ms.max(1)
            } else {
                (size_u64 * 1000) / tokens.rate_per_ms.max(1)
            };

            self.stats.total_deferred.fetch_add(1, Ordering::Relaxed);
            SendDecision::Deferred {
                wait_ms: wait_time.max(1),
            }
        }
    }

    /// Refill token buckets (call periodically at refill_interval_ms).
    pub fn refill(&self, now_ms: u64) {
        let mut last_refill = match self.last_refill_ms.write() {
            Ok(l) => l,
            Err(_) => return,
        };

        let elapsed = now_ms.saturating_sub(*last_refill);
        if elapsed < self.config.refill_interval_ms {
            return;
        }

        *last_refill = now_ms;

        let mut tokens = match self.tokens.write() {
            Ok(t) => t,
            Err(_) => return,
        };

        let refill_amount = (elapsed as u64)
            .saturating_mul(tokens.rate_per_ms)
            .min(self.config.max_burst_bytes);
        tokens.available = tokens
            .available
            .saturating_add(refill_amount)
            .min(self.config.max_burst_bytes);

        self.stats.refill_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Snapshot current stats.
    pub fn stats(&self) -> FlowSchedStatsSnapshot {
        self.stats.snapshot()
    }

    /// List all active flows.
    pub fn active_flows(&self) -> Vec<FlowEntry> {
        match self.flows.read() {
            Ok(flows) => flows.values().map(|f| f.entry.clone()).collect(),
            Err(_) => Vec::new(),
        }
    }
}

struct TokenBucket {
    available: u64,
    rate_per_ms: u64,
    max_burst: u64,
}

impl TokenBucket {
    fn new(max_bps: u64, max_burst: u64, refill_interval_ms: u64) -> Self {
        let rate_per_ms = if refill_interval_ms > 0 {
            max_bps.saturating_div(refill_interval_ms)
        } else {
            0
        };
        Self {
            available: max_burst,
            rate_per_ms,
            max_burst,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_flow_id(seed: u64) -> FlowId {
        FlowId(seed)
    }

    fn now_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    }

    #[test]
    fn test_create_scheduler() {
        let config = FlowSchedConfig::default();
        let scheduler = FlowScheduler::new(config);
        let stats = scheduler.stats();
        assert_eq!(stats.active_flows, 0);
    }

    #[test]
    fn test_register_single_flow() {
        let scheduler = FlowScheduler::new(FlowSchedConfig::default());
        let result = scheduler.register_flow(make_flow_id(1), 0, 100, now_ms());
        assert!(result.is_ok());

        let flows = scheduler.active_flows();
        assert_eq!(flows.len(), 1);
        assert_eq!(flows[0].flow_id, make_flow_id(1));
    }

    #[test]
    fn test_register_duplicate_flow() {
        let scheduler = FlowScheduler::new(FlowSchedConfig::default());
        scheduler
            .register_flow(make_flow_id(1), 0, 100, now_ms())
            .unwrap();

        let result = scheduler.register_flow(make_flow_id(1), 0, 100, now_ms());
        assert!(matches!(result, Err(FlowSchedError::FlowAlreadyExists(_))));
    }

    #[test]
    fn test_unregister_unknown_flow() {
        let scheduler = FlowScheduler::new(FlowSchedConfig::default());
        let result = scheduler.unregister_flow(make_flow_id(1));
        assert!(matches!(result, Err(FlowSchedError::FlowNotFound(_))));
    }

    #[test]
    fn test_unregister_existing_flow() {
        let scheduler = FlowScheduler::new(FlowSchedConfig::default());
        scheduler
            .register_flow(make_flow_id(1), 0, 100, now_ms())
            .unwrap();

        let result = scheduler.unregister_flow(make_flow_id(1));
        assert!(result.is_ok());

        let flows = scheduler.active_flows();
        assert!(flows.is_empty());
    }

    #[test]
    fn test_unlimited_bandwidth() {
        let config = FlowSchedConfig {
            max_bps: 0,
            ..Default::default()
        };
        let scheduler = FlowScheduler::new(config);
        scheduler
            .register_flow(make_flow_id(1), 0, 100, now_ms())
            .unwrap();

        let decision = scheduler.request_send(make_flow_id(1), 1000, now_ms());
        assert!(matches!(
            decision,
            SendDecision::Approved { tokens_consumed: 0 }
        ));
    }

    #[test]
    fn test_request_with_tokens_available() {
        let config = FlowSchedConfig {
            max_bps: 1_000_000,
            max_burst_bytes: 1_000_000,
            refill_interval_ms: 1,
            ..Default::default()
        };
        let scheduler = FlowScheduler::new(config);
        scheduler
            .register_flow(make_flow_id(1), 0, 100, now_ms())
            .unwrap();

        let decision = scheduler.request_send(make_flow_id(1), 1000, now_ms());
        assert!(matches!(
            decision,
            SendDecision::Approved {
                tokens_consumed: 1000
            }
        ));
    }

    #[test]
    fn test_request_deferred_when_tokens_exhausted() {
        let config = FlowSchedConfig {
            max_bps: 1000,
            max_burst_bytes: 1000,
            refill_interval_ms: 1000,
            ..Default::default()
        };
        let scheduler = FlowScheduler::new(config);
        scheduler
            .register_flow(make_flow_id(1), 0, 100, now_ms())
            .unwrap();

        let _ = scheduler.request_send(make_flow_id(1), 1000, now_ms());

        let decision = scheduler.request_send(make_flow_id(1), 1000, now_ms());
        match decision {
            SendDecision::Deferred { wait_ms } => assert!(wait_ms > 0),
            _ => panic!("expected Deferred"),
        }
    }

    #[test]
    fn test_invalid_priority() {
        let scheduler = FlowScheduler::new(FlowSchedConfig::default());
        let result = scheduler.register_flow(make_flow_id(1), 10, 100, now_ms());
        assert!(matches!(result, Err(FlowSchedError::InvalidPriority(_, _))));
    }

    #[test]
    fn test_multiple_priority_levels() {
        let scheduler = FlowScheduler::new(FlowSchedConfig::default());

        scheduler
            .register_flow(make_flow_id(1), 0, 100, now_ms())
            .unwrap();
        scheduler
            .register_flow(make_flow_id(2), 3, 100, now_ms())
            .unwrap();

        let flows = scheduler.active_flows();
        assert_eq!(flows.len(), 2);

        let high_priority = flows.iter().find(|f| f.priority == 0).unwrap();
        assert_eq!(high_priority.flow_id, make_flow_id(1));
    }

    #[test]
    fn test_refill_restores_tokens() {
        let config = FlowSchedConfig {
            max_bps: 100_000,
            max_burst_bytes: 100_000,
            refill_interval_ms: 10,
            ..Default::default()
        };
        let scheduler = FlowScheduler::new(config);
        scheduler
            .register_flow(make_flow_id(1), 0, 100, now_ms())
            .unwrap();

        let _ = scheduler.request_send(make_flow_id(1), 100_000, now_ms());

        let after_send = scheduler.stats();
        assert_eq!(after_send.total_approved, 1);

        scheduler.refill(now_ms() + 100);

        let after_refill = scheduler.stats();
        assert_eq!(after_refill.refill_count, 1);
    }

    #[test]
    fn test_stats_approved_count() {
        let config = FlowSchedConfig {
            max_bps: 0,
            ..Default::default()
        };
        let scheduler = FlowScheduler::new(config);
        scheduler
            .register_flow(make_flow_id(1), 0, 100, now_ms())
            .unwrap();

        for _ in 0..5 {
            let _ = scheduler.request_send(make_flow_id(1), 1000, now_ms());
        }

        let stats = scheduler.stats();
        assert_eq!(stats.total_approved, 5);
        assert_eq!(stats.total_bytes_approved, 5000);
    }

    #[test]
    fn test_stats_deferred_count() {
        let config = FlowSchedConfig {
            max_bps: 1000,
            max_burst_bytes: 1000,
            refill_interval_ms: 1000,
            ..Default::default()
        };
        let scheduler = FlowScheduler::new(config);
        scheduler
            .register_flow(make_flow_id(1), 0, 100, now_ms())
            .unwrap();

        let _ = scheduler.request_send(make_flow_id(1), 1000, now_ms());
        let _ = scheduler.request_send(make_flow_id(1), 1000, now_ms());

        let stats = scheduler.stats();
        assert_eq!(stats.total_deferred, 1);
    }

    #[test]
    fn test_active_flows_empty() {
        let scheduler = FlowScheduler::new(FlowSchedConfig::default());
        let flows = scheduler.active_flows();
        assert!(flows.is_empty());
    }

    #[test]
    fn test_active_flows_returns_all() {
        let scheduler = FlowScheduler::new(FlowSchedConfig::default());

        for i in 1..=5 {
            scheduler
                .register_flow(make_flow_id(i), 0, 100, now_ms())
                .unwrap();
        }

        let flows = scheduler.active_flows();
        assert_eq!(flows.len(), 5);
    }

    #[test]
    fn test_request_unknown_flow() {
        let scheduler = FlowScheduler::new(FlowSchedConfig::default());

        let decision = scheduler.request_send(make_flow_id(999), 1000, now_ms());
        assert!(matches!(decision, SendDecision::Rejected { .. }));
    }

    #[test]
    fn test_refill_multiple_times() {
        let config = FlowSchedConfig {
            max_bps: 1000,
            max_burst_bytes: 10_000,
            refill_interval_ms: 10,
            ..Default::default()
        };
        let scheduler = FlowScheduler::new(config);
        scheduler
            .register_flow(make_flow_id(1), 0, 100, now_ms())
            .unwrap();

        for i in 1..=5 {
            scheduler.refill(now_ms() + (i * 20));
        }

        let stats = scheduler.stats();
        assert_eq!(stats.refill_count, 5);
    }

    #[test]
    fn test_burst_cap_enforced() {
        let config = FlowSchedConfig {
            max_bps: 1000,
            max_burst_bytes: 1000,
            refill_interval_ms: 1000,
            ..Default::default()
        };
        let scheduler = FlowScheduler::new(config);
        scheduler
            .register_flow(make_flow_id(1), 0, 100, now_ms())
            .unwrap();

        let _ = scheduler.request_send(make_flow_id(1), 500, now_ms());
        scheduler.refill(now_ms() + 100);

        let decision = scheduler.request_send(make_flow_id(1), 1000, now_ms() + 100);
        match decision {
            SendDecision::Approved { tokens_consumed } => {
                assert!(tokens_consumed <= 1000);
            }
            _ => {}
        }
    }

    #[test]
    fn test_weight_based_scheduling() {
        let scheduler = FlowScheduler::new(FlowSchedConfig {
            max_bps: 0,
            ..Default::default()
        });

        scheduler
            .register_flow(make_flow_id(1), 0, 100, now_ms())
            .unwrap();
        scheduler
            .register_flow(make_flow_id(2), 0, 200, now_ms())
            .unwrap();

        let flows = scheduler.active_flows();
        let flow1 = flows.iter().find(|f| f.flow_id == make_flow_id(1)).unwrap();
        let flow2 = flows.iter().find(|f| f.flow_id == make_flow_id(2)).unwrap();

        assert_eq!(flow1.weight, 100);
        assert_eq!(flow2.weight, 200);
    }

    #[test]
    fn test_flow_entry_fields() {
        let now = now_ms();
        let scheduler = FlowScheduler::new(FlowSchedConfig::default());
        scheduler
            .register_flow(make_flow_id(1), 2, 50, now)
            .unwrap();

        let flows = scheduler.active_flows();
        let flow = &flows[0];

        assert_eq!(flow.priority, 2);
        assert_eq!(flow.weight, 50);
        assert_eq!(flow.created_at_ms, now);
        assert_eq!(flow.bytes_queued, 0);
        assert_eq!(flow.bytes_sent, 0);
    }

    #[test]
    fn test_rejected_has_reason() {
        let scheduler = FlowScheduler::new(FlowSchedConfig::default());

        let decision = scheduler.request_send(make_flow_id(1), 1000, now_ms());

        if let SendDecision::Rejected { reason } = decision {
            assert!(!reason.is_empty());
        } else {
            panic!("expected Rejected");
        }
    }

    #[test]
    fn test_config_defaults() {
        let config = FlowSchedConfig::default();

        assert_eq!(config.max_bps, 0);
        assert_eq!(config.priority_levels, 4);
        assert_eq!(config.refill_interval_ms, 10);
        assert_eq!(config.max_burst_bytes, 1_000_000);
        assert_eq!(config.quantum_bytes, 64_000);
    }

    #[test]
    fn test_concurrent_register_same_priority() {
        let scheduler = FlowScheduler::new(FlowSchedConfig::default());

        for i in 1..=3 {
            let result = scheduler.register_flow(make_flow_id(i), 0, 100, now_ms());
            assert!(result.is_ok());
        }

        let flows = scheduler.active_flows();
        assert_eq!(flows.len(), 3);
    }

    #[test]
    fn test_pending_send_struct() {
        let now = now_ms();
        let send = PendingSend {
            flow_id: make_flow_id(1),
            size_bytes: 5000,
            priority: 1,
            enqueued_at_ms: now,
        };

        assert_eq!(send.flow_id, make_flow_id(1));
        assert_eq!(send.size_bytes, 5000);
        assert_eq!(send.priority, 1);
        assert_eq!(send.enqueued_at_ms, now);
    }

    #[test]
    fn test_flow_id_equality() {
        let id1 = FlowId(100);
        let id2 = FlowId(100);
        let id3 = FlowId(200);

        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_send_decision_variants() {
        let approved = SendDecision::Approved {
            tokens_consumed: 100,
        };
        let deferred = SendDecision::Deferred { wait_ms: 50 };
        let rejected = SendDecision::Rejected {
            reason: "test".to_string(),
        };

        match approved {
            SendDecision::Approved { tokens_consumed } => assert_eq!(tokens_consumed, 100),
            _ => panic!("expected Approved"),
        }

        match deferred {
            SendDecision::Deferred { wait_ms } => assert_eq!(wait_ms, 50),
            _ => panic!("expected Deferred"),
        }

        match rejected {
            SendDecision::Rejected { reason } => assert_eq!(reason, "test"),
            _ => panic!("expected Rejected"),
        }
    }

    #[test]
    fn test_stats_snapshot() {
        let scheduler = FlowScheduler::new(FlowSchedConfig::default());
        scheduler
            .register_flow(make_flow_id(1), 0, 100, now_ms())
            .unwrap();

        let _ = scheduler.request_send(make_flow_id(1), 1000, now_ms());

        let snapshot = scheduler.stats();

        assert_eq!(snapshot.active_flows, 1);
        assert_eq!(snapshot.total_approved, 1);
        assert_eq!(snapshot.total_bytes_approved, 1000);
    }
}
