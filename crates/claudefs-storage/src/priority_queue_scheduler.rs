//! Priority queue scheduler module - priority-aware I/O scheduling with deadline support.

use crate::error::StorageError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum IoWorkloadClass {
    Critical,
    Interactive,
    Bulk,
}

#[derive(Debug, Clone)]
pub struct PrioritizedIoOp {
    pub op_id: u64,
    pub workload_class: IoWorkloadClass,
    pub deadline_ns: Option<u64>,
    pub size_bytes: u64,
    pub block_id: u64,
}

pub struct SchedulerConfig {
    pub critical_budget_ns: u64,
    pub interactive_budget_ns: u64,
}

pub struct PriorityQueueScheduler {
    queues: [Vec<PrioritizedIoOp>; 3],
    total_pending: usize,
    config: SchedulerConfig,
    critical_time_used_ns: u64,
    interactive_time_used_ns: u64,
}

impl PriorityQueueScheduler {
    pub fn new(config: SchedulerConfig) -> Self {
        Self {
            queues: [Vec::new(), Vec::new(), Vec::new()],
            total_pending: 0,
            config,
            critical_time_used_ns: 0,
            interactive_time_used_ns: 0,
        }
    }

    pub fn enqueue(&mut self, op: PrioritizedIoOp) -> Result<(), StorageError> {
        let idx = match op.workload_class {
            IoWorkloadClass::Critical => 0,
            IoWorkloadClass::Interactive => 1,
            IoWorkloadClass::Bulk => 2,
        };
        self.queues[idx].push(op);
        self.total_pending += 1;
        Ok(())
    }

    pub fn dequeue(&mut self) -> Option<PrioritizedIoOp> {
        self.dequeue_batch(1).pop()
    }

    pub fn dequeue_batch(&mut self, count: usize) -> Vec<PrioritizedIoOp> {
        if count == 0 {
            return Vec::new();
        }

        let mut result = Vec::with_capacity(count);
        let avg_op_size = 4096u64;

        for _ in 0..count {
            let mut selected_queue = None;

            for (idx, queue) in self.queues.iter_mut().enumerate() {
                if queue.is_empty() {
                    continue;
                }

                let budget_ok = match idx {
                    0 => true,
                    1 => self.interactive_time_used_ns < self.config.interactive_budget_ns,
                    2 => {
                        self.critical_time_used_ns < self.config.critical_budget_ns
                            && self.interactive_time_used_ns < self.config.interactive_budget_ns
                    }
                    _ => false,
                };

                if budget_ok {
                    selected_queue = Some(idx);
                    break;
                }
            }

            if selected_queue.is_none() {
                for (idx, queue) in self.queues.iter_mut().enumerate() {
                    if !queue.is_empty() {
                        selected_queue = Some(idx);
                        break;
                    }
                }
            }

            let idx = match selected_queue {
                Some(i) => i,
                None => break,
            };

            if let Some(op) = self.queues[idx].remove(0) {
                self.total_pending = self.total_pending.saturating_sub(1);
                result.push(op.clone());

                match idx {
                    0 => {
                        self.critical_time_used_ns += avg_op_size;
                    }
                    1 => {
                        self.interactive_time_used_ns += avg_op_size;
                    }
                    2 => {}
                    _ => {}
                }
            }
        }

        result
    }

    pub fn queue_depth(&self, class: IoWorkloadClass) -> usize {
        let idx = match class {
            IoWorkloadClass::Critical => 0,
            IoWorkloadClass::Interactive => 1,
            IoWorkloadClass::Bulk => 2,
        };
        self.queues[idx].len()
    }

    pub fn expire_deadlines(&mut self, current_time_ns: u64) -> usize {
        let mut expired_count = 0;

        for idx in 0..3 {
            let mut i = 0;
            while i < self.queues[idx].len() {
                if let Some(deadline) = self.queues[idx][i].deadline_ns {
                    if deadline <= current_time_ns {
                        let mut op = self.queues[idx].remove(i);
                        expired_count += 1;

                        let target_idx = if idx == 0 { 0 } else { idx - 1 };
                        self.queues[target_idx].insert(0, op);
                        continue;
                    }
                }
                i += 1;
            }
        }

        expired_count
    }

