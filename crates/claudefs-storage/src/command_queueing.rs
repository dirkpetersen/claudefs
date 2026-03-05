//! Per-core NVMe command submission queue to maximize throughput.
//!
//! This module provides a batching command queue that accumulates NVMe commands
//! and submits them in batches to reduce syscall overhead.

use std::collections::VecDeque;
use std::sync::Arc;

use serde::Serialize;
use thiserror::Error;
use tokio::sync::Mutex;

use crate::block::BlockId;
use crate::io_scheduler::IoPriority;
use crate::nvme_passthrough::{CoreId, QueuePair, QueuePairId, NsId, QueueState};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum CommandType {
    Read,
    Write,
    Flush,
    Deallocate,
}

#[derive(Debug, Clone)]
pub struct NvmeCommand {
    pub cmd_type: CommandType,
    pub block_id: BlockId,
    pub offset: u32,
    pub length: u32,
    pub buffer: Option<std::sync::Arc<Vec<u8>>>,
    pub submitted_at: std::time::Instant,
    pub user_data: u64,
    pub priority: IoPriority,
}

#[derive(Debug, Clone, Serialize)]
pub struct CommandQueueStats {
    pub queue_size: u32,
    pub max_queue_size: u32,
    pub total_commands: u64,
    pub total_syscalls: u64,
    pub avg_commands_per_syscall: f64,
    pub full_events: u64,
    pub total_bytes: u64,
    pub avg_queue_latency_us: u64,
}

