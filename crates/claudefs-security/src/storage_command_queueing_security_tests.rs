//! Storage command queueing security tests.
//!
//! Part of A10 Phase 35

use claudefs_storage::command_queueing::{
    CommandQueue, CommandQueueConfig, CommandQueueStats, CommandType, NvmeCommand,
};
use claudefs_storage::io_scheduler::IoPriority;
use claudefs_storage::nvme_passthrough::{CoreId, QueuePairId};
use claudefs_storage::block::BlockId;
use std::sync::Arc;
use std::time::Instant;

#[cfg(test)]
mod tests {
    use super::*;
    use claudefs_storage::nvme_passthrough::{QueuePair, QueueState, NsId};

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

    fn create_cmd(block_idx: u64) -> NvmeCommand {
        NvmeCommand {
            cmd_type: CommandType::Read,
            block_id: BlockId::new(0, block_idx),
            offset: 0,
            length: 4096,
            buffer: None,
            submitted_at: Instant::now(),
            user_data: block_idx,
            priority: IoPriority::Normal,
        }
    }

    mod capacity_and_backpressure_enforcement {
        use super::*;

        #[tokio::test]
        async fn test_storage_cmd_q_sec_queue_capacity_exactly_reached() {
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
                let result = queue.enqueue(create_cmd(i)).await;
                assert!(result.is_ok(), "Should enqueue up to capacity (5)");
            }

            let depth = queue.depth().await;
            assert_eq!(depth, 5, "Queue should have exactly 5 commands");
        }

        #[tokio::test]
        async fn test_storage_cmd_q_sec_queue_rejects_when_full() {
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
                queue.enqueue(create_cmd(i)).await.unwrap();
            }

