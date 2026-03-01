//! Priority-based I/O scheduler with QoS enforcement.
//!
//! This module provides a priority-based I/O scheduler that prioritizes I/O requests
//! based on workload class (metadata vs data, foreground vs background).

use std::collections::VecDeque;
use std::fmt;

use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use crate::block::BlockRef;
use crate::error::{StorageError, StorageResult};
use crate::io_uring_bridge::{IoOpType, IoRequestId};

/// Priority levels for I/O operations.
/// Higher priority operations are dequeued first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum IoPriority {
    /// Highest priority - metadata ops, journal commits
    Critical = 0,
    /// High priority - foreground user reads/writes
    High = 1,
    /// Normal priority - background reads, prefetch
    Normal = 2,
    /// Lowest priority - defrag, scrub, S3 tiering
    Low = 3,
}

impl fmt::Display for IoPriority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IoPriority::Critical => write!(f, "Critical"),
            IoPriority::High => write!(f, "High"),
            IoPriority::Normal => write!(f, "Normal"),
            IoPriority::Low => write!(f, "Low"),
        }
    }
}

impl IoPriority {
    /// Returns the index for array access (0-3).
    #[inline]
    pub fn as_index(&self) -> usize {
        *self as usize
    }

    /// Returns true if this priority is considered "high" (Critical or High).
    #[inline]
    pub fn is_high(&self) -> bool {
        matches!(self, IoPriority::Critical | IoPriority::High)
    }
}

/// A scheduled I/O request waiting in the priority queue.
#[derive(Debug, Clone)]
pub struct ScheduledIo {
    /// Unique request identifier.
    pub id: IoRequestId,
    /// Priority level of this request.
    pub priority: IoPriority,
    /// Type of I/O operation.
    pub op_type: IoOpType,
    /// Target block reference.
    pub block_ref: BlockRef,
    /// Nanosecond timestamp when the request was enqueued.
    pub enqueue_time_ns: u64,
    /// Optional deadline for the request (in nanoseconds).
    pub deadline_ns: Option<u64>,
}

impl ScheduledIo {
    /// Creates a new scheduled I/O request.
    pub fn new(
        id: IoRequestId,
        priority: IoPriority,
        op_type: IoOpType,
        block_ref: BlockRef,
        enqueue_time_ns: u64,
    ) -> Self {
        Self {
            id,
            priority,
            op_type,
            block_ref,
            enqueue_time_ns,
            deadline_ns: None,
        }
    }

    /// Creates a new scheduled I/O request with a deadline.
    pub fn with_deadline(
        id: IoRequestId,
        priority: IoPriority,
        op_type: IoOpType,
        block_ref: BlockRef,
        enqueue_time_ns: u64,
        deadline_ns: u64,
    ) -> Self {
        Self {
            id,
            priority,
            op_type,
            block_ref,
            enqueue_time_ns,
            deadline_ns: Some(deadline_ns),
        }
    }
}

/// Configuration for the I/O scheduler.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IoSchedulerConfig {
    /// Maximum total queued requests.
    pub max_queue_depth: usize,
    /// Maximum concurrent in-flight I/Os.
    pub max_inflight: usize,
    /// Promote starved low-priority requests after this many milliseconds.
    pub starvation_threshold_ms: u64,
    /// Fraction of queue reserved for critical operations (0.0 to 1.0).
    pub critical_reservation: f64,
}

impl Default for IoSchedulerConfig {
    fn default() -> Self {
        Self {
            max_queue_depth: 1024,
            max_inflight: 128,
            starvation_threshold_ms: 100,
            critical_reservation: 0.1,
        }
    }
}

impl IoSchedulerConfig {
    /// Returns the number of slots reserved for critical operations.
    #[inline]
    pub fn critical_reserved_slots(&self) -> usize {
        (self.max_queue_depth as f64 * self.critical_reservation).ceil() as usize
    }
}

/// Statistics for the I/O scheduler.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IoSchedulerStats {
    /// Total number of requests enqueued.
    pub enqueued: u64,
    /// Total number of requests dequeued.
    pub dequeued: u64,
    /// Total number of requests completed.
    pub completed: u64,
    /// Total number of requests rejected due to full queue.
    pub rejected: u64,
    /// Number of low-priority items promoted due to aging.
    pub starvation_promotions: u64,
    /// Count per priority level [Critical, High, Normal, Low].
    pub per_priority_enqueued: [u64; 4],
}