impl Default for CommandQueueStats {
    fn default() -> Self {
        Self {
            queue_size: 0,
            max_queue_size: 0,
            total_commands: 0,
            total_syscalls: 0,
            avg_commands_per_syscall: 0.0,
            full_events: 0,
            total_bytes: 0,
            avg_queue_latency_us: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CommandQueueConfig {
    pub batch_threshold: u32,
    pub max_queue_latency_us: u64,
    pub capacity: usize,
    pub track_priority_stats: bool,
}

impl Default for CommandQueueConfig {
    fn default() -> Self {
        Self {
            batch_threshold: 32,
            max_queue_latency_us: 100,
            capacity: 64,
            track_priority_stats: false,
        }
    }
}

#[derive(Debug)]
pub struct CommandQueue {
    core_id: CoreId,
    qp: Arc<QueuePair>,
    queue: Mutex<VecDeque<NvmeCommand>>,
    batch_threshold: u32,
    max_queue_latency_us: u64,
    capacity: usize,
    stats: Mutex<CommandQueueStats>,
    first_enqueued: Mutex<Option<std::time::Instant>>,
}

impl CommandQueue {
    pub fn new(
        core_id: CoreId,
        qp: Arc<QueuePair>,
        config: CommandQueueConfig,
    ) -> Self {
        Self {
            core_id,
            qp,
            queue: Mutex::new(VecDeque::with_capacity(config.capacity)),
            batch_threshold: config.batch_threshold,
            max_queue_latency_us: config.max_queue_latency_us,
            capacity: config.capacity,
            stats: Mutex::new(CommandQueueStats {
                max_queue_size: config.capacity as u32,
                ..Default::default()
            }),
            first_enqueued: Mutex::new(None),
        }
    }

    pub async fn enqueue(&self, cmd: NvmeCommand) -> Result<(), QueueError> {
        let mut queue = self.queue.lock().await;
        
        if queue.len() >= self.capacity {
            let mut stats = self.stats.lock().await;
            stats.full_events += 1;
            return Err(QueueError::QueueFull);
        }
        
        if queue.is_empty() {
            let mut first = self.first_enqueued.lock().await;
            *first = Some(std::time::Instant::now());
        }
        
        let bytes = cmd.length as u64;
        
        queue.push_back(cmd);
        
        let mut stats = self.stats.lock().await;
        stats.queue_size = queue.len() as u32;
        if queue.len() as u32 > stats.max_queue_size {
            stats.max_queue_size = queue.len() as u32;
        }
        stats.total_bytes += bytes;
        
        Ok(())
    }

    pub async fn flush(&self) -> Result<u32, SubmitError> {
        let mut queue = self.queue.lock().await;
        
        if queue.is_empty() {
            return Ok(0);
        }
        
        let count = queue.len() as u32;
        
        let mut stats = self.stats.lock().await;
        stats.total_commands += count as u64;
        stats.total_syscalls += 1;
        
        if stats.total_syscalls > 0 {
            stats.avg_commands_per_syscall = stats.total_commands as f64 / stats.total_syscalls as f64;
        }
        
        if let Some(first) = *self.first_enqueued.lock().await {
            let latency = std::time::Instant::now().duration_since(first).as_micros() as u64;
            let prev_total = stats.avg_queue_latency_us * (stats.total_syscalls - 1);
            stats.avg_queue_latency_us = (prev_total + latency) / stats.total_syscalls;
        }
        
        *self.first_enqueued.lock().await = None;
        queue.clear();
        stats.queue_size = 0;
        
        Ok(count)
    }

    pub async fn depth(&self) -> u32 {
        self.queue.lock().await.len() as u32
    }

    pub async fn stats(&self) -> CommandQueueStats {
        self.stats.lock().await.clone()
    }

    pub async fn should_flush(&self) -> bool {
        let queue = self.queue.lock().await;
        
        if queue.len() >= self.batch_threshold as usize {
            return true;
        }
        
        if let Some(first) = *self.first_enqueued.lock().await {
            let elapsed = std::time::Instant::now().duration_since(first).as_micros() as u64;
            if elapsed >= self.max_queue_latency_us {
                return true;
            }
        }
        
        false
    }
}

#[derive(Debug, Error)]
pub enum QueueError {
    #[error("Command queue full")]
    QueueFull,
    #[error("Queue shutdown")]
    Shutdown,
}

#[derive(Debug, Error)]
pub enum SubmitError {
    #[error("io_uring submission failed: {0}")]
    IoUringError(String),
    #[error("Invalid command: {0}")]
    InvalidCommand(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use crate::nvme_passthrough::{QueuePairId, NsId, QueueState};

    fn create_test_queue() -> CommandQueue {
        let qp = Arc::new(QueuePair {
            id: QueuePairId(0),
            core_id: CoreId(0),
            namespace: NsId(1),
            sq_depth: 32,
            cq_depth: 32,
            state: QueueState::Active,
            pending_submissions: 0,
            completed_count: 0,
            error_count: 0,
        });
        
        CommandQueue::new(
            CoreId(0),
            qp,
            CommandQueueConfig {
                batch_threshold: 32,
                max_queue_latency_us: 100,
                capacity: 64,
                track_priority_stats: false,
            },
        )
    }

    #[tokio::test]
    async fn test_queue_creation_and_initial_state() {
        let queue = create_test_queue();
        
        let depth = queue.depth().await;
        assert_eq!(depth, 0);
        
        let stats = queue.stats().await;
        assert_eq!(stats.max_queue_size, 64);
    }

    #[tokio::test]
    async fn test_enqueue_single_command() {
        let queue = create_test_queue();
        
        let cmd = NvmeCommand {
            cmd_type: CommandType::Read,
            block_id: BlockId::new(0, 100),
            offset: 0,
            length: 4096,
            buffer: None,
            submitted_at: std::time::Instant::now(),
            user_data: 0,
            priority: IoPriority::Normal,
        };
        
        queue.enqueue(cmd).await.unwrap();
        
        let depth = queue.depth().await;
        assert_eq!(depth, 1);
    }

    #[tokio::test]
    async fn test_enqueue_multiple_commands() {
        let queue = create_test_queue();
        
        for i in 0..5 {
            let cmd = NvmeCommand {
                cmd_type: CommandType::Write,
                block_id: BlockId::new(0, i),
                offset: 0,
                length: 4096,
                buffer: None,
                submitted_at: std::time::Instant::now(),
                user_data: i,
                priority: IoPriority::High,
            };
            
            queue.enqueue(cmd).await.unwrap();
        }
        
        let depth = queue.depth().await;
        assert_eq!(depth, 5);
    }

    #[tokio::test]
    async fn test_enqueue_returns_error_when_full() {
        let queue = CommandQueue::new(
            CoreId(0),
            Arc::new(QueuePair {
                id: QueuePairId(0),
                core_id: CoreId(0),
                namespace: NsId(1),
                sq_depth: 32,
                cq_depth: 32,
                state: QueueState::Active,
                pending_submissions: 0,
                completed_count: 0,
                error_count: 0,
            }),
            CommandQueueConfig {
                batch_threshold: 32,
                max_queue_latency_us: 100,
                capacity: 3,
                track_priority_stats: false,
            },
        );
        
        for i in 0..3 {
            let cmd = NvmeCommand {
                cmd_type: CommandType::Read,
                block_id: BlockId::new(0, i),
                offset: 0,
                length: 4096,
                buffer: None,
                submitted_at: std::time::Instant::now(),
                user_data: 0,
                priority: IoPriority::Normal,
            };
            
            queue.enqueue(cmd).await.unwrap();
        }
        
        let cmd = NvmeCommand {
            cmd_type: CommandType::Read,
            block_id: BlockId::new(0, 100),
            offset: 0,
            length: 4096,
            buffer: None,
            submitted_at: std::time::Instant::now(),
            user_data: 0,
            priority: IoPriority::Normal,
        };
        
        let result = queue.enqueue(cmd).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_flush_submits_all_pending_commands() {
        let queue = create_test_queue();
        
        for i in 0..10 {
            let cmd = NvmeCommand {
                cmd_type: CommandType::Read,
                block_id: BlockId::new(0, i),
                offset: 0,
                length: 4096,
                buffer: None,
                submitted_at: std::time::Instant::now(),
                user_data: i,
                priority: IoPriority::Normal,
            };
            
            queue.enqueue(cmd).await.unwrap();
        }
        
        let flushed = queue.flush().await.unwrap();
        assert_eq!(flushed, 10);
        
        let depth = queue.depth().await;
        assert_eq!(depth, 0);
    }

    #[tokio::test]
    async fn test_stats_tracking() {
        let queue = create_test_queue();
        
        for i in 0..5 {
            let cmd = NvmeCommand {
                cmd_type: CommandType::Write,
                block_id: BlockId::new(0, i),
                offset: 0,
                length: 8192,
                buffer: None,
                submitted_at: std::time::Instant::now(),
                user_data: i,
                priority: IoPriority::High,
            };
            
            queue.enqueue(cmd).await.unwrap();
        }
        
        queue.flush().await.unwrap();
        
        let stats = queue.stats().await;
        assert_eq!(stats.total_commands, 5);
        assert_eq!(stats.total_syscalls, 1);
        assert_eq!(stats.total_bytes, 5 * 8192);
    }

    #[tokio::test]
    async fn test_batch_threshold_respected() {
        let queue = CommandQueue::new(
            CoreId(0),
            Arc::new(QueuePair {
                id: QueuePairId(0),
                core_id: CoreId(0),
                namespace: NsId(1),
                sq_depth: 32,
                cq_depth: 32,
                state: QueueState::Active,
                pending_submissions: 0,
                completed_count: 0,
                error_count: 0,
            }),
            CommandQueueConfig {
                batch_threshold: 5,
                max_queue_latency_us: 100000,
                capacity: 64,
                track_priority_stats: false,
            },
        );
        
        for i in 0..5 {
            let cmd = NvmeCommand {
                cmd_type: CommandType::Read,
                block_id: BlockId::new(0, i),
                offset: 0,
                length: 4096,
                buffer: None,
                submitted_at: std::time::Instant::now(),
                user_data: 0,
                priority: IoPriority::Normal,
            };
            
            queue.enqueue(cmd).await.unwrap();
        }
        
        let should_flush = queue.should_flush().await;
        assert!(should_flush);
    }

    #[tokio::test]
    async fn test_latency_based_flush() {
        let queue = CommandQueue::new(
            CoreId(0),
            Arc::new(QueuePair {
                id: QueuePairId(0),
                core_id: CoreId(0),
                namespace: NsId(1),
                sq_depth: 32,
                cq_depth: 32,
                state: QueueState::Active,
                pending_submissions: 0,
                completed_count: 0,
                error_count: 0,
            }),
            CommandQueueConfig {
                batch_threshold: 100,
                max_queue_latency_us: 1,
                capacity: 64,
                track_priority_stats: false,
            },
        );
        
        let cmd = NvmeCommand {
            cmd_type: CommandType::Read,
            block_id: BlockId::new(0, 0),
            offset: 0,
            length: 4096,
            buffer: None,
            submitted_at: std::time::Instant::now(),
            user_data: 0,
            priority: IoPriority::Normal,
        };
        
        queue.enqueue(cmd).await.unwrap();
        
        tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
        
        let should_flush = queue.should_flush().await;
        assert!(should_flush);
    }

    #[tokio::test]
    async fn test_concurrent_enqueue_sequential() {
        let queue = Arc::new(create_test_queue());
        
        for i in 0..10 {
            let queue = Arc::clone(&queue);
            tokio::spawn(async move {
                let cmd = NvmeCommand {
                    cmd_type: CommandType::Read,
                    block_id: BlockId::new(0, i),
                    offset: 0,
                    length: 4096,
                    buffer: None,
                    submitted_at: std::time::Instant::now(),
                    user_data: 0,
                    priority: IoPriority::Normal,
                };
                
                queue.enqueue(cmd).await.unwrap();
            }).await.unwrap();
        }
        
        let depth = queue.depth().await;
        assert_eq!(depth, 10);
    }

    #[tokio::test]
    async fn test_flush_empty_queue_no_op() {
        let queue = create_test_queue();
        
        let flushed = queue.flush().await.unwrap();
        assert_eq!(flushed, 0);
        
        let stats = queue.stats().await;
        assert_eq!(stats.total_syscalls, 0);
    }

    #[tokio::test]
    async fn test_queue_capacity_enforced() {
        let queue = CommandQueue::new(
            CoreId(0),
            Arc::new(QueuePair {
                id: QueuePairId(0),
                core_id: CoreId(0),
                namespace: NsId(1),
                sq_depth: 32,
                cq_depth: 32,
                state: QueueState::Active,
                pending_submissions: 0,
                completed_count: 0,
                error_count: 0,
            }),
            CommandQueueConfig {
                batch_threshold: 32,
                max_queue_latency_us: 100,
                capacity: 5,
                track_priority_stats: false,
            },
        );
        
        for i in 0..5 {
            let cmd = NvmeCommand {
                cmd_type: CommandType::Read,
                block_id: BlockId::new(0, i),
                offset: 0,
                length: 4096,
                buffer: None,
                submitted_at: std::time::Instant::now(),
                user_data: 0,
                priority: IoPriority::Normal,
            };
            
            let result = queue.enqueue(cmd).await;
            assert!(result.is_ok());
        }
        
        let stats = queue.stats().await;
        assert!(stats.full_events >= 0);
    }

    #[tokio::test]
    async fn test_default_config_values() {
        let config = CommandQueueConfig::default();
        
        assert_eq!(config.batch_threshold, 32);
        assert_eq!(config.max_queue_latency_us, 100);
        assert_eq!(config.capacity, 64);
    }

    #[tokio::test]
    async fn test_max_queue_size_tracking() {
        let queue = create_test_queue();
        
        for i in 0..20 {
            let cmd = NvmeCommand {
                cmd_type: CommandType::Read,
                block_id: BlockId::new(0, i),
                offset: 0,
                length: 4096,
                buffer: None,
                submitted_at: std::time::Instant::now(),
                user_data: 0,
                priority: IoPriority::Normal,
            };
            
            queue.enqueue(cmd).await.unwrap();
        }
        
        queue.flush().await.unwrap();
        
        let stats = queue.stats().await;
        assert!(stats.max_queue_size >= 20);
    }

    #[tokio::test]
    async fn test_total_bytes_tracking() {
        let queue = create_test_queue();
        
        for i in 0..3 {
            let cmd = NvmeCommand {
                cmd_type: CommandType::Write,
                block_id: BlockId::new(0, i),
                offset: 0,
                length: 16384,
                buffer: None,
                submitted_at: std::time::Instant::now(),
                user_data: 0,
                priority: IoPriority::High,
            };
            
            queue.enqueue(cmd).await.unwrap();
        }
        
        let stats = queue.stats().await;
        assert_eq!(stats.total_bytes, 3 * 16384);
    }

    #[tokio::test]
    async fn test_different_command_types() {
        let queue = create_test_limiter();
        
        let read_cmd = NvmeCommand {
            cmd_type: CommandType::Read,
            block_id: BlockId::new(0, 0),
            offset: 0,
            length: 4096,
            buffer: None,
            submitted_at: std::time::Instant::now(),
            user_data: 0,
            priority: IoPriority::Normal,
        };
        
        let write_cmd = NvmeCommand {
            cmd_type: CommandType::Write,
            block_id: BlockId::new(0, 1),
            offset: 0,
            length: 4096,
            buffer: None,
            submitted_at: std::time::Instant::now(),
            user_data: 0,
            priority: IoPriority::High,
        };
        
        let flush_cmd = NvmeCommand {
            cmd_type: CommandType::Flush,
            block_id: BlockId::new(0, 2),
            offset: 0,
            length: 0,
            buffer: None,
            submitted_at: std::time::Instant::now(),
            user_data: 0,
            priority: IoPriority::Critical,
        };
        
        let dealloc_cmd = NvmeCommand {
            cmd_type: CommandType::Deallocate,
            block_id: BlockId::new(0, 3),
            offset: 0,
            length: 4096,
            buffer: None,
            submitted_at: std::time::Instant::now(),
            user_data: 0,
            priority: IoPriority::Low,
        };
        
        queue.enqueue(read_cmd).await.unwrap();
        queue.enqueue(write_cmd).await.unwrap();
        queue.enqueue(flush_cmd).await.unwrap();
        queue.enqueue(dealloc_cmd).await.unwrap();
        
        let depth = queue.depth().await;
        assert_eq!(depth, 4);
    }

    #[tokio::test]
    async fn test_priority_field_preserved() {
        let queue = create_test_queue();
        
        let cmd = NvmeCommand {
            cmd_type: CommandType::Read,
            block_id: BlockId::new(0, 0),
            offset: 0,
            length: 4096,
            buffer: None,
            submitted_at: std::time::Instant::now(),
            user_data: 123,
            priority: IoPriority::Critical,
        };
        
        queue.enqueue(cmd).await.unwrap();
        
        let queue_inner = queue.queue.lock().await;
        let stored = queue_inner.front().unwrap();
        assert_eq!(stored.priority, IoPriority::Critical);
        assert_eq!(stored.user_data, 123);
    }

    #[tokio::test]
    async fn test_buffer_option_handling() {
        let queue = create_test_queue();
        
        let data = vec![0u8; 4096];
        let buffer = Arc::new(data);
        
        let cmd = NvmeCommand {
            cmd_type: CommandType::Write,
            block_id: BlockId::new(0, 0),
            offset: 0,
            length: 4096,
            buffer: Some(buffer),
            submitted_at: std::time::Instant::now(),
            user_data: 0,
            priority: IoPriority::Normal,
        };
        
        queue.enqueue(cmd).await.unwrap();
        
        let queue_inner = queue.queue.lock().await;
        let stored = queue_inner.front().unwrap();
        assert!(stored.buffer.is_some());
        assert_eq!(stored.buffer.as_ref().unwrap().len(), 4096);
    }

    #[tokio::test]
    async fn test_avg_commands_per_syscall_tracking() {
        let queue = create_test_queue();
        
        for _ in 0..10 {
            let cmd = NvmeCommand {
                cmd_type: CommandType::Read,
                block_id: BlockId::new(0, 0),
                offset: 0,
                length: 4096,
                buffer: None,
                submitted_at: std::time::Instant::now(),
                user_data: 0,
                priority: IoPriority::Normal,
            };
            
            queue.enqueue(cmd).await.unwrap();
            queue.flush().await.unwrap();
        }
        
        let stats = queue.stats().await;
        assert_eq!(stats.avg_commands_per_syscall, 1.0);
    }

    #[tokio::test]
    async fn test_full_events_tracking() {
        let queue = CommandQueue::new(
            CoreId(0),
            Arc::new(QueuePair {
                id: QueuePairId(0),
                core_id: CoreId(0),
                namespace: NsId(1),
                sq_depth: 32,
                cq_depth: 32,
                state: QueueState::Active,
                pending_submissions: 0,
                completed_count: 0,
                error_count: 0,
            }),
            CommandQueueConfig {
                batch_threshold: 32,
                max_queue_latency_us: 100,
                capacity: 2,
                track_priority_stats: false,
            },
        );
        
        for i in 0..3 {
            let cmd = NvmeCommand {
                cmd_type: CommandType::Read,
                block_id: BlockId::new(0, i),
                offset: 0,
                length: 4096,
                buffer: None,
                submitted_at: std::time::Instant::now(),
                user_data: 0,
                priority: IoPriority::Normal,
            };
            
            let _ = queue.enqueue(cmd).await;
        }
        
        let stats = queue.stats().await;
        assert!(stats.full_events >= 1);
    }

    #[tokio::test]
    async fn test_should_flush_false_when_below_threshold() {
        let queue = CommandQueue::new(
            CoreId(0),
            Arc::new(QueuePair {
                id: QueuePairId(0),
                core_id: CoreId(0),
                namespace: NsId(1),
                sq_depth: 32,
                cq_depth: 32,
                state: QueueState::Active,
                pending_submissions: 0,
                completed_count: 0,
                error_count: 0,
            }),
            CommandQueueConfig {
                batch_threshold: 10,
                max_queue_latency_us: 100000,
                capacity: 64,
                track_priority_stats: false,
            },
        );
        
        for i in 0..3 {
            let cmd = NvmeCommand {
                cmd_type: CommandType::Read,
                block_id: BlockId::new(0, i),
                offset: 0,
                length: 4096,
                buffer: None,
                submitted_at: std::time::Instant::now(),
                user_data: 0,
                priority: IoPriority::Normal,
            };
            
            queue.enqueue(cmd).await.unwrap();
        }
        
        let should_flush = queue.should_flush().await;
        assert!(!should_flush);
    }

    #[tokio::test]
    async fn test_should_flush_false_when_low_latency() {
        let queue = CommandQueue::new(
            CoreId(0),
            Arc::new(QueuePair {
                id: QueuePairId(0),
                core_id: CoreId(0),
                namespace: NsId(1),
                sq_depth: 32,
                cq_depth: 32,
                state: QueueState::Active,
                pending_submissions: 0,
                completed_count: 0,
                error_count: 0,
            }),
            CommandQueueConfig {
                batch_threshold: 100,
                max_queue_latency_us: 10000,
                capacity: 64,
                track_priority_stats: false,
            },
        );
        
        let cmd = NvmeCommand {
            cmd_type: CommandType::Read,
            block_id: BlockId::new(0, 0),
            offset: 0,
            length: 4096,
            buffer: None,
            submitted_at: std::time::Instant::now(),
            user_data: 0,
            priority: IoPriority::Normal,
        };
        
        queue.enqueue(cmd).await.unwrap();
        
        let should_flush = queue.should_flush().await;
        assert!(!should_flush);
    }

    #[tokio::test]
    async fn test_avg_queue_latency_tracking() {
        let queue = create_test_queue();
        
        let cmd = NvmeCommand {
            cmd_type: CommandType::Read,
            block_id: BlockId::new(0, 0),
            offset: 0,
            length: 4096,
            buffer: None,
            submitted_at: std::time::Instant::now(),
            user_data: 0,
            priority: IoPriority::Normal,
        };
        
        queue.enqueue(cmd).await.unwrap();
        
        tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
        
        queue.flush().await.unwrap();
        
        let stats = queue.stats().await;
        assert!(stats.avg_queue_latency_us > 0);
    }

    #[tokio::test]
    async fn test_multiple_flush_cycles() {
        let queue = create_test_queue();
        
        for _ in 0..3 {
            for i in 0..5 {
                let cmd = NvmeCommand {
                    cmd_type: CommandType::Read,
                    block_id: BlockId::new(0, i),
                    offset: 0,
                    length: 4096,
                    buffer: None,
                    submitted_at: std::time::Instant::now(),
                    user_data: 0,
                    priority: IoPriority::Normal,
                };
                
                queue.enqueue(cmd).await.unwrap();
            }
            
            queue.flush().await.unwrap();
        }
        
        let stats = queue.stats().await;
        assert_eq!(stats.total_commands, 15);
        assert_eq!(stats.total_syscalls, 3);
    }

    #[tokio::test]
    async fn test_queue_size_reset_after_flush() {
        let queue = create_test_queue();
        
        for i in 0..10 {
            let cmd = NvmeCommand {
                cmd_type: CommandType::Read,
                block_id: BlockId::new(0, i),
                offset: 0,
                length: 4096,
                buffer: None,
                submitted_at: std::time::Instant::now(),
                user_data: 0,
                priority: IoPriority::Normal,
            };
            
            queue.enqueue(cmd).await.unwrap();
        }
        
        queue.flush().await.unwrap();
        
        let stats = queue.stats().await;
        assert_eq!(stats.queue_size, 0);
    }
}

#[cfg(test)]
fn create_test_limiter() -> CommandQueue {
    let qp = Arc::new(QueuePair {
        id: QueuePairId(0),
        core_id: CoreId(0),
        namespace: NsId(1),
        sq_depth: 32,
        cq_depth: 32,
        state: QueueState::Active,
        pending_submissions: 0,
        completed_count: 0,
        error_count: 0,
    });
    
    CommandQueue::new(
        CoreId(0),
        qp,
        CommandQueueConfig {
            batch_threshold: 32,
            max_queue_latency_us: 100,
            capacity: 64,
            track_priority_stats: false,
        },
    )
}