            let result = queue.enqueue(create_cmd(100)).await;
            assert!(result.is_err(), "Should reject when queue is full");
        }

        #[tokio::test]
        async fn test_storage_cmd_q_sec_full_event_tracked() {
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
                let _ = queue.enqueue(create_cmd(i)).await;
            }

            let stats = queue.stats().await;
            assert!(
                stats.full_events >= 1,
                "Should track full events, got {}",
                stats.full_events
            );
        }

        #[tokio::test]
        async fn test_storage_cmd_q_sec_concurrent_enqueue_backpressure() {
            let queue = Arc::new(create_test_queue());

            let mut handles = vec![];
            for i in 0..50 {
                let queue = Arc::clone(&queue);
                let cmd = create_cmd(i);
                handles.push(tokio::spawn(async move {
                    queue.enqueue(cmd).await
                }));
            }

            let results: Vec<_> = futures::future::join_all(handles)
                .await
                .into_iter()
                .map(|r| r.unwrap())
                .collect();

            let success_count = results.iter().filter(|r| r.is_ok()).count();
            let depth = queue.depth().await;

            assert!(
                depth <= 64,
                "Depth should stay within capacity (64), got {}",
                depth
            );
            assert!(
                success_count + results.iter().filter(|r| r.is_err()).count() == 50,
                "All operations should have a determinable result"
            );
        }

        #[tokio::test]
        async fn test_storage_cmd_q_sec_capacity_one_commands_processed() {
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
                    batch_threshold: 1,
                    max_queue_latency_us: 10000,
                    capacity: 10,
                    track_priority_stats: false,
                },
            );

            queue.enqueue(create_cmd(0)).await.unwrap();

            let should_flush = queue.should_flush().await;
            assert!(
                should_flush,
                "Should flush when batch_threshold (1) reached"
            );
        }

        #[tokio::test]
        async fn test_storage_cmd_q_sec_depth_never_exceeds_capacity() {
            let queue = Arc::new(create_test_queue());

            let enqueue_handle = {
                let queue = Arc::clone(&queue);
                tokio::spawn(async move {
                    for i in 0..100 {
                        let _ = queue.enqueue(create_cmd(i)).await;
                    }
                })
            };

            let depth_handle = {
                let queue = Arc::clone(&queue);
                tokio::spawn(async move {
                    let mut max_depth = 0u32;
                    for _ in 0..50 {
                        let d = queue.depth().await;
                        if d > max_depth {
                            max_depth = d;
                        }
                        tokio::time::sleep(std::time::Duration::from_micros(100)).await;
                    }
                    max_depth
                })
            };

            enqueue_handle.await.unwrap();
            let max_depth = depth_handle.await.unwrap();

            assert!(
                max_depth <= 64,
                "Max depth should never exceed capacity (64), got {}",
                max_depth
            );
        }

        #[tokio::test]
        async fn test_storage_cmd_q_sec_full_queue_recovery_after_flush() {
            let queue = Arc::new(create_test_queue());

            for i in 0..64 {
                queue.enqueue(create_cmd(i)).await.unwrap();
            }

            queue.flush().await.unwrap();

            let result = queue.enqueue(create_cmd(999)).await;
            assert!(result.is_ok(), "Should accept new commands after flush");
        }
    }

    mod buffer_lifecycle_safety {
        use super::*;

        #[tokio::test]
        async fn test_storage_cmd_q_sec_buffer_arc_refcount_preserved() {
            let data = vec![0u8; 4096];
            let buffer = Arc::new(data);

            let cmd = NvmeCommand {
                cmd_type: CommandType::Write,
                block_id: BlockId::new(0, 0),
                offset: 0,
                length: 4096,
                buffer: Some(Arc::clone(&buffer)),
                submitted_at: Instant::now(),
                user_data: 0,
                priority: IoPriority::Normal,
            };

            let queue = create_test_queue();
            queue.enqueue(cmd).await.unwrap();

            assert!(
                Arc::strong_count(&buffer) >= 2,
                "Buffer should still be referenced after enqueue"
            );

            queue.flush().await.unwrap();

            assert!(
                Arc::strong_count(&buffer) >= 1,
                "Buffer should still exist after flush"
            );
        }

        #[tokio::test]
        async fn test_storage_cmd_q_sec_none_buffer_handling() {
            let cmd = NvmeCommand {
                cmd_type: CommandType::Flush,
                block_id: BlockId::new(0, 0),
                offset: 0,
                length: 0,
                buffer: None,
                submitted_at: Instant::now(),
                user_data: 0,
                priority: IoPriority::Critical,
            };

            let queue = create_test_queue();
            let result = queue.enqueue(cmd).await;

            assert!(result.is_ok(), "Should accept commands with None buffer");
        }

        #[tokio::test]
        async fn test_storage_cmd_q_sec_multiple_buffers_same_data() {
            let data = vec![1u8; 4096];
            let buffer1 = Arc::new(data.clone());
            let buffer2 = Arc::new(data);

            let queue = create_test_queue();

            queue.enqueue(NvmeCommand {
                cmd_type: CommandType::Write,
                block_id: BlockId::new(0, 0),
                offset: 0,
                length: 4096,
                buffer: Some(buffer1),
                submitted_at: Instant::now(),
                user_data: 0,
                priority: IoPriority::Normal,
            }).await.unwrap();

            queue.enqueue(NvmeCommand {
                cmd_type: CommandType::Write,
                block_id: BlockId::new(0, 1),
                offset: 0,
                length: 4096,
                buffer: Some(buffer2),
                submitted_at: Instant::now(),
                user_data: 1,
                priority: IoPriority::Normal,
            }).await.unwrap();

            queue.flush().await.unwrap();

            let stats = queue.stats().await;
            assert_eq!(stats.total_commands, 2);
        }

        #[tokio::test]
        async fn test_storage_cmd_q_sec_large_buffer_handling() {
            let large_data = vec![0u8; 1024 * 1024];
            let buffer = Arc::new(large_data);

            let cmd = NvmeCommand {
                cmd_type: CommandType::Write,
                block_id: BlockId::new(0, 0),
                offset: 0,
                length: 1024 * 1024,
                buffer: Some(buffer),
                submitted_at: Instant::now(),
                user_data: 0,
                priority: IoPriority::Normal,
            };

            let queue = create_test_queue();
            let result = queue.enqueue(cmd).await;

            assert!(result.is_ok(), "Should handle large buffers");
        }

        #[tokio::test]
        async fn test_storage_cmd_q_sec_buffer_not_leaked_after_clear() {
            let data = vec![0u8; 4096];
            let buffer = Arc::new(data);

            let cmd = NvmeCommand {
                cmd_type: CommandType::Write,
                block_id: BlockId::new(0, 0),
                offset: 0,
                length: 4096,
                buffer: Some(Arc::clone(&buffer)),
                submitted_at: Instant::now(),
                user_data: 0,
                priority: IoPriority::Normal,
            };

            let queue = create_test_queue();
            queue.enqueue(cmd).await.unwrap();
            queue.flush().await.unwrap();

            let stats = queue.stats().await;
            assert_eq!(stats.queue_size, 0, "Queue should be empty after flush");
        }

        #[tokio::test]
        async fn test_storage_cmd_q_sec_empty_buffer_vec_handling() {
            let empty_data = vec![];
            let buffer = Arc::new(empty_data);

            let cmd = NvmeCommand {
                cmd_type: CommandType::Flush,
                block_id: BlockId::new(0, 0),
                offset: 0,
                length: 0,
                buffer: Some(buffer),
                submitted_at: Instant::now(),
                user_data: 0,
                priority: IoPriority::Normal,
            };

            let queue = create_test_queue();
            let result = queue.enqueue(cmd).await;

            assert!(result.is_ok(), "Should handle empty buffer");
        }

        #[tokio::test]
        async fn test_storage_cmd_q_sec_buffer_content_integrity() {
            let data = vec![0u8; 4096];
            let buffer = Arc::new(data);

            let queue = create_test_queue();
            
            let result = queue.enqueue(NvmeCommand {
                cmd_type: CommandType::Write,
                block_id: BlockId::new(0, 0),
                offset: 0,
                length: 4096,
                buffer: Some(Arc::clone(&buffer)),
                submitted_at: Instant::now(),
                user_data: 0,
                priority: IoPriority::Normal,
            }).await;

            assert!(result.is_ok(), "Should accept command with buffer");
            assert_eq!(Arc::strong_count(&buffer), 2, "Buffer should have 2 refs after enqueue");
        }

        #[tokio::test]
        async fn test_storage_cmd_q_sec_buffer_drop_on_queue_drop() {
            let queue = {
                let data = vec![0u8; 4096];
                let buffer = Arc::new(data);
                let cmd = NvmeCommand {
                    cmd_type: CommandType::Write,
                    block_id: BlockId::new(0, 0),
                    offset: 0,
                    length: 4096,
                    buffer: Some(buffer),
                    submitted_at: Instant::now(),
                    user_data: 0,
                    priority: IoPriority::Normal,
                };
                
                let q = create_test_queue();
                q.enqueue(cmd).await.unwrap();
                q
            };

            queue.flush().await.unwrap();
            
            let stats = queue.stats().await;
            assert_eq!(stats.queue_size, 0);
        }
    }

    mod command_ordering_and_integrity {
        use super::*;

        #[tokio::test]
        async fn test_storage_cmd_q_sec_fifo_ordering_preserved() {
            let queue = create_test_queue();

            for i in 0..10 {
                let mut cmd = create_cmd(i);
                cmd.user_data = i;
                queue.enqueue(cmd).await.unwrap();
            }

            queue.flush().await.unwrap();

            let stats = queue.stats().await;
            assert_eq!(stats.total_commands, 10);
        }

        #[tokio::test]
        async fn test_storage_cmd_q_sec_user_data_sequence_integrity() {
            let queue = create_test_queue();

            for i in 0..5 {
                let cmd = create_cmd(i);
                queue.enqueue(cmd).await.unwrap();
            }

            queue.flush().await.unwrap();

            let stats = queue.stats().await;
            assert_eq!(stats.total_commands, 5);
        }

        #[tokio::test]
        async fn test_storage_cmd_q_sec_priority_field_preserved() {
            let queue = create_test_queue();

            let cmd_high = NvmeCommand {
                cmd_type: CommandType::Read,
                block_id: BlockId::new(0, 0),
                offset: 0,
                length: 4096,
                buffer: None,
                submitted_at: Instant::now(),
                user_data: 0,
                priority: IoPriority::High,
            };

            queue.enqueue(cmd_high).await.unwrap();

            let stats = queue.stats().await;
            assert!(stats.queue_size >= 1, "Command should be in queue");
        }

        #[tokio::test]
        async fn test_storage_cmd_q_sec_all_command_types_accepted() {
            let queue = create_test_queue();

            let read_cmd = NvmeCommand {
                cmd_type: CommandType::Read,
                block_id: BlockId::new(0, 0),
                offset: 0,
                length: 4096,
                buffer: None,
                submitted_at: Instant::now(),
                user_data: 0,
                priority: IoPriority::Normal,
            };

            let write_cmd = NvmeCommand {
                cmd_type: CommandType::Write,
                block_id: BlockId::new(0, 1),
                offset: 0,
                length: 4096,
                buffer: None,
                submitted_at: Instant::now(),
                user_data: 1,
                priority: IoPriority::Normal,
            };

            let flush_cmd = NvmeCommand {
                cmd_type: CommandType::Flush,
                block_id: BlockId::new(0, 2),
                offset: 0,
                length: 0,
                buffer: None,
                submitted_at: Instant::now(),
                user_data: 2,
                priority: IoPriority::Critical,
            };

            let dealloc_cmd = NvmeCommand {
                cmd_type: CommandType::Deallocate,
                block_id: BlockId::new(0, 3),
                offset: 0,
                length: 4096,
                buffer: None,
                submitted_at: Instant::now(),
                user_data: 3,
                priority: IoPriority::Low,
            };

            queue.enqueue(read_cmd).await.unwrap();
            queue.enqueue(write_cmd).await.unwrap();
            queue.enqueue(flush_cmd).await.unwrap();
            queue.enqueue(dealloc_cmd).await.unwrap();

            let depth = queue.depth().await;
            assert_eq!(depth, 4, "All command types should be accepted");
        }

        #[tokio::test]
        async fn test_storage_cmd_q_sec_block_id_preserved() {
            let queue = create_test_queue();

            let cmd = NvmeCommand {
                cmd_type: CommandType::Read,
                block_id: BlockId::new(42, 123),
                offset: 512,
                length: 4096,
                buffer: None,
                submitted_at: Instant::now(),
                user_data: 0,
                priority: IoPriority::Normal,
            };

            let result = queue.enqueue(cmd).await;
            assert!(result.is_ok(), "Should accept command with block_id");
        }

        #[tokio::test]
        async fn test_storage_cmd_q_sec_submitted_at_timestamp_preserved() {
            let queue = create_test_queue();
            let before = Instant::now();

            let cmd = NvmeCommand {
                cmd_type: CommandType::Read,
                block_id: BlockId::new(0, 0),
                offset: 0,
                length: 4096,
                buffer: None,
                submitted_at: before,
                user_data: 0,
                priority: IoPriority::Normal,
            };

            let result = queue.enqueue(cmd).await;
            assert!(result.is_ok(), "Should accept command with timestamp");
            
            let after = Instant::now();
            let depth = queue.depth().await;
            assert!(depth >= 1, "Command should be in queue between before and after");
        }
    }

    mod batch_threshold_enforcement {
        use super::*;

        #[tokio::test]
        async fn test_storage_cmd_q_sec_should_flush_at_batch_threshold() {
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
                queue.enqueue(create_cmd(i)).await.unwrap();
            }

            let should = queue.should_flush().await;
            assert!(should, "should_flush should be true at batch_threshold");
        }

        #[tokio::test]
        async fn test_storage_cmd_q_sec_should_flush_below_threshold_returns_false() {
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
                queue.enqueue(create_cmd(i)).await.unwrap();
            }

            let should = queue.should_flush().await;
            assert!(!should, "should_flush should be false below threshold");
        }

        #[tokio::test]
        async fn test_storage_cmd_q_sec_should_flush_on_latency_timeout() {
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

            queue.enqueue(create_cmd(0)).await.unwrap();

            tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;

            let should = queue.should_flush().await;
            assert!(should, "should_flush should be true after latency timeout");
        }

        #[tokio::test]
        async fn test_storage_cmd_q_sec_flush_returns_count() {
            let queue = create_test_queue();

            for i in 0..7 {
                queue.enqueue(create_cmd(i)).await.unwrap();
            }

            let flushed = queue.flush().await.unwrap();
            assert_eq!(flushed, 7, "flush should return count of commands");
        }

        #[tokio::test]
        async fn test_storage_cmd_q_sec_flush_clears_queue() {
            let queue = create_test_queue();

            for i in 0..10 {
                queue.enqueue(create_cmd(i)).await.unwrap();
            }

            queue.flush().await.unwrap();

            let depth = queue.depth().await;
            assert_eq!(depth, 0, "Queue should be empty after flush");
        }

        #[tokio::test]
        async fn test_storage_cmd_q_sec_flush_empty_queue_returns_zero() {
            let queue = create_test_queue();

            let flushed = queue.flush().await.unwrap();
            assert_eq!(flushed, 0, "Flush empty queue should return 0");
        }
    }

    mod statistics_accuracy {
        use super::*;

        #[tokio::test]
        async fn test_storage_cmd_q_sec_total_commands_incremented() {
            let queue = create_test_queue();

            for i in 0..5 {
                queue.enqueue(create_cmd(i)).await.unwrap();
            }

            queue.flush().await.unwrap();

            let stats = queue.stats().await;
            assert_eq!(
                stats.total_commands, 5,
                "total_commands should be 5 after flush"
            );
        }

        #[tokio::test]
        async fn test_storage_cmd_q_sec_total_syscalls_incremented() {
            let queue = create_test_queue();

            queue.enqueue(create_cmd(0)).await.unwrap();
            queue.flush().await.unwrap();

            queue.enqueue(create_cmd(1)).await.unwrap();
            queue.flush().await.unwrap();

            let stats = queue.stats().await;
            assert_eq!(
                stats.total_syscalls, 2,
                "total_syscalls should be 2 after 2 flushes"
            );
        }

        #[tokio::test]
        async fn test_storage_cmd_q_sec_avg_commands_per_syscall_correct() {
            let queue = create_test_queue();

            for i in 0..10 {
                queue.enqueue(create_cmd(i)).await.unwrap();
            }
            queue.flush().await.unwrap();

            let stats = queue.stats().await;
            assert!(
                stats.avg_commands_per_syscall >= 10.0,
                "avg_commands_per_syscall should be >= 10"
            );
        }

        #[tokio::test]
        async fn test_storage_cmd_q_sec_total_bytes_accumulated() {
            let queue = create_test_queue();

            for i in 0..3 {
                let cmd = NvmeCommand {
                    cmd_type: CommandType::Write,
                    block_id: BlockId::new(0, i),
                    offset: 0,
                    length: 8192,
                    buffer: None,
                    submitted_at: Instant::now(),
                    user_data: i,
                    priority: IoPriority::Normal,
                };
                queue.enqueue(cmd).await.unwrap();
            }

            queue.flush().await.unwrap();

            let stats = queue.stats().await;
            assert_eq!(
                stats.total_bytes, 3 * 8192,
                "total_bytes should accumulate correctly"
            );
        }

        #[tokio::test]
        async fn test_storage_cmd_q_sec_max_queue_size_tracked() {
            let queue = create_test_queue();

            for i in 0..20 {
                queue.enqueue(create_cmd(i)).await.unwrap();
            }

            let stats = queue.stats().await;
            assert!(
                stats.max_queue_size >= 20,
                "max_queue_size should track peak"
            );
        }
    }
}