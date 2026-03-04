//! Chunk I/O scheduling with priority-based queue.
//!
//! Prioritizes interactive reads over background writes (GC, compaction)
//! while preventing starvation through anti-starvation quotas.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use thiserror::Error;

/// Chunk operation types.
#[derive(Debug, Clone)]
pub enum ChunkOp {
    /// Read operation for user-facing request.
    Read {
        /// Hash of the chunk to read.
        chunk_hash: [u8; 32],
        /// ID of the requester.
        requester_id: u64,
    },
    /// Write operation for background tasks.
    Write {
        /// Hash of the chunk to write.
        chunk_hash: [u8; 32],
        /// Data to write.
        data: Vec<u8>,
    },
    /// Prefetch operation for anticipated reads.
    Prefetch {
        /// Hash of the chunk to prefetch.
        chunk_hash: [u8; 32],
    },
}

/// Priority levels for operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum OpPriority {
    /// Background operations (GC, compaction writes) - lowest priority.
    Background,
    /// Prefetch operations - medium priority.
    Prefetch,
    /// Interactive operations (user-facing reads) - highest priority.
    Interactive,
}

/// Scheduled operation with metadata.
#[derive(Debug, Clone)]
pub struct ScheduledOp {
    /// The operation to execute.
    pub op: ChunkOp,
    /// Priority level.
    pub priority: OpPriority,
    /// Timestamp when submitted (ms).
    pub submitted_at_ms: u64,
    /// Unique operation ID.
    pub op_id: u64,
}

/// Configuration for the scheduler.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulerConfig {
    /// Maximum operations in queue before rejecting.
    pub max_queue_size: usize,
    /// Number of consecutive interactive ops before allowing one background op.
    pub interactive_quota: u32,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            max_queue_size: 10000,
            interactive_quota: 10,
        }
    }
}

/// Errors from the scheduler.
#[derive(Debug, Error)]
pub enum SchedulerError {
    /// Queue is at capacity.
    #[error("queue full")]
    QueueFull,
}

/// Priority-based chunk operation scheduler.
pub struct ChunkScheduler {
    config: SchedulerConfig,
    interactive_queue: VecDeque<ScheduledOp>,
    prefetch_queue: VecDeque<ScheduledOp>,
    background_queue: VecDeque<ScheduledOp>,
    next_op_id: u64,
    interactive_since_background: u32,
}

impl Default for ChunkScheduler {
    fn default() -> Self {
        Self::new(SchedulerConfig::default())
    }
}

impl ChunkScheduler {
    /// Creates a new scheduler with the given configuration.
    pub fn new(config: SchedulerConfig) -> Self {
        Self {
            config,
            interactive_queue: VecDeque::new(),
            prefetch_queue: VecDeque::new(),
            background_queue: VecDeque::new(),
            next_op_id: 1,
            interactive_since_background: 0,
        }
    }

    /// Submit an operation with the given priority.
    /// Returns the assigned operation ID or an error if queue is full.
    pub fn submit(
        &mut self,
        op: ChunkOp,
        priority: OpPriority,
        now_ms: u64,
    ) -> Result<u64, SchedulerError> {
        if self.queue_len() >= self.config.max_queue_size {
            return Err(SchedulerError::QueueFull);
        }

        let op_id = self.next_op_id;
        self.next_op_id += 1;

        let scheduled = ScheduledOp {
            op,
            priority,
            submitted_at_ms: now_ms,
            op_id,
        };

        match priority {
            OpPriority::Interactive => self.interactive_queue.push_back(scheduled),
            OpPriority::Prefetch => self.prefetch_queue.push_back(scheduled),
            OpPriority::Background => self.background_queue.push_back(scheduled),
        }

        Ok(op_id)
    }

    /// Dequeue the next operation respecting priority and anti-starvation.
    pub fn pop_next(&mut self) -> Option<ScheduledOp> {
        // Check if we need to allow a background op to prevent starvation
        if self.interactive_since_background >= self.config.interactive_quota
            && !self.background_queue.is_empty()
        {
            self.interactive_since_background = 0;
            return self.background_queue.pop_front();
        }

        // Priority order: Interactive > Prefetch > Background
        if let Some(op) = self.interactive_queue.pop_front() {
            self.interactive_since_background += 1;
            return Some(op);
        }

        if let Some(op) = self.prefetch_queue.pop_front() {
            return Some(op);
        }

        if let Some(op) = self.background_queue.pop_front() {
            return Some(op);
        }

        None
    }

