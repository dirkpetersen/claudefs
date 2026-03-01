//! Priority-based request scheduling for the transport layer.
//!
//! This module provides priority queues for scheduling network requests based on
//! their urgency and importance, with starvation prevention for lower-priority requests.

use crate::error::TransportError;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;

/// Priority levels for request scheduling.
///
/// Lower values indicate higher priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Priority {
    /// Highest priority - heartbeats and drain signals.
    Critical = 0,
    /// High priority - metadata operations (lookup, getattr, create, delete, rename).
    High = 1,
    /// Normal priority - data operations (read, write).
    Normal = 2,
    /// Low priority - background replication, compaction.
    Low = 3,
    /// Best effort - monitoring, stats collection.
    BestEffort = 4,
}

/// Configuration for the priority scheduler.
#[derive(Debug, Clone)]
pub struct PriorityConfig {
    /// Maximum total queued items.
    pub max_queue_size: usize,
    /// Process a lower-priority item after this many higher ones.
    pub starvation_threshold: u64,
    /// Whether priority scheduling is enabled.
    pub enable_priority: bool,
}

impl Default for PriorityConfig {
    fn default() -> Self {
        Self {
            max_queue_size: 4096,
            starvation_threshold: 100,
            enable_priority: true,
        }
    }
}

/// A request with associated priority information.
#[derive(Debug, Clone)]
pub struct PrioritizedRequest {
    /// The priority level of this request.
    pub priority: Priority,
    /// Unique identifier for this request.
    pub request_id: u64,
    /// The operation code.
    pub opcode: u8,
    /// The request payload data.
    pub payload: Vec<u8>,
    /// Timestamp when the request was enqueued.
    pub enqueued_at: Instant,
}

/// Statistics for the priority scheduler.
#[derive(Debug, Default)]
pub struct PriorityStats {
    /// Number of requests enqueued per priority level.
    enqueued: [AtomicU64; 5],
    /// Number of requests dequeued per priority level.
    dequeued: [AtomicU64; 5],
    /// Number of requests rejected due to full queue.
    rejected: AtomicU64,
    /// Number of times lower priority was served to prevent starvation.
    starvation_promotions: AtomicU64,
}

/// Snapshot of priority statistics at a point in time.
#[derive(Debug, Clone, Default)]
pub struct PriorityStatsSnapshot {
    /// Enqueued counts per priority level.
    pub enqueued: [u64; 5],
    /// Dequeued counts per priority level.
    pub dequeued: [u64; 5],
    /// Total rejected count.
    pub rejected: u64,
    /// Total starvation promotions.
    pub starvation_promotions: u64,
    /// Current total queued items.
    pub total_queued: usize,
}

/// Priority-based request scheduler with multiple queues and starvation prevention.
pub struct PriorityScheduler {
    config: PriorityConfig,
    queues: [Mutex<VecDeque<PrioritizedRequest>>; 5],
    total_queued: AtomicUsize,
    consecutive_high: AtomicU64,
    stats: PriorityStats,
}

impl PriorityScheduler {
    /// Create a new priority scheduler with the given configuration.
    pub fn new(config: PriorityConfig) -> Self {
        Self {
            config,
            queues: [
                Mutex::new(VecDeque::new()),
                Mutex::new(VecDeque::new()),
                Mutex::new(VecDeque::new()),
                Mutex::new(VecDeque::new()),
                Mutex::new(VecDeque::new()),
            ],
            total_queued: AtomicUsize::new(0),
            consecutive_high: AtomicU64::new(0),
            stats: PriorityStats::default(),
        }
    }

    /// Enqueue a request into the appropriate priority queue.
    ///
    /// Returns `BufferExhausted` if the total queue size would exceed the maximum.
    pub fn enqueue(&self, request: PrioritizedRequest) -> Result<(), TransportError> {
        if self.is_full() {
            self.stats.rejected.fetch_add(1, Ordering::Relaxed);
            return Err(TransportError::BufferExhausted);
        }

        let priority_idx = request.priority as usize;
        self.queues[priority_idx].lock().unwrap().push_back(request);

        self.total_queued.fetch_add(1, Ordering::Relaxed);
        self.stats.enqueued[priority_idx].fetch_add(1, Ordering::Relaxed);

        Ok(())
    }

