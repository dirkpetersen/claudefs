//! I/O coalescing module - merges adjacent I/O requests to reduce device submission overhead.

use crate::error::StorageError;

type Result<T> = std::result::Result<T, StorageError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IoOpType {
    Read,
    Write,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IoRequest {
    pub op_type: IoOpType,
    pub block_id: u64,
    pub block_count: u32,
    pub priority: u8,
    pub client_id: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CoalescedRequest {
    pub op_type: IoOpType,
    pub start_block: u64,
    pub block_count: u32,
    pub constituent_count: usize,
    pub priority: u8,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CoalescingConfig {
    pub max_coalesce_blocks: u32,
    pub max_pending_count: usize,
}

pub struct IoCoalescer {
    pending_reads: Vec<IoRequest>,
    pending_writes: Vec<IoRequest>,
    config: CoalescingConfig,
}

impl IoCoalescer {
    pub fn new(config: CoalescingConfig) -> Self {
        Self {
            pending_reads: Vec::new(),
            pending_writes: Vec::new(),
            config,
        }
    }

    pub fn add_request(&mut self, req: IoRequest) -> Result<()> {
        let target = match req.op_type {
            IoOpType::Read => &mut self.pending_reads,
            IoOpType::Write => &mut self.pending_writes,
        };

        if target.len() >= self.config.max_pending_count {
            return Err(StorageError::AllocatorError(format!(
                "Pending queue full: {} >= {}",
                target.len() + 1,
                self.config.max_pending_count
            )));
        }

        target.push(req);
        target.sort_by_key(|r| r.block_id);

        if target.len() > self.config.max_pending_count {
            return Err(StorageError::AllocatorError(
                "Force flush triggered".to_string(),
            ));
        }

        Ok(())
    }

    pub fn coalesce(&mut self) -> Vec<CoalescedRequest> {
        let mut result = Vec::new();

        result.extend(self.process_queue(&self.pending_reads, IoOpType::Read));
        result.extend(self.process_queue(&self.pending_writes, IoOpType::Write));

        self.pending_reads.clear();
        self.pending_writes.clear();

        result
    }

    fn process_queue(&self, queue: &[IoRequest], op_type: IoOpType) -> Vec<CoalescedRequest> {
        if queue.is_empty() {
            return Vec::new();
        }

        let mut result = Vec::new();
        let mut current: Option<CoalescedRequest> = None;

        for req in queue.iter() {
            if let Some(ref mut curr) = current {
                let prev_end = curr.start_block + curr.block_count as u64;
                let would_exceed = (curr.block_count as u64 + req.block_count as u64)
                    > self.config.max_coalesce_blocks as u64;
                let is_adjacent = prev_end == req.block_id;

                if !is_adjacent || would_exceed {
                    result.push(current.take().unwrap());
                    current = Some(CoalescedRequest {
                        op_type,
                        start_block: req.block_id,
                        block_count: req.block_count,
                        constituent_count: 1,
                        priority: req.priority,
                    });
                } else {
                    curr.block_count += req.block_count;
                    curr.constituent_count += 1;
                    if req.priority > curr.priority {
                        curr.priority = req.priority;
                    }
                }
            } else {
                current = Some(CoalescedRequest {
                    op_type,
                    start_block: req.block_id,
                    block_count: req.block_count,
                    constituent_count: 1,
                    priority: req.priority,
                });
            }
        }

        if let Some(c) = current {
            result.push(c);
        }

        result
    }

    pub fn pending_count(&self, op_type: IoOpType) -> usize {
        match op_type {
            IoOpType::Read => self.pending_reads.len(),
            IoOpType::Write => self.pending_writes.len(),
        }
    }

    pub fn clear(&mut self) -> Result<()> {
        self.pending_reads.clear();
        self.pending_writes.clear();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_coalesce_returns_empty() {
        let config = CoalescingConfig {
            max_coalesce_blocks: 100,
            max_pending_count: 10,
        };
        let mut coalescer = IoCoalescer::new(config);
        let result = coalescer.coalesce();
        assert!(result.is_empty());
    }

    #[test]
    fn single_request_coalescing() {
        let config = CoalescingConfig {
            max_coalesce_blocks: 100,
            max_pending_count: 10,
        };
        let mut coalescer = IoCoalescer::new(config);
        coalescer
            .add_request(IoRequest {
                op_type: IoOpType::Read,
                block_id: 10,
                block_count: 5,
                priority: 1,
                client_id: 1,
            })
            .unwrap();
        let result = coalescer.coalesce();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].start_block, 10);
        assert_eq!(result[0].block_count, 5);
        assert_eq!(result[0].constituent_count, 1);
    }

    #[test]
    fn adjacent_read_coalescing() {
        let config = CoalescingConfig {
            max_coalesce_blocks: 100,
            max_pending_count: 10,
        };
        let mut coalescer = IoCoalescer::new(config);
        coalescer
            .add_request(IoRequest {
                op_type: IoOpType::Read,
                block_id: 10,
                block_count: 5,
                priority: 1,
                client_id: 1,
            })
            .unwrap();
        coalescer
            .add_request(IoRequest {
                op_type: IoOpType::Read,
                block_id: 15,
                block_count: 3,
                priority: 1,
                client_id: 1,
            })
            .unwrap();
        let result = coalescer.coalesce();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].block_count, 8);
        assert_eq!(result[0].constituent_count, 2);
    }

    #[test]
    fn adjacent_write_coalescing() {
        let config = CoalescingConfig {
            max_coalesce_blocks: 100,
            max_pending_count: 10,
        };
        let mut coalescer = IoCoalescer::new(config);
        coalescer
            .add_request(IoRequest {
                op_type: IoOpType::Write,
                block_id: 20,
                block_count: 4,
                priority: 2,
                client_id: 2,
            })
            .unwrap();
        coalescer
            .add_request(IoRequest {
                op_type: IoOpType::Write,
                block_id: 24,
                block_count: 6,
                priority: 2,
                client_id: 2,
            })
            .unwrap();
        let result = coalescer.coalesce();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].block_count, 10);
        assert_eq!(result[0].op_type, IoOpType::Write);
    }

    #[test]
    fn non_adjacent_not_coalesced() {
        let config = CoalescingConfig {
            max_coalesce_blocks: 100,
            max_pending_count: 10,
        };
        let mut coalescer = IoCoalescer::new(config);
        coalescer
            .add_request(IoRequest {
                op_type: IoOpType::Read,
                block_id: 10,
                block_count: 5,
                priority: 1,
                client_id: 1,
            })
            .unwrap();
        coalescer
            .add_request(IoRequest {
                op_type: IoOpType::Read,
                block_id: 100,
                block_count: 3,
                priority: 1,
                client_id: 1,
            })
            .unwrap();
        let result = coalescer.coalesce();
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn split_at_max_coalesce_blocks() {
        let config = CoalescingConfig {
            max_coalesce_blocks: 10,
            max_pending_count: 10,
        };
        let mut coalescer = IoCoalescer::new(config);
        coalescer
            .add_request(IoRequest {
                op_type: IoOpType::Read,
                block_id: 0,
                block_count: 8,
                priority: 1,
                client_id: 1,
            })
            .unwrap();
        coalescer
            .add_request(IoRequest {
                op_type: IoOpType::Read,
                block_id: 8,
                block_count: 5,
                priority: 1,
                client_id: 1,
            })
            .unwrap();
        let result = coalescer.coalesce();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].block_count, 8);
        assert_eq!(result[1].block_count, 5);
    }

    #[test]
    fn max_priority_preserved() {
        let config = CoalescingConfig {
            max_coalesce_blocks: 100,
            max_pending_count: 10,
        };
        let mut coalescer = IoCoalescer::new(config);
        coalescer
            .add_request(IoRequest {
                op_type: IoOpType::Read,
                block_id: 10,
                block_count: 5,
                priority: 3,
                client_id: 1,
            })
            .unwrap();
        coalescer
            .add_request(IoRequest {
                op_type: IoOpType::Read,
                block_id: 15,
                block_count: 3,
                priority: 1,
                client_id: 1,
            })
            .unwrap();
        let result = coalescer.coalesce();
        assert_eq!(result[0].priority, 3);
    }

    #[test]
    fn pending_count_reads() {
        let config = CoalescingConfig {
            max_coalesce_blocks: 100,
            max_pending_count: 10,
        };
        let mut coalescer = IoCoalescer::new(config);
        coalescer
            .add_request(IoRequest {
                op_type: IoOpType::Read,
                block_id: 10,
                block_count: 5,
                priority: 1,
                client_id: 1,
            })
            .unwrap();
        coalescer
            .add_request(IoRequest {
                op_type: IoOpType::Read,
                block_id: 20,
                block_count: 3,
                priority: 1,
                client_id: 1,
            })
            .unwrap();
        assert_eq!(coalescer.pending_count(IoOpType::Read), 2);
        assert_eq!(coalescer.pending_count(IoOpType::Write), 0);
    }

    #[test]
    fn pending_count_writes() {
        let config = CoalescingConfig {
            max_coalesce_blocks: 100,
            max_pending_count: 10,
        };
        let mut coalescer = IoCoalescer::new(config);
        coalescer
            .add_request(IoRequest {
                op_type: IoOpType::Write,
                block_id: 10,
                block_count: 5,
                priority: 1,
                client_id: 1,
            })
            .unwrap();
        assert_eq!(coalescer.pending_count(IoOpType::Write), 1);
        assert_eq!(coalescer.pending_count(IoOpType::Read), 0);
    }

    #[test]
    fn clear_removes_all() {
        let config = CoalescingConfig {
            max_coalesce_blocks: 100,
            max_pending_count: 10,
        };
        let mut coalescer = IoCoalescer::new(config);
        coalescer
            .add_request(IoRequest {
                op_type: IoOpType::Read,
                block_id: 10,
                block_count: 5,
                priority: 1,
                client_id: 1,
            })
            .unwrap();
        coalescer
            .add_request(IoRequest {
                op_type: IoOpType::Write,
                block_id: 20,
                block_count: 3,
                priority: 1,
                client_id: 1,
            })
            .unwrap();
        coalescer.clear().unwrap();
        assert_eq!(coalescer.pending_count(IoOpType::Read), 0);
        assert_eq!(coalescer.pending_count(IoOpType::Write), 0);
    }

    #[test]
    fn add_request_error_handling() {
        let config = CoalescingConfig {
            max_coalesce_blocks: 100,
            max_pending_count: 2,
        };
        let mut coalescer = IoCoalescer::new(config);
        coalescer
            .add_request(IoRequest {
                op_type: IoOpType::Read,
                block_id: 10,
                block_count: 5,
                priority: 1,
                client_id: 1,
            })
            .unwrap();
        coalescer
            .add_request(IoRequest {
                op_type: IoOpType::Read,
                block_id: 20,
                block_count: 3,
                priority: 1,
                client_id: 1,
            })
            .unwrap();
        let result = coalescer.add_request(IoRequest {
            op_type: IoOpType::Read,
            block_id: 30,
            block_count: 2,
            priority: 1,
            client_id: 1,
        });
        assert!(result.is_err());
    }

    #[test]
    fn mixed_read_write_separation() {
        let config = CoalescingConfig {
            max_coalesce_blocks: 100,
            max_pending_count: 10,
        };
        let mut coalescer = IoCoalescer::new(config);
        coalescer
            .add_request(IoRequest {
                op_type: IoOpType::Read,
                block_id: 10,
                block_count: 5,
                priority: 1,
                client_id: 1,
            })
            .unwrap();
        coalescer
            .add_request(IoRequest {
                op_type: IoOpType::Write,
                block_id: 15,
                block_count: 3,
                priority: 1,
                client_id: 1,
            })
            .unwrap();
        let result = coalescer.coalesce();
        assert_eq!(result.len(), 2);
        let reads: Vec<_> = result
            .iter()
            .filter(|r| r.op_type == IoOpType::Read)
            .collect();
        let writes: Vec<_> = result
            .iter()
            .filter(|r| r.op_type == IoOpType::Write)
            .collect();
        assert_eq!(reads.len(), 1);
        assert_eq!(writes.len(), 1);
    }

    #[test]
    fn multiple_coalesced_groups() {
        let config = CoalescingConfig {
            max_coalesce_blocks: 100,
            max_pending_count: 10,
        };
        let mut coalescer = IoCoalescer::new(config);
        for i in 0..5 {
            coalescer
                .add_request(IoRequest {
                    op_type: IoOpType::Read,
                    block_id: i * 3,
                    block_count: 3,
                    priority: 1,
                    client_id: 1,
                })
                .unwrap();
        }
        let result = coalescer.coalesce();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].constituent_count, 5);
        assert_eq!(result[0].block_count, 15);
    }

    #[test]
    fn priority_ordering() {
        let config = CoalescingConfig {
            max_coalesce_blocks: 100,
            max_pending_count: 10,
        };
        let mut coalescer = IoCoalescer::new(config);
        coalescer
            .add_request(IoRequest {
                op_type: IoOpType::Read,
                block_id: 10,
                block_count: 5,
                priority: 1,
                client_id: 1,
            })
            .unwrap();
        coalescer
            .add_request(IoRequest {
                op_type: IoOpType::Read,
                block_id: 15,
                block_count: 3,
                priority: 5,
                client_id: 2,
            })
            .unwrap();
        coalescer
            .add_request(IoRequest {
                op_type: IoOpType::Read,
                block_id: 18,
                block_count: 2,
                priority: 3,
                client_id: 3,
            })
            .unwrap();
        let result = coalescer.coalesce();
        assert_eq!(result[0].priority, 5);
    }

    #[test]
    fn force_flush_at_max_pending_count() {
        let config = CoalescingConfig {
            max_coalesce_blocks: 100,
            max_pending_count: 3,
        };
        let mut coalescer = IoCoalescer::new(config);
        for i in 0..3 {
            coalescer
                .add_request(IoRequest {
                    op_type: IoOpType::Read,
                    block_id: i,
                    block_count: 1,
                    priority: 1,
                    client_id: i as u32,
                })
                .unwrap();
        }
        let result = coalescer.add_request(IoRequest {
            op_type: IoOpType::Read,
            block_id: 100,
            block_count: 1,
            priority: 1,
            client_id: 100,
        });
        assert!(result.is_err());
    }

    #[test]
    fn block_boundary_handling() {
        let config = CoalescingConfig {
            max_coalesce_blocks: 100,
            max_pending_count: 10,
        };
        let mut coalescer = IoCoalescer::new(config);
        coalescer
            .add_request(IoRequest {
                op_type: IoOpType::Read,
                block_id: 0,
                block_count: 10,
                priority: 1,
                client_id: 1,
            })
            .unwrap();
        coalescer
            .add_request(IoRequest {
                op_type: IoOpType::Read,
                block_id: 10,
                block_count: 10,
                priority: 1,
                client_id: 1,
            })
            .unwrap();
        let result = coalescer.coalesce();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].block_count, 20);
    }

    #[test]
    fn empty_pending_lists() {
        let config = CoalescingConfig {
            max_coalesce_blocks: 100,
            max_pending_count: 10,
        };
        let coalescer = IoCoalescer::new(config);
        assert_eq!(coalescer.pending_count(IoOpType::Read), 0);
        assert_eq!(coalescer.pending_count(IoOpType::Write), 0);
    }

    #[test]
    fn zero_block_count() {
        let config = CoalescingConfig {
            max_coalesce_blocks: 100,
            max_pending_count: 10,
        };
        let mut coalescer = IoCoalescer::new(config);
        coalescer
            .add_request(IoRequest {
                op_type: IoOpType::Read,
                block_id: 10,
                block_count: 0,
                priority: 1,
                client_id: 1,
            })
            .unwrap();
        let result = coalescer.coalesce();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].block_count, 0);
    }

    #[test]
    fn max_priority_edge_case() {
        let config = CoalescingConfig {
            max_coalesce_blocks: 100,
            max_pending_count: 10,
        };
        let mut coalescer = IoCoalescer::new(config);
        coalescer
            .add_request(IoRequest {
                op_type: IoOpType::Read,
                block_id: 10,
                block_count: 5,
                priority: 255,
                client_id: 1,
            })
            .unwrap();
        coalescer
            .add_request(IoRequest {
                op_type: IoOpType::Read,
                block_id: 15,
                block_count: 3,
                priority: 0,
                client_id: 2,
            })
            .unwrap();
        let result = coalescer.coalesce();
        assert_eq!(result[0].priority, 255);
    }

    #[test]
    fn client_id_preserved() {
        let config = CoalescingConfig {
            max_coalesce_blocks: 100,
            max_pending_count: 10,
        };
        let mut coalescer = IoCoalescer::new(config);
        coalescer
            .add_request(IoRequest {
                op_type: IoOpType::Read,
                block_id: 10,
                block_count: 5,
                priority: 1,
                client_id: 999,
            })
            .unwrap();
        coalescer
            .add_request(IoRequest {
                op_type: IoOpType::Read,
                block_id: 15,
                block_count: 3,
                priority: 1,
                client_id: 888,
            })
            .unwrap();
        let result = coalescer.coalesce();
        assert_eq!(result[0].constituent_count, 2);
    }

    #[test]
    fn constituent_count_tracking() {
        let config = CoalescingConfig {
            max_coalesce_blocks: 100,
            max_pending_count: 10,
        };
        let mut coalescer = IoCoalescer::new(config);
        for i in 0..7 {
            coalescer
                .add_request(IoRequest {
                    op_type: IoOpType::Read,
                    block_id: i * 2,
                    block_count: 2,
                    priority: 1,
                    client_id: 1,
                })
                .unwrap();
        }
        let result = coalescer.coalesce();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].constituent_count, 7);
    }

    #[test]
    fn start_block_calc() {
        let config = CoalescingConfig {
            max_coalesce_blocks: 100,
            max_pending_count: 10,
        };
        let mut coalescer = IoCoalescer::new(config);
        coalescer
            .add_request(IoRequest {
                op_type: IoOpType::Read,
                block_id: 50,
                block_count: 3,
                priority: 1,
                client_id: 1,
            })
            .unwrap();
        coalescer
            .add_request(IoRequest {
                op_type: IoOpType::Read,
                block_id: 53,
                block_count: 4,
                priority: 1,
                client_id: 1,
            })
            .unwrap();
        let result = coalescer.coalesce();
        assert_eq!(result[0].start_block, 50);
    }

    #[test]
    fn read_write_separation() {
        let config = CoalescingConfig {
            max_coalesce_blocks: 100,
            max_pending_count: 10,
        };
        let mut coalescer = IoCoalescer::new(config);
        for i in 0..3 {
            coalescer
                .add_request(IoRequest {
                    op_type: IoOpType::Read,
                    block_id: i * 3,
                    block_count: 3,
                    priority: 1,
                    client_id: 1,
                })
                .unwrap();
            coalescer
                .add_request(IoRequest {
                    op_type: IoOpType::Write,
                    block_id: i * 3 + 100,
                    block_count: 3,
                    priority: 1,
                    client_id: 1,
                })
                .unwrap();
        }
        let result = coalescer.coalesce();
        let reads = result
            .iter()
            .filter(|r| r.op_type == IoOpType::Read)
            .count();
        let writes = result
            .iter()
            .filter(|r| r.op_type == IoOpType::Write)
            .count();
        assert_eq!(reads, 1);
        assert_eq!(writes, 1);
    }

    #[test]
    fn sorting_correctness() {
        let config = CoalescingConfig {
            max_coalesce_blocks: 100,
            max_pending_count: 10,
        };
        let mut coalescer = IoCoalescer::new(config);
        coalescer
            .add_request(IoRequest {
                op_type: IoOpType::Read,
                block_id: 100,
                block_count: 5,
                priority: 1,
                client_id: 1,
            })
            .unwrap();
        coalescer
            .add_request(IoRequest {
                op_type: IoOpType::Read,
                block_id: 10,
                block_count: 3,
                priority: 1,
                client_id: 1,
            })
            .unwrap();
        coalescer
            .add_request(IoRequest {
                op_type: IoOpType::Read,
                block_id: 50,
                block_count: 4,
                priority: 1,
                client_id: 1,
            })
            .unwrap();
        let result = coalescer.coalesce();
        assert_eq!(result[0].start_block, 10);
        assert_eq!(result[1].start_block, 50);
        assert_eq!(result[2].start_block, 100);
    }

    #[test]
    fn large_num_requests() {
        let config = CoalescingConfig {
            max_coalesce_blocks: 1000,
            max_pending_count: 100,
        };
        let mut coalescer = IoCoalescer::new(config);
        for i in 0..50 {
            coalescer
                .add_request(IoRequest {
                    op_type: IoOpType::Read,
                    block_id: i * 2,
                    block_count: 1,
                    priority: 1,
                    client_id: 1,
                })
                .unwrap();
        }
        let result = coalescer.coalesce();
        assert_eq!(result.len(), 50);
    }

    #[test]
    fn adjacent_different_priorities() {
        let config = CoalescingConfig {
            max_coalesce_blocks: 100,
            max_pending_count: 10,
        };
        let mut coalescer = IoCoalescer::new(config);
        coalescer
            .add_request(IoRequest {
                op_type: IoOpType::Read,
                block_id: 10,
                block_count: 5,
                priority: 10,
                client_id: 1,
            })
            .unwrap();
        coalescer
            .add_request(IoRequest {
                op_type: IoOpType::Read,
                block_id: 15,
                block_count: 3,
                priority: 20,
                client_id: 2,
            })
            .unwrap();
        coalescer
            .add_request(IoRequest {
                op_type: IoOpType::Read,
                block_id: 18,
                block_count: 2,
                priority: 5,
                client_id: 3,
            })
            .unwrap();
        let result = coalescer.coalesce();
        assert_eq!(result[0].priority, 20);
    }

    #[test]
    fn force_flush_behavior() {
        let config = CoalescingConfig {
            max_coalesce_blocks: 100,
            max_pending_count: 5,
        };
        let mut coalescer = IoCoalescer::new(config);
        for i in 0..5 {
            let _ = coalescer.add_request(IoRequest {
                op_type: IoOpType::Read,
                block_id: i * 10,
                block_count: 1,
                priority: 1,
                client_id: i as u32,
            });
        }
        assert_eq!(coalescer.pending_count(IoOpType::Read), 5);
    }

    #[test]
    fn clear_resets_state() {
        let config = CoalescingConfig {
            max_coalesce_blocks: 100,
            max_pending_count: 10,
        };
        let mut coalescer = IoCoalescer::new(config);
        coalescer
            .add_request(IoRequest {
                op_type: IoOpType::Read,
                block_id: 10,
                block_count: 5,
                priority: 1,
                client_id: 1,
            })
            .unwrap();
        coalescer
            .add_request(IoRequest {
                op_type: IoOpType::Write,
                block_id: 20,
                block_count: 3,
                priority: 1,
                client_id: 1,
            })
            .unwrap();
        coalescer.clear().unwrap();
        let result = coalescer.coalesce();
        assert!(result.is_empty());
    }
}