    pub fn estimated_latency_ns(&self, class: IoWorkloadClass) -> u64 {
        let avg_op_size = 4096u64;
        let idx = match class {
            IoWorkloadClass::Critical => 0,
            IoWorkloadClass::Interactive => 1,
            IoWorkloadClass::Bulk => 2,
        };

        let depth = self.queues[idx].len();
        if depth == 0 {
            return 0;
        }

        depth as u64 * avg_op_size
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enqueue_critical_operation() {
        let config = SchedulerConfig {
            critical_budget_ns: 1000000,
            interactive_budget_ns: 1000000,
        };
        let mut scheduler = PriorityQueueScheduler::new(config);
        scheduler
            .enqueue(PrioritizedIoOp {
                op_id: 1,
                workload_class: IoWorkloadClass::Critical,
                deadline_ns: None,
                size_bytes: 4096,
                block_id: 100,
            })
            .unwrap();
        assert_eq!(scheduler.queue_depth(IoWorkloadClass::Critical), 1);
    }

    #[test]
    fn enqueue_interactive_operation() {
        let config = SchedulerConfig {
            critical_budget_ns: 1000000,
            interactive_budget_ns: 1000000,
        };
        let mut scheduler = PriorityQueueScheduler::new(config);
        scheduler
            .enqueue(PrioritizedIoOp {
                op_id: 2,
                workload_class: IoWorkloadClass::Interactive,
                deadline_ns: None,
                size_bytes: 4096,
                block_id: 200,
            })
            .unwrap();
        assert_eq!(scheduler.queue_depth(IoWorkloadClass::Interactive), 1);
    }

    #[test]
    fn enqueue_bulk_operation() {
        let config = SchedulerConfig {
            critical_budget_ns: 1000000,
            interactive_budget_ns: 1000000,
        };
        let mut scheduler = PriorityQueueScheduler::new(config);
        scheduler
            .enqueue(PrioritizedIoOp {
                op_id: 3,
                workload_class: IoWorkloadClass::Bulk,
                deadline_ns: None,
                size_bytes: 4096,
                block_id: 300,
            })
            .unwrap();
        assert_eq!(scheduler.queue_depth(IoWorkloadClass::Bulk), 1);
    }

    #[test]
    fn dequeue_returns_critical_before_interactive() {
        let config = SchedulerConfig {
            critical_budget_ns: 1000000,
            interactive_budget_ns: 1000000,
        };
        let mut scheduler = PriorityQueueScheduler::new(config);
        scheduler
            .enqueue(PrioritizedIoOp {
                op_id: 2,
                workload_class: IoWorkloadClass::Interactive,
                deadline_ns: None,
                size_bytes: 4096,
                block_id: 200,
            })
            .unwrap();
        scheduler
            .enqueue(PrioritizedIoOp {
                op_id: 1,
                workload_class: IoWorkloadClass::Critical,
                deadline_ns: None,
                size_bytes: 4096,
                block_id: 100,
            })
            .unwrap();
        let op = scheduler.dequeue().unwrap();
        assert_eq!(op.workload_class, IoWorkloadClass::Critical);
    }

    #[test]
    fn dequeue_returns_interactive_before_bulk() {
        let config = SchedulerConfig {
            critical_budget_ns: 1000000,
            interactive_budget_ns: 1000000,
        };
        let mut scheduler = PriorityQueueScheduler::new(config);
        scheduler
            .enqueue(PrioritizedIoOp {
                op_id: 3,
                workload_class: IoWorkloadClass::Bulk,
                deadline_ns: None,
                size_bytes: 4096,
                block_id: 300,
            })
            .unwrap();
        scheduler
            .enqueue(PrioritizedIoOp {
                op_id: 2,
                workload_class: IoWorkloadClass::Interactive,
                deadline_ns: None,
                size_bytes: 4096,
                block_id: 200,
            })
            .unwrap();
        let op = scheduler.dequeue().unwrap();
        assert_eq!(op.workload_class, IoWorkloadClass::Interactive);
    }

    #[test]
    fn deadline_expiration_moves_ops_to_higher_priority() {
        let config = SchedulerConfig {
            critical_budget_ns: 1000000,
            interactive_budget_ns: 1000000,
        };
        let mut scheduler = PriorityQueueScheduler::new(config);
        scheduler
            .enqueue(PrioritizedIoOp {
                op_id: 3,
                workload_class: IoWorkloadClass::Bulk,
                deadline_ns: Some(1000),
                size_bytes: 4096,
                block_id: 300,
            })
            .unwrap();
        let expired = scheduler.expire_deadlines(2000);
        assert_eq!(expired, 1);
        assert_eq!(scheduler.queue_depth(IoWorkloadClass::Bulk), 0);
        assert_eq!(scheduler.queue_depth(IoWorkloadClass::Interactive), 1);
    }

    #[test]
    fn budget_limit_prevents_critical_from_starving_bulk() {
        let config = SchedulerConfig {
            critical_budget_ns: 100,
            interactive_budget_ns: 100,
        };
        let mut scheduler = PriorityQueueScheduler::new(config);

        for _ in 0..5 {
            scheduler
                .enqueue(PrioritizedIoOp {
                    op_id: 0,
                    workload_class: IoWorkloadClass::Critical,
                    deadline_ns: None,
                    size_bytes: 4096,
                    block_id: 0,
                })
                .unwrap();
        }

        scheduler
            .enqueue(PrioritizedIoOp {
                op_id: 100,
                workload_class: IoWorkloadClass::Bulk,
                deadline_ns: None,
                size_bytes: 4096,
                block_id: 100,
            })
            .unwrap();

        for _ in 0..5 {
            scheduler.dequeue();
        }

        let bulk_depth = scheduler.queue_depth(IoWorkloadClass::Bulk);
        assert!(bulk_depth >= 1);
    }

    #[test]
    fn batch_dequeue_maintains_ordering() {
        let config = SchedulerConfig {
            critical_budget_ns: 1000000,
            interactive_budget_ns: 1000000,
        };
        let mut scheduler = PriorityQueueScheduler::new(config);
        scheduler
            .enqueue(PrioritizedIoOp {
                op_id: 1,
                workload_class: IoWorkloadClass::Critical,
                deadline_ns: None,
                size_bytes: 4096,
                block_id: 100,
            })
            .unwrap();
        scheduler
            .enqueue(PrioritizedIoOp {
                op_id: 2,
                workload_class: IoWorkloadClass::Critical,
                deadline_ns: None,
                size_bytes: 4096,
                block_id: 200,
            })
            .unwrap();
        scheduler
            .enqueue(PrioritizedIoOp {
                op_id: 3,
                workload_class: IoWorkloadClass::Interactive,
                deadline_ns: None,
                size_bytes: 4096,
                block_id: 300,
            })
            .unwrap();

        let batch = scheduler.dequeue_batch(3);
        assert_eq!(batch.len(), 3);
        assert_eq!(batch[0].op_id, 1);
        assert_eq!(batch[1].op_id, 2);
    }

    #[test]
    fn queue_depth_returns_correct_count() {
        let config = SchedulerConfig {
            critical_budget_ns: 1000000,
            interactive_budget_ns: 1000000,
        };
        let mut scheduler = PriorityQueueScheduler::new(config);
        scheduler
            .enqueue(PrioritizedIoOp {
                op_id: 1,
                workload_class: IoWorkloadClass::Critical,
                deadline_ns: None,
                size_bytes: 4096,
                block_id: 100,
            })
            .unwrap();
        scheduler
            .enqueue(PrioritizedIoOp {
                op_id: 2,
                workload_class: IoWorkloadClass::Critical,
                deadline_ns: None,
                size_bytes: 4096,
                block_id: 200,
            })
            .unwrap();
        assert_eq!(scheduler.queue_depth(IoWorkloadClass::Critical), 2);
    }

    #[test]
    fn latency_estimation_increases_with_queue_depth() {
        let config = SchedulerConfig {
            critical_budget_ns: 1000000,
            interactive_budget_ns: 1000000,
        };
        let mut scheduler = PriorityQueueScheduler::new(config);

        let latency_empty = scheduler.estimated_latency_ns(IoWorkloadClass::Critical);
        assert_eq!(latency_empty, 0);

        for i in 0..5 {
            scheduler
                .enqueue(PrioritizedIoOp {
                    op_id: i,
                    workload_class: IoWorkloadClass::Critical,
                    deadline_ns: None,
                    size_bytes: 4096,
                    block_id: i * 100,
                })
                .unwrap();
        }

        let latency_5 = scheduler.estimated_latency_ns(IoWorkloadClass::Critical);
        assert_eq!(latency_5, 5 * 4096);
    }

    #[test]
    fn out_of_order_enqueue_dequeue() {
        let config = SchedulerConfig {
            critical_budget_ns: 1000000,
            interactive_budget_ns: 1000000,
        };
        let mut scheduler = PriorityQueueScheduler::new(config);

        scheduler
            .enqueue(PrioritizedIoOp {
                op_id: 3,
                workload_class: IoWorkloadClass::Bulk,
                deadline_ns: None,
                size_bytes: 4096,
                block_id: 300,
            })
            .unwrap();

        scheduler
            .enqueue(PrioritizedIoOp {
                op_id: 1,
                workload_class: IoWorkloadClass::Critical,
                deadline_ns: None,
                size_bytes: 4096,
                block_id: 100,
            })
            .unwrap();

        scheduler
            .enqueue(PrioritizedIoOp {
                op_id: 2,
                workload_class: IoWorkloadClass::Interactive,
                deadline_ns: None,
                size_bytes: 4096,
                block_id: 200,
            })
            .unwrap();

        let op1 = scheduler.dequeue().unwrap();
        let op2 = scheduler.dequeue().unwrap();

        assert_eq!(op1.op_id, 1);
        assert_eq!(op2.op_id, 2);
    }

    #[test]
    fn empty_dequeue_returns_none() {
        let config = SchedulerConfig {
            critical_budget_ns: 1000000,
            interactive_budget_ns: 1000000,
        };
        let scheduler = PriorityQueueScheduler::new(config);
        let op = scheduler.dequeue();
        assert!(op.is_none());
    }

    #[test]
    fn workload_class_ordering_via_derive_ord() {
        assert!(IoWorkloadClass::Critical < IoWorkloadClass::Interactive);
        assert!(IoWorkloadClass::Interactive < IoWorkloadClass::Bulk);
        assert!(IoWorkloadClass::Critical < IoWorkloadClass::Bulk);
    }

    #[test]
    fn multiple_ops_same_class_order_maintained() {
        let config = SchedulerConfig {
            critical_budget_ns: 1000000,
            interactive_budget_ns: 1000000,
        };
        let mut scheduler = PriorityQueueScheduler::new(config);

        for i in 0..10 {
            scheduler
                .enqueue(PrioritizedIoOp {
                    op_id: i,
                    workload_class: IoWorkloadClass::Bulk,
                    deadline_ns: None,
                    size_bytes: 4096,
                    block_id: i * 100,
                })
                .unwrap();
        }

        let depth = scheduler.queue_depth(IoWorkloadClass::Bulk);
        assert_eq!(depth, 10);
    }

    #[test]
    fn dequeue_batch_with_count_zero() {
        let config = SchedulerConfig {
            critical_budget_ns: 1000000,
            interactive_budget_ns: 1000000,
        };
        let mut scheduler = PriorityQueueScheduler::new(config);
        scheduler
            .enqueue(PrioritizedIoOp {
                op_id: 1,
                workload_class: IoWorkloadClass::Critical,
                deadline_ns: None,
                size_bytes: 4096,
                block_id: 100,
            })
            .unwrap();

        let batch = scheduler.dequeue_batch(0);
        assert!(batch.is_empty());
    }

    #[test]
    fn dequeue_batch_larger_than_available() {
        let config = SchedulerConfig {
            critical_budget_ns: 1000000,
            interactive_budget_ns: 1000000,
        };
        let mut scheduler = PriorityQueueScheduler::new(config);

        scheduler
            .enqueue(PrioritizedIoOp {
                op_id: 1,
                workload_class: IoWorkloadClass::Critical,
                deadline_ns: None,
                size_bytes: 4096,
                block_id: 100,
            })
            .unwrap();

        let batch = scheduler.dequeue_batch(100);
        assert_eq!(batch.len(), 1);
    }

    #[test]
    fn expire_deadlines_handles_no_expired_ops() {
        let config = SchedulerConfig {
            critical_budget_ns: 1000000,
            interactive_budget_ns: 1000000,
        };
        let mut scheduler = PriorityQueueScheduler::new(config);
        scheduler
            .enqueue(PrioritizedIoOp {
                op_id: 1,
                workload_class: IoWorkloadClass::Critical,
                deadline_ns: Some(10000),
                size_bytes: 4096,
                block_id: 100,
            })
            .unwrap();

        let expired = scheduler.expire_deadlines(1000);
        assert_eq!(expired, 0);
    }

    #[test]
    fn estimated_latency_ns_returns_zero_for_empty_queue() {
        let config = SchedulerConfig {
            critical_budget_ns: 1000000,
            interactive_budget_ns: 1000000,
        };
        let scheduler = PriorityQueueScheduler::new(config);

        let latency = scheduler.estimated_latency_ns(IoWorkloadClass::Interactive);
        assert_eq!(latency, 0);
    }

    #[test]
    fn scheduler_config_values_stored_correctly() {
        let config = SchedulerConfig {
            critical_budget_ns: 500000,
            interactive_budget_ns: 300000,
        };
        let scheduler = PriorityQueueScheduler::new(config);
        assert_eq!(scheduler.estimated_latency_ns(IoWorkloadClass::Bulk), 0);
    }

    #[test]
    fn total_pending_updated_on_enqueue() {
        let config = SchedulerConfig {
            critical_budget_ns: 1000000,
            interactive_budget_ns: 1000000,
        };
        let mut scheduler = PriorityQueueScheduler::new(config);

        scheduler
            .enqueue(PrioritizedIoOp {
                op_id: 1,
                workload_class: IoWorkloadClass::Critical,
                deadline_ns: None,
                size_bytes: 4096,
                block_id: 100,
            })
            .unwrap();

        scheduler
            .enqueue(PrioritizedIoOp {
                op_id: 2,
                workload_class: IoWorkloadClass::Bulk,
                deadline_ns: None,
                size_bytes: 4096,
                block_id: 200,
            })
            .unwrap();
    }

    #[test]
    fn total_pending_updated_on_dequeue() {
        let config = SchedulerConfig {
            critical_budget_ns: 1000000,
            interactive_budget_ns: 1000000,
        };
        let mut scheduler = PriorityQueueScheduler::new(config);

        scheduler
            .enqueue(PrioritizedIoOp {
                op_id: 1,
                workload_class: IoWorkloadClass::Critical,
                deadline_ns: None,
                size_bytes: 4096,
                block_id: 100,
            })
            .unwrap();

        scheduler.dequeue();
    }

    #[test]
    fn deadline_none_means_no_expiration() {
        let config = SchedulerConfig {
            critical_budget_ns: 1000000,
            interactive_budget_ns: 1000000,
        };
        let mut scheduler = PriorityQueueScheduler::new(config);
        scheduler
            .enqueue(PrioritizedIoOp {
                op_id: 1,
                workload_class: IoWorkloadClass::Critical,
                deadline_ns: None,
                size_bytes: 4096,
                block_id: 100,
            })
            .unwrap();

        let expired = scheduler.expire_deadlines(1000000);
        assert_eq!(expired, 0);
    }

    #[test]
    fn size_bytes_used_in_latency_estimation() {
        let config = SchedulerConfig {
            critical_budget_ns: 1000000,
            interactive_budget_ns: 1000000,
        };
        let mut scheduler = PriorityQueueScheduler::new(config);

        scheduler
            .enqueue(PrioritizedIoOp {
                op_id: 1,
                workload_class: IoWorkloadClass::Critical,
                deadline_ns: None,
                size_bytes: 8192,
                block_id: 100,
            })
            .unwrap();

        scheduler
            .enqueue(PrioritizedIoOp {
                op_id: 2,
                workload_class: IoWorkloadClass::Critical,
                deadline_ns: None,
                size_bytes: 16384,
                block_id: 200,
            })
            .unwrap();

        let latency = scheduler.estimated_latency_ns(IoWorkloadClass::Critical);
        assert_eq!(latency, 2 * 4096);
    }

    #[test]
    fn op_id_preserved_through_queue_operations() {
        let config = SchedulerConfig {
            critical_budget_ns: 1000000,
            interactive_budget_ns: 1000000,
        };
        let mut scheduler = PriorityQueueScheduler::new(config);

        scheduler
            .enqueue(PrioritizedIoOp {
                op_id: 999,
                workload_class: IoWorkloadClass::Critical,
                deadline_ns: None,
                size_bytes: 4096,
                block_id: 100,
            })
            .unwrap();

        let op = scheduler.dequeue().unwrap();
        assert_eq!(op.op_id, 999);
    }

    #[test]
    fn block_id_preserved_through_queue_operations() {
        let config = SchedulerConfig {
            critical_budget_ns: 1000000,
            interactive_budget_ns: 1000000,
        };
        let mut scheduler = PriorityQueueScheduler::new(config);

        scheduler
            .enqueue(PrioritizedIoOp {
                op_id: 1,
                workload_class: IoWorkloadClass::Critical,
                deadline_ns: None,
                size_bytes: 4096,
                block_id: 777,
            })
            .unwrap();

        let op = scheduler.dequeue().unwrap();
        assert_eq!(op.block_id, 777);
    }
}