    /// Dequeue the next request based on priority.
    ///
    /// Implements starvation prevention: after `starvation_threshold` consecutive
    /// high-priority dequeues, serves one lower-priority item.
    pub fn dequeue(&self) -> Option<PrioritizedRequest> {
        if self.total_queued.load(Ordering::Relaxed) == 0 {
            return None;
        }

        let threshold = self.config.starvation_threshold;
        let consecutive = self.consecutive_high.load(Ordering::Relaxed);
        let should_starve = consecutive >= threshold;

        // Determine which priority level to dequeue from
        if should_starve && self.config.enable_priority {
            // Find the lowest non-empty priority for starvation prevention
            for i in (0..5).rev() {
                if !self.queues[i].lock().unwrap().is_empty() {
                    if i > 0 {
                        self.stats
                            .starvation_promotions
                            .fetch_add(1, Ordering::Relaxed);
                    }
                    self.consecutive_high.store(0, Ordering::Relaxed);
                    return self.dequeue_from_priority(i);
                }
            }
            return None;
        }

        if self.config.enable_priority {
            // Normal priority-based selection
            for i in 0..5 {
                if !self.queues[i].lock().unwrap().is_empty() {
                    if i <= 1 {
                        self.consecutive_high.fetch_add(1, Ordering::Relaxed);
                    } else {
                        self.consecutive_high.store(0, Ordering::Relaxed);
                    }
                    return self.dequeue_from_priority(i);
                }
            }
            return None;
        }

        // Priority disabled - serve in FIFO order across all queues
        for i in 0..5 {
            if let Some(req) = self.queues[i].lock().unwrap().pop_front() {
                self.total_queued.fetch_sub(1, Ordering::Relaxed);
                self.stats.dequeued[i].fetch_add(1, Ordering::Relaxed);
                return Some(req);
            }
        }

        None
    }

    fn dequeue_from_priority(&self, priority: usize) -> Option<PrioritizedRequest> {
        if let Some(request) = self.queues[priority].lock().unwrap().pop_front() {
            self.total_queued.fetch_sub(1, Ordering::Relaxed);
            self.stats.dequeued[priority].fetch_add(1, Ordering::Relaxed);
            Some(request)
        } else {
            None
        }
    }

    /// Get the total number of items across all queues.
    pub fn queue_size(&self) -> usize {
        self.total_queued.load(Ordering::Relaxed)
    }

    /// Get the number of items in a specific priority queue.
    pub fn queue_size_by_priority(&self, priority: Priority) -> usize {
        self.queues[priority as usize].lock().unwrap().len()
    }

    /// Check if the scheduler is at capacity.
    pub fn is_full(&self) -> bool {
        self.total_queued.load(Ordering::Relaxed) >= self.config.max_queue_size
    }

    /// Get a snapshot of the current statistics.
    pub fn stats(&self) -> PriorityStatsSnapshot {
        let mut enqueued = [0u64; 5];
        let mut dequeued = [0u64; 5];
        for i in 0..5 {
            enqueued[i] = self.stats.enqueued[i].load(Ordering::Relaxed);
            dequeued[i] = self.stats.dequeued[i].load(Ordering::Relaxed);
        }
        PriorityStatsSnapshot {
            enqueued,
            dequeued,
            rejected: self.stats.rejected.load(Ordering::Relaxed),
            starvation_promotions: self.stats.starvation_promotions.load(Ordering::Relaxed),
            total_queued: self.total_queued.load(Ordering::Relaxed),
        }
    }

    /// Clear all queues.
    pub fn clear(&self) {
        for queue in &self.queues {
            let mut q = queue.lock().unwrap();
            self.total_queued.fetch_sub(q.len(), Ordering::Relaxed);
            q.clear();
        }
        self.consecutive_high.store(0, Ordering::Relaxed);
    }
}

