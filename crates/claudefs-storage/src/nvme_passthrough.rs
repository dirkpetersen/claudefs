//! NVMe passthrough queue alignment for production workloads.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;
use tracing::{debug, error, info, warn};

#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct QueuePairId(pub u32);

#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct CoreId(pub u32);

#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct NsId(pub u32);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum QueueState {
    Active,
    Draining,
    Idle,
    Failed { reason: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NvmeOpType {
    Read,
    Write,
    Flush,
    WriteZeroes,
    DatasetManagement,
    AtomicWrite,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmissionEntry {
    pub command_id: u64,
    pub core_id: CoreId,
    pub op_type: NvmeOpType,
    pub namespace: NsId,
    pub lba_start: u64,
    pub lba_count: u32,
    pub data_len: usize,
    pub submitted_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionEntry {
    pub command_id: u64,
    pub status: CompletionStatus,
    pub completed_at: u64,
    pub latency_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CompletionStatus {
    Success,
    NamespaceNotReady,
    CommandAborted,
    MediaError,
    InternalError { code: u16 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueuePair {
    pub id: QueuePairId,
    pub core_id: CoreId,
    pub namespace: NsId,
    pub sq_depth: u32,
    pub cq_depth: u32,
    pub state: QueueState,
    pub pending_submissions: u32,
    pub completed_count: u64,
    pub error_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PassthroughConfig {
    pub sq_depth: u32,
    pub cq_depth: u32,
    pub max_queue_pairs: u32,
    pub atomic_writes: bool,
    pub max_atomic_write_bytes: u32,
    pub min_kernel_major: u32,
    pub min_kernel_minor: u32,
}

impl Default for PassthroughConfig {
    fn default() -> Self {
        Self {
            sq_depth: 1024,
            cq_depth: 1024,
            max_queue_pairs: 64,
            atomic_writes: true,
            max_atomic_write_bytes: 65536,
            min_kernel_major: 6,
            min_kernel_minor: 20,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PassthroughStats {
    pub total_submissions: u64,
    pub total_completions: u64,
    pub total_errors: u64,
    pub reads: u64,
    pub writes: u64,
    pub flushes: u64,
    pub atomic_writes: u64,
    pub avg_latency_ns: u64,
    pub max_latency_ns: u64,
    pub queue_pairs_active: u32,
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum PassthroughError {
    #[error("Queue pair {0:?} not found")]
    QueueNotFound(QueuePairId),

    #[error("Core {0:?} already has a queue pair")]
    CoreAlreadyBound(CoreId),

    #[error("No queue pair for core {0:?}")]
    NoQueueForCore(CoreId),

    #[error("Queue {0:?} is full (depth: {1})")]
    QueueFull(QueuePairId, u32),

    #[error("Command {0} not found")]
    CommandNotFound(u64),

    #[error("Queue {0:?} not in active state")]
    QueueNotActive(QueuePairId),

    #[error("Max queue pairs ({0}) reached")]
    MaxQueuePairsReached(u32),

    #[error("Atomic writes not enabled")]
    AtomicWritesDisabled,
}

pub struct PassthroughManager {
    config: PassthroughConfig,
    queue_pairs: HashMap<QueuePairId, QueuePair>,
    core_to_queue: HashMap<CoreId, QueuePairId>,
    submissions: HashMap<u64, SubmissionEntry>,
    completions: Vec<CompletionEntry>,
    next_command_id: u64,
    stats: PassthroughStats,
}

impl PassthroughManager {
    pub fn new(config: PassthroughConfig) -> Self {
        debug!("Creating PassthroughManager with config: {:?}", config);
        Self {
            config,
            queue_pairs: HashMap::new(),
            core_to_queue: HashMap::new(),
            submissions: HashMap::new(),
            completions: Vec::new(),
            next_command_id: 0,
            stats: PassthroughStats::default(),
        }
    }

    pub fn create_queue_pair(
        &mut self,
        core_id: CoreId,
        namespace: NsId,
    ) -> Result<QueuePairId, PassthroughError> {
        if self.core_to_queue.contains_key(&core_id) {
            warn!("Core {:?} already has a queue pair", core_id);
            return Err(PassthroughError::CoreAlreadyBound(core_id));
        }

        let current_count = self.queue_pairs.len() as u32;
        if current_count >= self.config.max_queue_pairs {
            warn!(
                "Max queue pairs reached: {} >= {}",
                current_count, self.config.max_queue_pairs
            );
            return Err(PassthroughError::MaxQueuePairsReached(
                self.config.max_queue_pairs,
            ));
        }

        let qp_id = QueuePairId(current_count);
        let queue_pair = QueuePair {
            id: qp_id,
            core_id,
            namespace,
            sq_depth: self.config.sq_depth,
            cq_depth: self.config.cq_depth,
            state: QueueState::Active,
            pending_submissions: 0,
            completed_count: 0,
            error_count: 0,
        };

        debug!(
            "Creating queue pair {:?} for core {:?}, namespace {:?}",
            qp_id, core_id, namespace
        );
        self.queue_pairs.insert(qp_id, queue_pair);
        self.core_to_queue.insert(core_id, qp_id);

        Ok(qp_id)
    }

    pub fn remove_queue_pair(&mut self, qp_id: QueuePairId) -> Result<(), PassthroughError> {
        let queue = self
            .queue_pairs
            .remove(&qp_id)
            .ok_or(PassthroughError::QueueNotFound(qp_id))?;
        let core_id = queue.core_id;
        self.core_to_queue.remove(&core_id);

        let cmd_ids: Vec<u64> = self
            .submissions
            .iter()
            .filter(|(_, s)| s.core_id == core_id)
            .map(|(id, _)| *id)
            .collect();

        for cmd_id in cmd_ids {
            self.submissions.remove(&cmd_id);
        }

        info!("Removed queue pair {:?}", qp_id);
        Ok(())
    }

    pub fn get_queue_pair(&self, qp_id: QueuePairId) -> Option<&QueuePair> {
        self.queue_pairs.get(&qp_id)
    }

    pub fn get_queue_for_core(&self, core_id: CoreId) -> Option<QueuePairId> {
        self.core_to_queue.get(&core_id).copied()
    }

    pub fn submit(
        &mut self,
        core_id: CoreId,
        op_type: NvmeOpType,
        namespace: NsId,
        lba_start: u64,
        lba_count: u32,
        data_len: usize,
        timestamp: u64,
    ) -> Result<u64, PassthroughError> {
        let qp_id = self
            .core_to_queue
            .get(&core_id)
            .ok_or(PassthroughError::NoQueueForCore(core_id))?;

        let queue = self.queue_pairs.get_mut(qp_id).unwrap();

        if queue.state != QueueState::Active {
            error!("Queue {:?} not in active state: {:?}", qp_id, queue.state);
            return Err(PassthroughError::QueueNotActive(*qp_id));
        }

        if queue.pending_submissions >= queue.sq_depth {
            warn!(
                "Queue {:?} is full: {} >= {}",
                qp_id, queue.pending_submissions, queue.sq_depth
            );
            return Err(PassthroughError::QueueFull(*qp_id, queue.sq_depth));
        }

        if op_type == NvmeOpType::AtomicWrite && !self.config.atomic_writes {
            warn!("Atomic writes attempted but disabled");
            return Err(PassthroughError::AtomicWritesDisabled);
        }

        let command_id = self.next_command_id;
        self.next_command_id += 1;

        let entry = SubmissionEntry {
            command_id,
            core_id,
            op_type,
            namespace,
            lba_start,
            lba_count,
            data_len,
            submitted_at: timestamp,
        };

        self.submissions.insert(command_id, entry);
        queue.pending_submissions += 1;
        self.stats.total_submissions += 1;

        match op_type {
            NvmeOpType::Read => self.stats.reads += 1,
            NvmeOpType::Write => self.stats.writes += 1,
            NvmeOpType::Flush => self.stats.flushes += 1,
            NvmeOpType::AtomicWrite => self.stats.atomic_writes += 1,
            _ => {}
        }

        debug!(
            "Submitted command {} to queue {:?}, op: {:?}",
            command_id, qp_id, op_type
        );
        Ok(command_id)
    }

    pub fn complete(
        &mut self,
        command_id: u64,
        status: CompletionStatus,
        timestamp: u64,
    ) -> Result<CompletionEntry, PassthroughError> {
        let submission = self
            .submissions
            .remove(&command_id)
            .ok_or(PassthroughError::CommandNotFound(command_id))?;

        let latency_ns = if timestamp > submission.submitted_at {
            timestamp - submission.submitted_at
        } else {
            0
        };

        for queue in self.queue_pairs.values_mut() {
            if queue.core_id == submission.core_id {
                queue.pending_submissions = queue.pending_submissions.saturating_sub(1);
                queue.completed_count += 1;

                if !matches!(status, CompletionStatus::Success) {
                    queue.error_count += 1;
                    self.stats.total_errors += 1;
                }
                break;
            }
        }

        let entry = CompletionEntry {
            command_id,
            status,
            completed_at: timestamp,
            latency_ns,
        };

        self.completions.push(entry.clone());
        self.stats.total_completions += 1;

        if latency_ns > self.stats.max_latency_ns {
            self.stats.max_latency_ns = latency_ns;
        }

        let total = self.stats.total_completions;
        if total > 0 {
            let sum = self.stats.avg_latency_ns * (total - 1) + latency_ns;
            self.stats.avg_latency_ns = sum / total;
        }

        debug!(
            "Completed command {} with latency {}ns",
            command_id, latency_ns
        );
        Ok(entry)
    }

    pub fn drain_queue(&mut self, qp_id: QueuePairId) -> Result<(), PassthroughError> {
        let queue = self
            .queue_pairs
            .get_mut(&qp_id)
            .ok_or(PassthroughError::QueueNotFound(qp_id))?;
        queue.state = QueueState::Draining;
        info!("Queue {:?} draining", qp_id);
        Ok(())
    }

    pub fn reset_queue(&mut self, qp_id: QueuePairId) -> Result<(), PassthroughError> {
        let queue = self
            .queue_pairs
            .get_mut(&qp_id)
            .ok_or(PassthroughError::QueueNotFound(qp_id))?;
        queue.state = QueueState::Active;
        info!("Queue {:?} reset to active", qp_id);
        Ok(())
    }

    pub fn pending_count(&self, qp_id: QueuePairId) -> u32 {
        self.queue_pairs
            .get(&qp_id)
            .map(|q| q.pending_submissions)
            .unwrap_or(0)
    }

    pub fn is_queue_full(&self, qp_id: QueuePairId) -> bool {
        self.queue_pairs
            .get(&qp_id)
            .map(|q| q.pending_submissions >= q.sq_depth)
            .unwrap_or(false)
    }

    pub fn supports_atomic_writes(&self) -> bool {
        self.config.atomic_writes
    }

    pub fn queue_pair_count(&self) -> usize {
        self.queue_pairs.len()
    }

    pub fn active_queue_pairs(&self) -> Vec<QueuePairId> {
        self.queue_pairs
            .iter()
            .filter(|(_, q)| matches!(q.state, QueueState::Active))
            .map(|(id, _)| *id)
            .collect()
    }

    pub fn stats(&self) -> &PassthroughStats {
        &self.stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_config() -> PassthroughConfig {
        PassthroughConfig::default()
    }

    #[test]
    fn test_new_manager_empty() {
        let manager = PassthroughManager::new(default_config());
        assert_eq!(manager.queue_pair_count(), 0);
        assert!(manager.active_queue_pairs().is_empty());
    }

    #[test]
    fn test_create_queue_pair() {
        let mut manager = PassthroughManager::new(default_config());
        let qp_id = manager.create_queue_pair(CoreId(0), NsId(1)).unwrap();
        assert_eq!(qp_id, QueuePairId(0));
        assert_eq!(manager.queue_pair_count(), 1);
        let queue = manager.get_queue_pair(qp_id).unwrap();
        assert_eq!(queue.core_id, CoreId(0));
        assert_eq!(queue.namespace, NsId(1));
    }

    #[test]
    fn test_create_duplicate_core_fails() {
        let mut manager = PassthroughManager::new(default_config());
        manager.create_queue_pair(CoreId(0), NsId(1)).unwrap();
        let result = manager.create_queue_pair(CoreId(0), NsId(2));
        assert!(matches!(
            result,
            Err(PassthroughError::CoreAlreadyBound(CoreId(0)))
        ));
    }

    #[test]
    fn test_create_exceeds_max_fails() {
        let mut config = default_config();
        config.max_queue_pairs = 2;
        let mut manager = PassthroughManager::new(config);
        manager.create_queue_pair(CoreId(0), NsId(1)).unwrap();
        manager.create_queue_pair(CoreId(1), NsId(1)).unwrap();
        let result = manager.create_queue_pair(CoreId(2), NsId(1));
        assert!(matches!(
            result,
            Err(PassthroughError::MaxQueuePairsReached(2))
        ));
    }

    #[test]
    fn test_remove_queue_pair() {
        let mut manager = PassthroughManager::new(default_config());
        let qp_id = manager.create_queue_pair(CoreId(0), NsId(1)).unwrap();
        manager.remove_queue_pair(qp_id).unwrap();
        assert_eq!(manager.queue_pair_count(), 0);
    }

    #[test]
    fn test_remove_nonexistent_fails() {
        let mut manager = PassthroughManager::new(default_config());
        let result = manager.remove_queue_pair(QueuePairId(999));
        assert!(matches!(
            result,
            Err(PassthroughError::QueueNotFound(QueuePairId(999)))
        ));
    }

    #[test]
    fn test_get_queue_pair() {
        let mut manager = PassthroughManager::new(default_config());
        let qp_id = manager.create_queue_pair(CoreId(0), NsId(1)).unwrap();
        let queue = manager.get_queue_pair(qp_id).unwrap();
        assert_eq!(queue.id, qp_id);
    }

    #[test]
    fn test_get_queue_for_core() {
        let mut manager = PassthroughManager::new(default_config());
        let qp_id = manager.create_queue_pair(CoreId(0), NsId(1)).unwrap();
        let found_id = manager.get_queue_for_core(CoreId(0)).unwrap();
        assert_eq!(found_id, qp_id);
    }

    #[test]
    fn test_submit_read() {
        let mut manager = PassthroughManager::new(default_config());
        manager.create_queue_pair(CoreId(0), NsId(1)).unwrap();
        let cmd_id = manager
            .submit(CoreId(0), NvmeOpType::Read, NsId(1), 0, 1, 4096, 100)
            .unwrap();
        assert_eq!(cmd_id, 0);
        let stats = manager.stats();
        assert_eq!(stats.reads, 1);
    }

    #[test]
    fn test_submit_write() {
        let mut manager = PassthroughManager::new(default_config());
        manager.create_queue_pair(CoreId(0), NsId(1)).unwrap();
        let cmd_id = manager
            .submit(CoreId(0), NvmeOpType::Write, NsId(1), 0, 1, 4096, 100)
            .unwrap();
        assert_eq!(cmd_id, 0);
        let stats = manager.stats();
        assert_eq!(stats.writes, 1);
    }

    #[test]
    fn test_submit_flush() {
        let mut manager = PassthroughManager::new(default_config());
        manager.create_queue_pair(CoreId(0), NsId(1)).unwrap();
        let cmd_id = manager
            .submit(CoreId(0), NvmeOpType::Flush, NsId(1), 0, 0, 0, 100)
            .unwrap();
        let stats = manager.stats();
        assert_eq!(stats.flushes, 1);
    }

    #[test]
    fn test_submit_atomic_write() {
        let mut manager = PassthroughManager::new(default_config());
        manager.create_queue_pair(CoreId(0), NsId(1)).unwrap();
        let cmd_id = manager
            .submit(CoreId(0), NvmeOpType::AtomicWrite, NsId(1), 0, 1, 4096, 100)
            .unwrap();
        let stats = manager.stats();
        assert_eq!(stats.atomic_writes, 1);
    }

    #[test]
    fn test_submit_no_queue_fails() {
        let mut manager = PassthroughManager::new(default_config());
        let result = manager.submit(CoreId(99), NvmeOpType::Read, NsId(1), 0, 1, 4096, 100);
        assert!(matches!(
            result,
            Err(PassthroughError::NoQueueForCore(CoreId(99)))
        ));
    }

    #[test]
    fn test_submit_queue_full_fails() {
        let mut config = default_config();
        config.sq_depth = 1;
        let mut manager = PassthroughManager::new(config);
        manager.create_queue_pair(CoreId(0), NsId(1)).unwrap();
        manager
            .submit(CoreId(0), NvmeOpType::Read, NsId(1), 0, 1, 4096, 100)
            .unwrap();
        let result = manager.submit(CoreId(0), NvmeOpType::Read, NsId(1), 0, 1, 4096, 100);
        assert!(matches!(result, Err(PassthroughError::QueueFull(_, 1))));
    }

    #[test]
    fn test_complete_success() {
        let mut manager = PassthroughManager::new(default_config());
        manager.create_queue_pair(CoreId(0), NsId(1)).unwrap();
        let cmd_id = manager
            .submit(CoreId(0), NvmeOpType::Read, NsId(1), 0, 1, 4096, 100)
            .unwrap();
        let entry = manager
            .complete(cmd_id, CompletionStatus::Success, 150)
            .unwrap();
        assert_eq!(entry.command_id, cmd_id);
        assert!(matches!(entry.status, CompletionStatus::Success));
    }

    #[test]
    fn test_complete_error() {
        let mut manager = PassthroughManager::new(default_config());
        manager.create_queue_pair(CoreId(0), NsId(1)).unwrap();
        let cmd_id = manager
            .submit(CoreId(0), NvmeOpType::Read, NsId(1), 0, 1, 4096, 100)
            .unwrap();
        let entry = manager
            .complete(cmd_id, CompletionStatus::MediaError, 150)
            .unwrap();
        assert!(matches!(entry.status, CompletionStatus::MediaError));
    }

    #[test]
    fn test_complete_not_found_fails() {
        let mut manager = PassthroughManager::new(default_config());
        let result = manager.complete(999, CompletionStatus::Success, 150);
        assert!(matches!(
            result,
            Err(PassthroughError::CommandNotFound(999))
        ));
    }

    #[test]
    fn test_drain_queue() {
        let mut manager = PassthroughManager::new(default_config());
        let qp_id = manager.create_queue_pair(CoreId(0), NsId(1)).unwrap();
        manager.drain_queue(qp_id).unwrap();
        let queue = manager.get_queue_pair(qp_id).unwrap();
        assert!(matches!(queue.state, QueueState::Draining));
    }

    #[test]
    fn test_drain_prevents_submit() {
        let mut manager = PassthroughManager::new(default_config());
        let qp_id = manager.create_queue_pair(CoreId(0), NsId(1)).unwrap();
        manager.drain_queue(qp_id).unwrap();
        let result = manager.submit(CoreId(0), NvmeOpType::Read, NsId(1), 0, 1, 4096, 100);
        assert!(matches!(result, Err(PassthroughError::QueueNotActive(_))));
    }

    #[test]
    fn test_reset_queue() {
        let mut manager = PassthroughManager::new(default_config());
        let qp_id = manager.create_queue_pair(CoreId(0), NsId(1)).unwrap();
        manager.drain_queue(qp_id).unwrap();
        manager.reset_queue(qp_id).unwrap();
        let queue = manager.get_queue_pair(qp_id).unwrap();
        assert!(matches!(queue.state, QueueState::Active));
    }

    #[test]
    fn test_pending_count() {
        let mut manager = PassthroughManager::new(default_config());
        let qp_id = manager.create_queue_pair(CoreId(0), NsId(1)).unwrap();
        assert_eq!(manager.pending_count(qp_id), 0);
        manager
            .submit(CoreId(0), NvmeOpType::Read, NsId(1), 0, 1, 4096, 100)
            .unwrap();
        assert_eq!(manager.pending_count(qp_id), 1);
    }

    #[test]
    fn test_is_queue_full() {
        let mut config = default_config();
        config.sq_depth = 1;
        let mut manager = PassthroughManager::new(config);
        let qp_id = manager.create_queue_pair(CoreId(0), NsId(1)).unwrap();
        assert!(!manager.is_queue_full(qp_id));
        manager
            .submit(CoreId(0), NvmeOpType::Read, NsId(1), 0, 1, 4096, 100)
            .unwrap();
        assert!(manager.is_queue_full(qp_id));
    }

    #[test]
    fn test_supports_atomic_writes() {
        let mut manager = PassthroughManager::new(default_config());
        assert!(manager.supports_atomic_writes());
    }

    #[test]
    fn test_queue_pair_count() {
        let mut manager = PassthroughManager::new(default_config());
        assert_eq!(manager.queue_pair_count(), 0);
        manager.create_queue_pair(CoreId(0), NsId(1)).unwrap();
        assert_eq!(manager.queue_pair_count(), 1);
        manager.create_queue_pair(CoreId(1), NsId(1)).unwrap();
        assert_eq!(manager.queue_pair_count(), 2);
    }

    #[test]
    fn test_active_queue_pairs_excludes_drained() {
        let mut manager = PassthroughManager::new(default_config());
        let qp1 = manager.create_queue_pair(CoreId(0), NsId(1)).unwrap();
        let qp2 = manager.create_queue_pair(CoreId(1), NsId(1)).unwrap();
        manager.drain_queue(qp1).unwrap();
        let active = manager.active_queue_pairs();
        assert_eq!(active.len(), 1);
        assert!(active.contains(&qp2));
    }

    #[test]
    fn test_stats_update_on_submit() {
        let mut manager = PassthroughManager::new(default_config());
        manager.create_queue_pair(CoreId(0), NsId(1)).unwrap();
        manager
            .submit(CoreId(0), NvmeOpType::Read, NsId(1), 0, 1, 4096, 100)
            .unwrap();
        let stats = manager.stats();
        assert_eq!(stats.total_submissions, 1);
    }

    #[test]
    fn test_stats_update_on_complete() {
        let mut manager = PassthroughManager::new(default_config());
        manager.create_queue_pair(CoreId(0), NsId(1)).unwrap();
        let cmd_id = manager
            .submit(CoreId(0), NvmeOpType::Read, NsId(1), 0, 1, 4096, 100)
            .unwrap();
        manager
            .complete(cmd_id, CompletionStatus::Success, 150)
            .unwrap();
        let stats = manager.stats();
        assert_eq!(stats.total_completions, 1);
    }

    #[test]
    fn test_passthrough_config_default() {
        let config = PassthroughConfig::default();
        assert_eq!(config.sq_depth, 1024);
        assert_eq!(config.cq_depth, 1024);
        assert_eq!(config.max_queue_pairs, 64);
        assert!(config.atomic_writes);
        assert_eq!(config.max_atomic_write_bytes, 65536);
        assert_eq!(config.min_kernel_major, 6);
        assert_eq!(config.min_kernel_minor, 20);
    }

    #[test]
    fn test_latency_tracking() {
        let mut manager = PassthroughManager::new(default_config());
        manager.create_queue_pair(CoreId(0), NsId(1)).unwrap();
        let cmd_id = manager
            .submit(CoreId(0), NvmeOpType::Read, NsId(1), 0, 1, 4096, 100)
            .unwrap();
        manager
            .complete(cmd_id, CompletionStatus::Success, 200)
            .unwrap();
        let stats = manager.stats();
        assert_eq!(stats.avg_latency_ns, 100);
        assert_eq!(stats.max_latency_ns, 100);
    }

    #[test]
    fn test_submit_atomic_disabled_fails() {
        let mut config = default_config();
        config.atomic_writes = false;
        let mut manager = PassthroughManager::new(config);
        manager.create_queue_pair(CoreId(0), NsId(1)).unwrap();
        let result = manager.submit(CoreId(0), NvmeOpType::AtomicWrite, NsId(1), 0, 1, 4096, 100);
        assert!(matches!(
            result,
            Err(PassthroughError::AtomicWritesDisabled)
        ));
    }
}