    /// Returns current queue length across all priorities.
    pub fn queue_len(&self) -> usize {
        self.interactive_queue.len() + self.prefetch_queue.len() + self.background_queue.len()
    }

    /// Returns true if all queues are empty.
    pub fn is_empty(&self) -> bool {
        self.queue_len() == 0
    }

    /// Clears all pending operations.
    pub fn clear(&mut self) {
        self.interactive_queue.clear();
        self.prefetch_queue.clear();
        self.background_queue.clear();
        self.interactive_since_background = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_hash(n: u8) -> [u8; 32] {
        let mut h = [0u8; 32];
        h[0] = n;
        h
    }

    #[test]
    fn scheduler_config_default() {
        let config = SchedulerConfig::default();
        assert_eq!(config.max_queue_size, 10000);
        assert_eq!(config.interactive_quota, 10);
    }

    #[test]
    fn submit_single_op() {
        let mut scheduler = ChunkScheduler::new(SchedulerConfig::default());
        let op = ChunkOp::Read {
            chunk_hash: make_hash(1),
            requester_id: 1,
        };

        let result = scheduler.submit(op, OpPriority::Interactive, 1000);
        assert!(result.is_ok());
        assert_eq!(scheduler.queue_len(), 1);
    }

    #[test]
    fn submit_returns_unique_ids() {
        let mut scheduler = ChunkScheduler::new(SchedulerConfig::default());

        let id1 = scheduler
            .submit(
                ChunkOp::Read {
                    chunk_hash: make_hash(1),
                    requester_id: 1,
                },
                OpPriority::Interactive,
                1000,
            )
            .unwrap();

        let id2 = scheduler
            .submit(
                ChunkOp::Read {
                    chunk_hash: make_hash(2),
                    requester_id: 1,
                },
                OpPriority::Interactive,
                1000,
            )
            .unwrap();

        assert_ne!(id1, id2);
    }

    #[test]
    fn next_returns_highest_priority() {
        let mut scheduler = ChunkScheduler::new(SchedulerConfig::default());

        scheduler
            .submit(
                ChunkOp::Write {
                    chunk_hash: make_hash(1),
                    data: vec![1, 2, 3],
                },
                OpPriority::Background,
                1000,
            )
            .unwrap();
        scheduler
            .submit(
                ChunkOp::Read {
                    chunk_hash: make_hash(2),
                    requester_id: 1,
                },
                OpPriority::Interactive,
                1000,
            )
            .unwrap();

        let next = scheduler.pop_next().unwrap();
        assert_eq!(next.priority, OpPriority::Interactive);
    }

    #[test]
    fn next_interactive_before_background() {
        let mut scheduler = ChunkScheduler::new(SchedulerConfig::default());

        scheduler
            .submit(
                ChunkOp::Write {
                    chunk_hash: make_hash(1),
                    data: vec![1],
                },
                OpPriority::Background,
                1000,
            )
            .unwrap();
        scheduler
            .submit(
                ChunkOp::Read {
                    chunk_hash: make_hash(2),
                    requester_id: 1,
                },
                OpPriority::Interactive,
                1000,
            )
            .unwrap();

        let first = scheduler.pop_next().unwrap();
        assert_eq!(first.priority, OpPriority::Interactive);

        let second = scheduler.pop_next().unwrap();
        assert_eq!(second.priority, OpPriority::Background);
    }

    #[test]
    fn next_interactive_before_prefetch() {
        let mut scheduler = ChunkScheduler::new(SchedulerConfig::default());

        scheduler
            .submit(
                ChunkOp::Prefetch {
                    chunk_hash: make_hash(1),
                },
                OpPriority::Prefetch,
                1000,
            )
            .unwrap();
        scheduler
            .submit(
                ChunkOp::Read {
                    chunk_hash: make_hash(2),
                    requester_id: 1,
                },
                OpPriority::Interactive,
                1000,
            )
            .unwrap();

        let first = scheduler.pop_next().unwrap();
        assert_eq!(first.priority, OpPriority::Interactive);

        let second = scheduler.pop_next().unwrap();
        assert_eq!(second.priority, OpPriority::Prefetch);
    }

    #[test]
    fn next_prefetch_before_background() {
        let mut scheduler = ChunkScheduler::new(SchedulerConfig::default());

        scheduler
            .submit(
                ChunkOp::Write {
                    chunk_hash: make_hash(1),
                    data: vec![1],
                },
                OpPriority::Background,
                1000,
            )
            .unwrap();
        scheduler
            .submit(
                ChunkOp::Prefetch {
                    chunk_hash: make_hash(2),
                },
                OpPriority::Prefetch,
                1000,
            )
            .unwrap();

        let first = scheduler.pop_next().unwrap();
        assert_eq!(first.priority, OpPriority::Prefetch);

        let second = scheduler.pop_next().unwrap();
        assert_eq!(second.priority, OpPriority::Background);
    }

    #[test]
    fn queue_full_returns_error() {
        let config = SchedulerConfig {
            max_queue_size: 2,
            interactive_quota: 10,
        };
        let mut scheduler = ChunkScheduler::new(config);

        scheduler
            .submit(
                ChunkOp::Read {
                    chunk_hash: make_hash(1),
                    requester_id: 1,
                },
                OpPriority::Interactive,
                1000,
            )
            .unwrap();
        scheduler
            .submit(
                ChunkOp::Read {
                    chunk_hash: make_hash(2),
                    requester_id: 1,
                },
                OpPriority::Interactive,
                1000,
            )
            .unwrap();

        let result = scheduler.submit(
            ChunkOp::Read {
                chunk_hash: make_hash(3),
                requester_id: 1,
            },
            OpPriority::Interactive,
            1000,
        );

        assert!(matches!(result, Err(SchedulerError::QueueFull)));
    }

    #[test]
    fn next_on_empty_returns_none() {
        let mut scheduler = ChunkScheduler::new(SchedulerConfig::default());
        assert!(scheduler.pop_next().is_none());
    }

    #[test]
    fn queue_len_after_submit() {
        let mut scheduler = ChunkScheduler::new(SchedulerConfig::default());

        assert_eq!(scheduler.queue_len(), 0);

        scheduler
            .submit(
                ChunkOp::Read {
                    chunk_hash: make_hash(1),
                    requester_id: 1,
                },
                OpPriority::Interactive,
                1000,
            )
            .unwrap();
        assert_eq!(scheduler.queue_len(), 1);

        scheduler
            .submit(
                ChunkOp::Write {
                    chunk_hash: make_hash(2),
                    data: vec![1],
                },
                OpPriority::Background,
                1000,
            )
            .unwrap();
        assert_eq!(scheduler.queue_len(), 2);
    }

    #[test]
    fn is_empty_initially() {
        let scheduler = ChunkScheduler::new(SchedulerConfig::default());
        assert!(scheduler.is_empty());
    }

    #[test]
    fn clear_empties_queue() {
        let mut scheduler = ChunkScheduler::new(SchedulerConfig::default());

        scheduler
            .submit(
                ChunkOp::Read {
                    chunk_hash: make_hash(1),
                    requester_id: 1,
                },
                OpPriority::Interactive,
                1000,
            )
            .unwrap();
        scheduler
            .submit(
                ChunkOp::Write {
                    chunk_hash: make_hash(2),
                    data: vec![1],
                },
                OpPriority::Background,
                1000,
            )
            .unwrap();

        assert_eq!(scheduler.queue_len(), 2);

        scheduler.clear();

        assert!(scheduler.is_empty());
        assert_eq!(scheduler.queue_len(), 0);
    }

    #[test]
    fn interactive_quota_anti_starvation() {
        let config = SchedulerConfig {
            max_queue_size: 100,
            interactive_quota: 3,
        };
        let mut scheduler = ChunkScheduler::new(config);

        // Submit one background op
        scheduler
            .submit(
                ChunkOp::Write {
                    chunk_hash: make_hash(1),
                    data: vec![1],
                },
                OpPriority::Background,
                1000,
            )
            .unwrap();

        // Submit many interactive ops
        for i in 0..5 {
            scheduler
                .submit(
                    ChunkOp::Read {
                        chunk_hash: make_hash(i as u8 + 10),
                        requester_id: 1,
                    },
                    OpPriority::Interactive,
                    1000,
                )
                .unwrap();
        }

        // First 3 interactive ops should be returned
        for _ in 0..3 {
            let op = scheduler.pop_next().unwrap();
            assert_eq!(op.priority, OpPriority::Interactive);
        }

        // After quota, background should get a turn
        let op = scheduler.pop_next().unwrap();
        assert_eq!(op.priority, OpPriority::Background);

        // Then interactive resumes
        let op = scheduler.pop_next().unwrap();
        assert_eq!(op.priority, OpPriority::Interactive);
    }

    #[test]
    fn op_id_monotonically_increasing() {
        let mut scheduler = ChunkScheduler::new(SchedulerConfig::default());

        let mut ids = Vec::new();
        for i in 0..5 {
            let id = scheduler
                .submit(
                    ChunkOp::Read {
                        chunk_hash: make_hash(i as u8),
                        requester_id: 1,
                    },
                    OpPriority::Interactive,
                    1000,
                )
                .unwrap();
            ids.push(id);
        }

        for i in 0..ids.len() - 1 {
            assert!(ids[i] < ids[i + 1]);
        }
    }

    #[test]
    fn mixed_priority_order() {
        let mut scheduler = ChunkScheduler::new(SchedulerConfig {
            max_queue_size: 100,
            interactive_quota: 100,
        });

        scheduler
            .submit(
                ChunkOp::Write {
                    chunk_hash: make_hash(1),
                    data: vec![1],
                },
                OpPriority::Background,
                1000,
            )
            .unwrap();
        scheduler
            .submit(
                ChunkOp::Prefetch {
                    chunk_hash: make_hash(2),
                },
                OpPriority::Prefetch,
                1000,
            )
            .unwrap();
        scheduler
            .submit(
                ChunkOp::Read {
                    chunk_hash: make_hash(3),
                    requester_id: 1,
                },
                OpPriority::Interactive,
                1000,
            )
            .unwrap();

        let op1 = scheduler.pop_next().unwrap();
        assert_eq!(op1.priority, OpPriority::Interactive);

        let op2 = scheduler.pop_next().unwrap();
        assert_eq!(op2.priority, OpPriority::Prefetch);

        let op3 = scheduler.pop_next().unwrap();
        assert_eq!(op3.priority, OpPriority::Background);
    }

    #[test]
    fn op_priority_ordering() {
        assert!(OpPriority::Interactive > OpPriority::Prefetch);
        assert!(OpPriority::Prefetch > OpPriority::Background);
        assert!(OpPriority::Interactive > OpPriority::Background);
    }

    #[test]
    fn scheduled_op_fields() {
        let mut scheduler = ChunkScheduler::new(SchedulerConfig::default());

        let op = ChunkOp::Read {
            chunk_hash: make_hash(42),
            requester_id: 123,
        };
        let id = scheduler.submit(op, OpPriority::Interactive, 5000).unwrap();

        let scheduled = scheduler.pop_next().unwrap();
        assert_eq!(scheduled.op_id, id);
        assert_eq!(scheduled.submitted_at_ms, 5000);
        assert_eq!(scheduled.priority, OpPriority::Interactive);
    }

    #[test]
    fn queue_len_decrements_after_next() {
        let mut scheduler = ChunkScheduler::new(SchedulerConfig::default());

        scheduler
            .submit(
                ChunkOp::Read {
                    chunk_hash: make_hash(1),
                    requester_id: 1,
                },
                OpPriority::Interactive,
                1000,
            )
            .unwrap();
        scheduler
            .submit(
                ChunkOp::Read {
                    chunk_hash: make_hash(2),
                    requester_id: 1,
                },
                OpPriority::Interactive,
                1000,
            )
            .unwrap();

        assert_eq!(scheduler.queue_len(), 2);

        scheduler.pop_next();
        assert_eq!(scheduler.queue_len(), 1);

        scheduler.pop_next();
        assert_eq!(scheduler.queue_len(), 0);
    }

    #[test]
    fn chunk_op_debug_format() {
        let op = ChunkOp::Read {
            chunk_hash: make_hash(1),
            requester_id: 42,
        };
        let debug_str = format!("{:?}", op);
        assert!(debug_str.contains("Read"));
    }

    #[test]
    fn scheduler_config_clone() {
        let config = SchedulerConfig {
            max_queue_size: 100,
            interactive_quota: 5,
        };
        let cloned = config.clone();
        assert_eq!(cloned.max_queue_size, 100);
        assert_eq!(cloned.interactive_quota, 5);
    }
}