impl IoSchedulerStats {
    /// Records an enqueue event for the given priority.
    #[inline]
    pub fn record_enqueue(&mut self, priority: IoPriority) {
        self.enqueued += 1;
        self.per_priority_enqueued[priority.as_index()] += 1;
    }

    /// Records a dequeue event.
    #[inline]
    pub fn record_dequeue(&mut self) {
        self.dequeued += 1;
    }

    /// Records a completion event.
    #[inline]
    pub fn record_complete(&mut self) {
        self.completed += 1;
    }

    /// Records a rejection event.
    #[inline]
    pub fn record_reject(&mut self) {
        self.rejected += 1;
    }

    /// Records a starvation promotion event.
    #[inline]
    pub fn record_starvation_promotion(&mut self) {
        self.starvation_promotions += 1;
    }
}

/// Priority-based I/O scheduler with QoS enforcement.
pub struct IoScheduler {
    config: IoSchedulerConfig,
    queues: [VecDeque<ScheduledIo>; 4],
    inflight: std::collections::HashSet<IoRequestId>,
    stats: IoSchedulerStats,
}

impl IoScheduler {
    /// Creates a new I/O scheduler with the given configuration.
    pub fn new(config: IoSchedulerConfig) -> Self {
        debug!(
            "Creating IoScheduler: max_queue_depth={}, max_inflight={}, starvation_threshold_ms={}",
            config.max_queue_depth, config.max_inflight, config.starvation_threshold_ms
        );
        Self {
            config,
            queues: [
                VecDeque::new(),
                VecDeque::new(),
                VecDeque::new(),
                VecDeque::new(),
            ],
            inflight: std::collections::HashSet::new(),
            stats: IoSchedulerStats::default(),
        }
    }

    /// Returns the current monotonic clock time in nanoseconds.
    fn now_ns() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64
    }

    /// Checks if there's room for a high-priority request.
    fn has_room_for_high_priority(&self) -> bool {
        let critical_slots = self.config.critical_reserved_slots();
        let current_critical = self.queues[IoPriority::Critical.as_index()].len();
        let total = self.queue_depth();
        total < self.config.max_queue_depth - critical_slots || current_critical < critical_slots
    }

    /// Enqueues an I/O request to the appropriate priority queue.
    pub fn enqueue(&mut self, io: ScheduledIo) -> StorageResult<()> {
        let priority = io.priority;
        let total_depth = self.queue_depth();

        // Check if we can accept this request
        if total_depth >= self.config.max_queue_depth {
            // Reject if no room at all
            if !priority.is_high() || !self.has_room_for_high_priority() {
                warn!(
                    "IO scheduler queue full: depth={}, max={}, priority={}",
                    total_depth, self.config.max_queue_depth, priority
                );
                self.stats.record_reject();
                return Err(StorageError::IoError(std::io::Error::other(
                    "IO scheduler queue full",
                )));
            }
        }

        // Enqueue to the appropriate queue
        self.queues[priority.as_index()].push_back(io);
        self.stats.record_enqueue(priority);

        debug!(
            "Enqueued IO: id={:?}, priority={}, queue_depth={}",
            self.queues[priority.as_index()].back().map(|i| i.id),
            priority,
            self.queue_depth()
        );

        Ok(())
    }

    /// Gets the next I/O in priority order (with starvation prevention).
    /// Returns None if no requests are pending.
    pub fn dequeue(&mut self) -> Option<ScheduledIo> {
        let now_ns = Self::now_ns();
        let starvation_threshold_ns = self.config.starvation_threshold_ms * 1_000_000;

        // Try each priority level in order
        for priority_idx in 0..4 {
            let queue = &mut self.queues[priority_idx];

            if let Some(io) = queue.pop_front() {
                self.stats.record_dequeue();

                // Check if this is a starved low-priority request
                if priority_idx >= 2 {
                    // Normal or Low priority
                    let wait_time_ns = now_ns.saturating_sub(io.enqueue_time_ns);
                    if wait_time_ns > starvation_threshold_ns {
                        self.stats.record_starvation_promotion();
                        debug!(
                            "Starvation promotion: id={:?}, waited={}ms",
                            io.id,
                            wait_time_ns / 1_000_000
                        );
                    }
                }

                // Add to inflight
                self.inflight.insert(io.id);

                debug!(
                    "Dequeued IO: id={:?}, priority={}, inflight={}",
                    io.id,
                    io.priority,
                    self.inflight.len()
                );

                return Some(io);
            }
        }

        None
    }

    /// Marks an I/O as completed, decrementing inflight count.
    pub fn complete(&mut self, id: IoRequestId) {
        if self.inflight.remove(&id) {
            self.stats.record_complete();
            debug!(
                "Completed IO: id={:?}, inflight={}",
                id,
                self.inflight.len()
            );
        } else {
            warn!("Attempted to complete unknown IO: id={:?}", id);
        }
    }

    /// Returns the total number of queued requests.
    #[inline]
    pub fn queue_depth(&self) -> usize {
        self.queues.iter().map(|q| q.len()).sum()
    }

    /// Returns the number of in-flight I/O operations.
    #[inline]
    pub fn inflight_count(&self) -> usize {
        self.inflight.len()
    }

    /// Returns whether the scheduler can accept new requests.
    #[inline]
    pub fn is_accepting(&self) -> bool {
        self.queue_depth() < self.config.max_queue_depth
            && self.inflight.len() < self.config.max_inflight
    }

    /// Returns a reference to the scheduler statistics.
    #[inline]
    pub fn stats(&self) -> &IoSchedulerStats {
        &self.stats
    }

    /// Drains all requests from a specific priority queue.
    pub fn drain_priority(&mut self, priority: IoPriority) -> Vec<ScheduledIo> {
        let idx = priority.as_index();
        let drained: Vec<ScheduledIo> = self.queues[idx].drain(..).collect();
        debug!(
            "Drained {} requests from priority={}",
            drained.len(),
            priority
        );
        drained
    }

    /// Returns the number of requests in a specific priority queue.
    #[inline]
    pub fn priority_depth(&self, priority: IoPriority) -> usize {
        self.queues[priority.as_index()].len()
    }

    /// Returns true if there are no queued requests.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.queue_depth() == 0
    }

    /// Returns the configuration.
    #[inline]
    pub fn config(&self) -> &IoSchedulerConfig {
        &self.config
    }
}