/// Classify an opcode into a priority level based on its high byte.
///
/// - 0x03 (Heartbeat) -> Critical
/// - 0x01 (Metadata ops) -> High
/// - 0x02 (Data ops) -> Normal
/// - Other -> BestEffort
pub fn classify_opcode(opcode: u8) -> Priority {
    match opcode {
        0x03 => Priority::Critical,
        0x01 => Priority::High,
        0x02 => Priority::Normal,
        _ => Priority::BestEffort,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_priority_config_default() {
        let config = PriorityConfig::default();
        assert_eq!(config.max_queue_size, 4096);
        assert_eq!(config.starvation_threshold, 100);
        assert!(config.enable_priority);
    }

    #[test]
    fn test_priority_ordering() {
        assert!(Priority::Critical < Priority::High);
        assert!(Priority::High < Priority::Normal);
        assert!(Priority::Normal < Priority::Low);
        assert!(Priority::Low < Priority::BestEffort);
    }

    #[test]
    fn test_classify_heartbeat() {
        // Heartbeat = 0x0301, high byte is 0x03
        let opcode = 0x03;
        assert_eq!(classify_opcode(opcode), Priority::Critical);
    }

    #[test]
    fn test_classify_lookup() {
        // Lookup = 0x0101, high byte is 0x01
        let opcode = 0x01;
        assert_eq!(classify_opcode(opcode), Priority::High);
    }

    #[test]
    fn test_classify_read() {
        // Read = 0x0201, high byte is 0x02
        let opcode = 0x02;
        assert_eq!(classify_opcode(opcode), Priority::Normal);
    }

    #[test]
    fn test_classify_unknown() {
        // Unknown opcode (e.g., 0x04) -> BestEffort
        let opcode = 0x04;
        assert_eq!(classify_opcode(opcode), Priority::BestEffort);
    }

    #[test]
    fn test_enqueue_dequeue() {
        let scheduler = PriorityScheduler::new(PriorityConfig::default());
        let request = PrioritizedRequest {
            priority: Priority::Normal,
            request_id: 1,
            opcode: 0x02,
            payload: vec![1, 2, 3],
            enqueued_at: Instant::now(),
        };

        scheduler.enqueue(request.clone()).unwrap();
        let dequeued = scheduler.dequeue().unwrap();

        assert_eq!(dequeued.request_id, 1);
        assert_eq!(dequeued.opcode, 0x02);
    }

    #[test]
    fn test_priority_order() {
        let scheduler = PriorityScheduler::new(PriorityConfig::default());

        // Enqueue in reverse priority order
        scheduler
            .enqueue(PrioritizedRequest {
                priority: Priority::Low,
                request_id: 1,
                opcode: 0x01,
                payload: vec![],
                enqueued_at: Instant::now(),
            })
            .unwrap();
        scheduler
            .enqueue(PrioritizedRequest {
                priority: Priority::High,
                request_id: 2,
                opcode: 0x01,
                payload: vec![],
                enqueued_at: Instant::now(),
            })
            .unwrap();
        scheduler
            .enqueue(PrioritizedRequest {
                priority: Priority::Critical,
                request_id: 3,
                opcode: 0x03,
                payload: vec![],
                enqueued_at: Instant::now(),
            })
            .unwrap();

        // Should dequeue in priority order
        assert_eq!(scheduler.dequeue().unwrap().request_id, 3); // Critical
        assert_eq!(scheduler.dequeue().unwrap().request_id, 2); // High
        assert_eq!(scheduler.dequeue().unwrap().request_id, 1); // Low
    }

    #[test]
    fn test_enqueue_full() {
        let config = PriorityConfig {
            max_queue_size: 2,
            starvation_threshold: 100,
            enable_priority: true,
        };
        let scheduler = PriorityScheduler::new(config);

        scheduler
            .enqueue(PrioritizedRequest {
                priority: Priority::Normal,
                request_id: 1,
                opcode: 0x02,
                payload: vec![],
                enqueued_at: Instant::now(),
            })
            .unwrap();
        scheduler
            .enqueue(PrioritizedRequest {
                priority: Priority::Normal,
                request_id: 2,
                opcode: 0x02,
                payload: vec![],
                enqueued_at: Instant::now(),
            })
            .unwrap();

        let result = scheduler.enqueue(PrioritizedRequest {
            priority: Priority::Normal,
            request_id: 3,
            opcode: 0x02,
            payload: vec![],
            enqueued_at: Instant::now(),
        });

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            TransportError::BufferExhausted
        ));
    }

    #[test]
    fn test_dequeue_empty() {
        let scheduler = PriorityScheduler::new(PriorityConfig::default());
        assert!(scheduler.dequeue().is_none());
    }

    #[test]
    fn test_queue_size() {
        let scheduler = PriorityScheduler::new(PriorityConfig::default());
        assert_eq!(scheduler.queue_size(), 0);

        scheduler
            .enqueue(PrioritizedRequest {
                priority: Priority::Normal,
                request_id: 1,
                opcode: 0x02,
                payload: vec![],
                enqueued_at: Instant::now(),
            })
            .unwrap();
        assert_eq!(scheduler.queue_size(), 1);

        scheduler.dequeue();
        assert_eq!(scheduler.queue_size(), 0);
    }

    #[test]
    fn test_queue_size_by_priority() {
        let scheduler = PriorityScheduler::new(PriorityConfig::default());

        scheduler
            .enqueue(PrioritizedRequest {
                priority: Priority::High,
                request_id: 1,
                opcode: 0x01,
                payload: vec![],
                enqueued_at: Instant::now(),
            })
            .unwrap();
        scheduler
            .enqueue(PrioritizedRequest {
                priority: Priority::High,
                request_id: 2,
                opcode: 0x01,
                payload: vec![],
                enqueued_at: Instant::now(),
            })
            .unwrap();
        scheduler
            .enqueue(PrioritizedRequest {
                priority: Priority::Normal,
                request_id: 3,
                opcode: 0x02,
                payload: vec![],
                enqueued_at: Instant::now(),
            })
            .unwrap();

        assert_eq!(scheduler.queue_size_by_priority(Priority::High), 2);
        assert_eq!(scheduler.queue_size_by_priority(Priority::Normal), 1);
        assert_eq!(scheduler.queue_size_by_priority(Priority::Critical), 0);
    }

    #[test]
    fn test_starvation_prevention() {
        let config = PriorityConfig {
            max_queue_size: 1000,
            starvation_threshold: 3,
            enable_priority: true,
        };
        let scheduler = PriorityScheduler::new(config);

        // Enqueue many high priority items
        for i in 0..5 {
            scheduler
                .enqueue(PrioritizedRequest {
                    priority: Priority::High,
                    request_id: i,
                    opcode: 0x01,
                    payload: vec![],
                    enqueued_at: Instant::now(),
                })
                .unwrap();
        }
        // Enqueue one low priority item
        scheduler
            .enqueue(PrioritizedRequest {
                priority: Priority::Low,
                request_id: 100,
                opcode: 0x01,
                payload: vec![],
                enqueued_at: Instant::now(),
            })
            .unwrap();

        // Dequeue 3 high priority items (threshold)
        for _ in 0..3 {
            scheduler.dequeue();
        }

        // Dequeue the 4th item - should trigger starvation prevention and serve low priority
        scheduler.dequeue();

        // Check that starvation prevention was triggered
        let stats = scheduler.stats();
        assert!(stats.starvation_promotions > 0);
    }

    #[test]
    fn test_clear() {
        let scheduler = PriorityScheduler::new(PriorityConfig::default());

        scheduler
            .enqueue(PrioritizedRequest {
                priority: Priority::Normal,
                request_id: 1,
                opcode: 0x02,
                payload: vec![],
                enqueued_at: Instant::now(),
            })
            .unwrap();
        scheduler
            .enqueue(PrioritizedRequest {
                priority: Priority::High,
                request_id: 2,
                opcode: 0x01,
                payload: vec![],
                enqueued_at: Instant::now(),
            })
            .unwrap();

        assert_eq!(scheduler.queue_size(), 2);

        scheduler.clear();

        assert_eq!(scheduler.queue_size(), 0);
        assert!(scheduler.dequeue().is_none());
    }

    #[test]
    fn test_stats() {
        let scheduler = PriorityScheduler::new(PriorityConfig::default());

        scheduler
            .enqueue(PrioritizedRequest {
                priority: Priority::High,
                request_id: 1,
                opcode: 0x01,
                payload: vec![],
                enqueued_at: Instant::now(),
            })
            .unwrap();
        scheduler.dequeue().unwrap();

        let stats = scheduler.stats();
        assert_eq!(stats.enqueued[1], 1); // High priority
        assert_eq!(stats.dequeued[1], 1);
        assert_eq!(stats.total_queued, 0);
    }

    #[test]
    fn test_is_full() {
        let config = PriorityConfig {
            max_queue_size: 2,
            starvation_threshold: 100,
            enable_priority: true,
        };
        let scheduler = PriorityScheduler::new(config);

        assert!(!scheduler.is_full());

        scheduler
            .enqueue(PrioritizedRequest {
                priority: Priority::Normal,
                request_id: 1,
                opcode: 0x02,
                payload: vec![],
                enqueued_at: Instant::now(),
            })
            .unwrap();
        scheduler
            .enqueue(PrioritizedRequest {
                priority: Priority::Normal,
                request_id: 2,
                opcode: 0x02,
                payload: vec![],
                enqueued_at: Instant::now(),
            })
            .unwrap();

        assert!(scheduler.is_full());
    }
}