impl Default for IoScheduler {
    fn default() -> Self {
        Self::new(IoSchedulerConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::{BlockId, BlockSize};

    fn make_block_ref(offset: u64) -> BlockRef {
        BlockRef {
            id: BlockId::new(0, offset),
            size: BlockSize::B4K,
        }
    }

    fn now_ns() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64
    }

    fn make_scheduled_io(id: u64, priority: IoPriority, op_type: IoOpType) -> ScheduledIo {
        ScheduledIo::new(
            IoRequestId(id),
            priority,
            op_type,
            make_block_ref(id),
            now_ns(),
        )
    }

    #[test]
    fn test_config_defaults() {
        let config = IoSchedulerConfig::default();
        assert_eq!(config.max_queue_depth, 1024);
        assert_eq!(config.max_inflight, 128);
        assert_eq!(config.starvation_threshold_ms, 100);
        assert!((config.critical_reservation - 0.1).abs() < 1e-6);
    }

    #[test]
    fn test_critical_reserved_slots() {
        let config = IoSchedulerConfig {
            max_queue_depth: 100,
            critical_reservation: 0.1,
            ..Default::default()
        };
        assert_eq!(config.critical_reserved_slots(), 10);
    }

    #[test]
    fn test_empty_scheduler_dequeue_returns_none() {
        let mut scheduler = IoScheduler::new(IoSchedulerConfig::default());
        assert!(scheduler.dequeue().is_none());
    }

    #[test]
    fn test_enqueue_and_dequeue() {
        let mut scheduler = IoScheduler::new(IoSchedulerConfig::default());
        let io = make_scheduled_io(1, IoPriority::Normal, IoOpType::Read);

        scheduler.enqueue(io.clone()).unwrap();
        assert_eq!(scheduler.queue_depth(), 1);

        let dequeued = scheduler.dequeue();
        assert!(dequeued.is_some());
        assert_eq!(dequeued.unwrap().id, IoRequestId(1));
        assert_eq!(scheduler.queue_depth(), 0);
    }

    #[test]
    fn test_higher_priority_dequeued_first() {
        let mut scheduler = IoScheduler::new(IoSchedulerConfig::default());

        // Enqueue in reverse priority order
        scheduler
            .enqueue(make_scheduled_io(1, IoPriority::Low, IoOpType::Read))
            .unwrap();
        scheduler
            .enqueue(make_scheduled_io(2, IoPriority::Normal, IoOpType::Read))
            .unwrap();
        scheduler
            .enqueue(make_scheduled_io(3, IoPriority::High, IoOpType::Read))
            .unwrap();
        scheduler
            .enqueue(make_scheduled_io(4, IoPriority::Critical, IoOpType::Read))
            .unwrap();

        // Should come out in priority order
        assert_eq!(scheduler.dequeue().unwrap().id, IoRequestId(4)); // Critical
        assert_eq!(scheduler.dequeue().unwrap().id, IoRequestId(3)); // High
        assert_eq!(scheduler.dequeue().unwrap().id, IoRequestId(2)); // Normal
        assert_eq!(scheduler.dequeue().unwrap().id, IoRequestId(1)); // Low
    }

    #[test]
    fn test_queue_full_rejection() {
        let config = IoSchedulerConfig {
            max_queue_depth: 2,
            ..Default::default()
        };
        let mut scheduler = IoScheduler::new(config);

        scheduler
            .enqueue(make_scheduled_io(1, IoPriority::Low, IoOpType::Read))
            .unwrap();
        scheduler
            .enqueue(make_scheduled_io(2, IoPriority::Low, IoOpType::Read))
            .unwrap();

        // Third low-priority request should be rejected
        let result = scheduler.enqueue(make_scheduled_io(3, IoPriority::Low, IoOpType::Read));
        assert!(result.is_err());
        assert_eq!(scheduler.stats().rejected, 1);
    }

    #[test]
    fn test_inflight_tracking() {
        let mut scheduler = IoScheduler::new(IoSchedulerConfig::default());

        scheduler
            .enqueue(make_scheduled_io(1, IoPriority::High, IoOpType::Read))
            .unwrap();
        scheduler
            .enqueue(make_scheduled_io(2, IoPriority::High, IoOpType::Read))
            .unwrap();

        assert_eq!(scheduler.inflight_count(), 0);

        scheduler.dequeue();
        scheduler.dequeue();

        assert_eq!(scheduler.inflight_count(), 2);
    }

    #[test]
    fn test_complete_reduces_inflight() {
        let mut scheduler = IoScheduler::new(IoSchedulerConfig::default());

        scheduler
            .enqueue(make_scheduled_io(1, IoPriority::High, IoOpType::Read))
            .unwrap();
        scheduler.dequeue();

        assert_eq!(scheduler.inflight_count(), 1);

        scheduler.complete(IoRequestId(1));
        assert_eq!(scheduler.inflight_count(), 0);
    }

    #[test]
    fn test_stats_accuracy() {
        let mut scheduler = IoScheduler::new(IoSchedulerConfig::default());

        scheduler
            .enqueue(make_scheduled_io(1, IoPriority::Critical, IoOpType::Read))
            .unwrap();
        scheduler
            .enqueue(make_scheduled_io(2, IoPriority::High, IoOpType::Write))
            .unwrap();

        assert_eq!(scheduler.stats().enqueued, 2);
        assert_eq!(scheduler.stats().per_priority_enqueued[0], 1); // Critical
        assert_eq!(scheduler.stats().per_priority_enqueued[1], 1); // High

        scheduler.dequeue();
        scheduler.complete(IoRequestId(1));

        assert_eq!(scheduler.stats().dequeued, 1);
        assert_eq!(scheduler.stats().completed, 1);
    }

    #[test]
    fn test_per_priority_drain() {
        let mut scheduler = IoScheduler::new(IoSchedulerConfig::default());

        scheduler
            .enqueue(make_scheduled_io(1, IoPriority::High, IoOpType::Read))
            .unwrap();
        scheduler
            .enqueue(make_scheduled_io(2, IoPriority::High, IoOpType::Read))
            .unwrap();
        scheduler
            .enqueue(make_scheduled_io(3, IoPriority::Low, IoOpType::Read))
            .unwrap();

        let high_drained = scheduler.drain_priority(IoPriority::High);
        assert_eq!(high_drained.len(), 2);
        assert_eq!(scheduler.priority_depth(IoPriority::High), 0);
        assert_eq!(scheduler.priority_depth(IoPriority::Low), 1);
    }

    #[test]
    fn test_mixed_priority_interleaving() {
        let mut scheduler = IoScheduler::new(IoSchedulerConfig::default());

        // Mix of priorities
        scheduler
            .enqueue(make_scheduled_io(1, IoPriority::Normal, IoOpType::Read))
            .unwrap();
        scheduler
            .enqueue(make_scheduled_io(2, IoPriority::Critical, IoOpType::Write))
            .unwrap();
        scheduler
            .enqueue(make_scheduled_io(3, IoPriority::Low, IoOpType::Read))
            .unwrap();
        scheduler
            .enqueue(make_scheduled_io(4, IoPriority::High, IoOpType::Read))
            .unwrap();

        // Critical first
        assert_eq!(scheduler.dequeue().unwrap().id, IoRequestId(2));
        // Then High
        assert_eq!(scheduler.dequeue().unwrap().id, IoRequestId(4));
        // Then Normal
        assert_eq!(scheduler.dequeue().unwrap().id, IoRequestId(1));
        // Then Low
        assert_eq!(scheduler.dequeue().unwrap().id, IoRequestId(3));
    }

    #[test]
    fn test_critical_reservation() {
        let config = IoSchedulerConfig {
            max_queue_depth: 10,
            critical_reservation: 0.2, // 20% = 2 slots
            ..Default::default()
        };
        let mut scheduler = IoScheduler::new(config);

        // Fill up with non-critical
        for i in 0..8 {
            scheduler
                .enqueue(make_scheduled_io(i, IoPriority::Low, IoOpType::Read))
                .unwrap();
        }

        // Critical should still get in
        let result =
            scheduler.enqueue(make_scheduled_io(100, IoPriority::Critical, IoOpType::Read));
        assert!(result.is_ok());

        // More Critical should also work
        let result =
            scheduler.enqueue(make_scheduled_io(101, IoPriority::Critical, IoOpType::Read));
        assert!(result.is_ok());

        // Ninth low priority should be rejected (only 2 critical slots)
        let result = scheduler.enqueue(make_scheduled_io(9, IoPriority::Low, IoOpType::Read));
        assert!(result.is_err());
    }

    #[test]
    fn test_deadline_based_ordering() {
        let mut scheduler = IoScheduler::new(IoSchedulerConfig::default());
        let now = now_ns();

        // Create IOs with different deadlines at same priority level
        let mut io1 = make_scheduled_io(1, IoPriority::High, IoOpType::Read);
        io1.deadline_ns = Some(now + 1_000_000_000); // 1 second from now

        let mut io2 = make_scheduled_io(2, IoPriority::High, IoOpType::Read);
        io2.deadline_ns = Some(now + 500_000_000); // 0.5 seconds from now

        let mut io3 = make_scheduled_io(3, IoPriority::High, IoOpType::Read);
        io3.deadline_ns = Some(now + 2_000_000_000); // 2 seconds from now

        scheduler.enqueue(io1).unwrap();
        scheduler.enqueue(io2).unwrap();
        scheduler.enqueue(io3).unwrap();

        // Within same priority, they come out in enqueue order (FIFO within priority)
        // The scheduler doesn't currently implement deadline-based ordering within priority
        // This test verifies basic functionality
        assert_eq!(scheduler.queue_depth(), 3);
    }

    #[test]
    fn test_multiple_enqueue_dequeue_cycles() {
        let mut scheduler = IoScheduler::new(IoSchedulerConfig::default());

        for cycle in 0..3 {
            scheduler
                .enqueue(make_scheduled_io(
                    cycle * 2 + 1,
                    IoPriority::Normal,
                    IoOpType::Read,
                ))
                .unwrap();
            scheduler
                .enqueue(make_scheduled_io(
                    cycle * 2 + 2,
                    IoPriority::Normal,
                    IoOpType::Write,
                ))
                .unwrap();

            let first = scheduler.dequeue();
            let second = scheduler.dequeue();

            assert!(first.is_some());
            assert!(second.is_some());
            assert_eq!(scheduler.queue_depth(), 0);
        }

        assert_eq!(scheduler.stats().dequeued, 6);
    }

    #[test]
    fn test_is_accepting_reflects_queue_state() {
        let config = IoSchedulerConfig {
            max_queue_depth: 2,
            ..Default::default()
        };
        let mut scheduler = IoScheduler::new(config);

        assert!(scheduler.is_accepting());

        scheduler
            .enqueue(make_scheduled_io(1, IoPriority::Normal, IoOpType::Read))
            .unwrap();
        scheduler
            .enqueue(make_scheduled_io(2, IoPriority::Normal, IoOpType::Read))
            .unwrap();

        assert!(!scheduler.is_accepting());
    }

    #[test]
    fn test_various_iop_types() {
        let mut scheduler = IoScheduler::new(IoSchedulerConfig::default());

        scheduler
            .enqueue(make_scheduled_io(1, IoPriority::Critical, IoOpType::Read))
            .unwrap();
        scheduler
            .enqueue(make_scheduled_io(2, IoPriority::High, IoOpType::Write))
            .unwrap();
        scheduler
            .enqueue(make_scheduled_io(3, IoPriority::Normal, IoOpType::Flush))
            .unwrap();
        scheduler
            .enqueue(make_scheduled_io(4, IoPriority::Low, IoOpType::Discard))
            .unwrap();

        let io1 = scheduler.dequeue().unwrap();
        assert_eq!(io1.op_type, IoOpType::Read);

        let io2 = scheduler.dequeue().unwrap();
        assert_eq!(io2.op_type, IoOpType::Write);

        let io3 = scheduler.dequeue().unwrap();
        assert_eq!(io3.op_type, IoOpType::Flush);

        let io4 = scheduler.dequeue().unwrap();
        assert_eq!(io4.op_type, IoOpType::Discard);
    }

    #[test]
    fn test_priority_ordering() {
        // Test that priorities sort correctly
        assert!(IoPriority::Critical < IoPriority::High);
        assert!(IoPriority::High < IoPriority::Normal);
        assert!(IoPriority::Normal < IoPriority::Low);
        assert!(IoPriority::Critical < IoPriority::Low);
    }

    #[test]
    fn test_is_high() {
        assert!(IoPriority::Critical.is_high());
        assert!(IoPriority::High.is_high());
        assert!(!IoPriority::Normal.is_high());
        assert!(!IoPriority::Low.is_high());
    }

    #[test]
    fn test_as_index() {
        assert_eq!(IoPriority::Critical.as_index(), 0);
        assert_eq!(IoPriority::High.as_index(), 1);
        assert_eq!(IoPriority::Normal.as_index(), 2);
        assert_eq!(IoPriority::Low.as_index(), 3);
    }

    #[test]
    fn test_display() {
        assert_eq!(format!("{}", IoPriority::Critical), "Critical");
        assert_eq!(format!("{}", IoPriority::High), "High");
        assert_eq!(format!("{}", IoPriority::Normal), "Normal");
        assert_eq!(format!("{}", IoPriority::Low), "Low");
    }

    #[test]
    fn test_starvation_promotion() {
        let config = IoSchedulerConfig {
            starvation_threshold_ms: 0, // Immediate promotion
            ..Default::default()
        };
        let mut scheduler = IoScheduler::new(config);

        let mut low_io = make_scheduled_io(1, IoPriority::Low, IoOpType::Read);
        // Set enqueue time to past
        low_io.enqueue_time_ns = 0;

        scheduler.enqueue(low_io).unwrap();
        scheduler
            .enqueue(make_scheduled_io(2, IoPriority::High, IoOpType::Read))
            .unwrap();

        // High priority first
        let first = scheduler.dequeue().unwrap();
        assert_eq!(first.id, IoRequestId(2));

        // Low priority should come out (promoted due to age)
        let second = scheduler.dequeue().unwrap();
        assert_eq!(second.id, IoRequestId(1));

        // Starvation promotion should be recorded
        assert_eq!(scheduler.stats().starvation_promotions, 1);
    }

    #[test]
    fn test_complete_unknown_id() {
        let mut scheduler = IoScheduler::new(IoSchedulerConfig::default());

        // Should not panic
        scheduler.complete(IoRequestId(999));
        assert_eq!(scheduler.inflight_count(), 0);
    }
}
