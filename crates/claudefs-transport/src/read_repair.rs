//! EC Read Repair Tracker.
//!
//! Tracks in-progress read repair operations for erasure-coded segments.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::sync::Mutex;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RepairId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShardRepairState {
    Fetching,
    Fetched,
    Failed,
    Missing,
    Reconstructing,
    Repaired,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepairShard {
    pub node_id: [u8; 16],
    pub shard_index: usize,
    pub state: ShardRepairState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum RepairPriority {
    Background,
    Foreground,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadRepairConfig {
    pub timeout_ms: u64,
    pub max_concurrent: usize,
}

impl Default for ReadRepairConfig {
    fn default() -> Self {
        Self {
            timeout_ms: 30000,
            max_concurrent: 16,
        }
    }
}

#[derive(Debug, Error)]
pub enum RepairError {
    #[error("repair {0:?} not found")]
    NotFound(RepairId),
    #[error("too many concurrent repairs (max {0})")]
    TooManyConcurrent(usize),
    #[error("cannot reconstruct: only {available} shards available, need {needed}")]
    InsufficientShards { available: usize, needed: usize },
    #[error("repair {0:?} already completed")]
    AlreadyCompleted(RepairId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RepairOpState {
    Fetching,
    Reconstructing,
    WritingBack,
    Complete,
    Failed,
    TimedOut,
}

pub struct RepairOp {
    pub id: RepairId,
    pub segment_id: u64,
    pub priority: RepairPriority,
    pub shards: Vec<RepairShard>,
    pub state: RepairOpState,
    pub created_at_ms: u64,
    pub ec_data_shards: usize,
    pub ec_parity_shards: usize,
}

impl RepairOp {
    pub fn new(
        id: RepairId,
        segment_id: u64,
        priority: RepairPriority,
        shards: Vec<RepairShard>,
        ec_data_shards: usize,
        ec_parity_shards: usize,
        now_ms: u64,
    ) -> Self {
        Self {
            id,
            segment_id,
            priority,
            shards,
            state: RepairOpState::Fetching,
            created_at_ms: now_ms,
            ec_data_shards,
            ec_parity_shards,
        }
    }

    pub fn record_fetch(&mut self, node_id: &[u8; 16], success: bool) {
        for shard in &mut self.shards {
            if &shard.node_id == node_id {
                if success {
                    shard.state = ShardRepairState::Fetched;
                } else {
                    shard.state = ShardRepairState::Failed;
                }
                break;
            }
        }
    }

    pub fn begin_reconstruct(&mut self) -> Result<(), RepairError> {
        if !self.can_reconstruct() {
            return Err(RepairError::InsufficientShards {
                available: self.fetched_count(),
                needed: self.ec_data_shards,
            });
        }
        self.state = RepairOpState::Reconstructing;
        for shard in &mut self.shards {
            if shard.state == ShardRepairState::Missing || shard.state == ShardRepairState::Failed {
                shard.state = ShardRepairState::Reconstructing;
            }
        }
        Ok(())
    }

    pub fn begin_writeback(&mut self) {
        self.state = RepairOpState::WritingBack;
    }

    pub fn complete(&mut self) {
        self.state = RepairOpState::Complete;
        for shard in &mut self.shards {
            if shard.state == ShardRepairState::Reconstructing {
                shard.state = ShardRepairState::Repaired;
            }
        }
    }

    pub fn fail(&mut self) {
        self.state = RepairOpState::Failed;
    }

    pub fn check_timeout(&mut self, now_ms: u64) -> bool {
        let elapsed = now_ms.saturating_sub(self.created_at_ms);
        if elapsed >= 30000 {
            self.state = RepairOpState::TimedOut;
            true
        } else {
            false
        }
    }

    pub fn fetched_count(&self) -> usize {
        self.shards
            .iter()
            .filter(|s| s.state == ShardRepairState::Fetched)
            .count()
    }

    pub fn missing_count(&self) -> usize {
        self.shards
            .iter()
            .filter(|s| s.state == ShardRepairState::Missing)
            .count()
    }

    pub fn can_reconstruct(&self) -> bool {
        self.fetched_count() >= self.ec_data_shards
    }
}

pub struct ReadRepairManager {
    config: ReadRepairConfig,
    next_id: AtomicU64,
    ops: Mutex<HashMap<RepairId, RepairOp>>,
    stats: Arc<ReadRepairStats>,
}

impl ReadRepairManager {
    pub fn new(config: ReadRepairConfig) -> Self {
        Self {
            config,
            next_id: AtomicU64::new(1),
            ops: Mutex::new(HashMap::new()),
            stats: Arc::new(ReadRepairStats::new()),
        }
    }

    pub fn start_repair(
        &self,
        segment_id: u64,
        priority: RepairPriority,
        shards: Vec<RepairShard>,
        ec_data_shards: usize,
        ec_parity_shards: usize,
        now_ms: u64,
    ) -> Result<RepairId, RepairError> {
        let active = {
            let ops = self.ops.lock().unwrap();
            ops.len()
        };
        if active >= self.config.max_concurrent {
            return Err(RepairError::TooManyConcurrent(self.config.max_concurrent));
        }

        let id = RepairId(self.next_id.fetch_add(1, Ordering::Relaxed));
        let op = RepairOp::new(
            id,
            segment_id,
            priority,
            shards,
            ec_data_shards,
            ec_parity_shards,
            now_ms,
        );

        self.stats.repairs_started.fetch_add(1, Ordering::Relaxed);
        match priority {
            RepairPriority::Foreground => {
                self.stats
                    .foreground_repairs
                    .fetch_add(1, Ordering::Relaxed);
            }
            RepairPriority::Background => {
                self.stats
                    .background_repairs
                    .fetch_add(1, Ordering::Relaxed);
            }
        }

        let mut ops = self.ops.lock().unwrap();
        ops.insert(id, op);

        Ok(id)
    }

    pub fn record_fetch(
        &self,
        id: RepairId,
        node_id: &[u8; 16],
        success: bool,
    ) -> Option<RepairOpState> {
        let mut ops = self.ops.lock().unwrap();
        let op = ops.get_mut(&id)?;
        op.record_fetch(node_id, success);
        Some(op.state)
    }

    pub fn begin_reconstruct(&self, id: RepairId) -> Result<RepairOpState, RepairError> {
        let mut ops = self.ops.lock().unwrap();
        let op = ops.get_mut(&id).ok_or(RepairError::NotFound(id))?;

        if op.state == RepairOpState::Complete
            || op.state == RepairOpState::Failed
            || op.state == RepairOpState::TimedOut
        {
            return Err(RepairError::AlreadyCompleted(id));
        }

        op.begin_reconstruct()?;
        Ok(op.state)
    }

    pub fn complete_repair(&self, id: RepairId) -> Result<(), RepairError> {
        let mut ops = self.ops.lock().unwrap();
        let op = ops.get_mut(&id).ok_or(RepairError::NotFound(id))?;

        let shard_count = op.shards.len();
        op.complete();

        self.stats.repairs_completed.fetch_add(1, Ordering::Relaxed);
        self.stats
            .shards_repaired
            .fetch_add(shard_count as u64, Ordering::Relaxed);

        Ok(())
    }

    pub fn check_timeouts(&self, now_ms: u64) -> Vec<RepairId> {
        let mut ops = self.ops.lock().unwrap();
        let mut timed_out = Vec::new();

        for (id, op) in ops.iter_mut() {
            if op.check_timeout(now_ms) {
                self.stats.repairs_timed_out.fetch_add(1, Ordering::Relaxed);
                timed_out.push(*id);
            }
        }

        timed_out
    }

    pub fn remove(&self, id: RepairId) {
        let mut ops = self.ops.lock().unwrap();
        if let Some(op) = ops.remove(&id) {
            if op.state == RepairOpState::Failed {
                self.stats.repairs_failed.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    pub fn active_count(&self) -> usize {
        let ops = self.ops.lock().unwrap();
        ops.len()
    }

    pub fn stats(&self) -> Arc<ReadRepairStats> {
        Arc::clone(&self.stats)
    }
}

pub struct ReadRepairStats {
    pub repairs_started: AtomicU64,
    pub repairs_completed: AtomicU64,
    pub repairs_failed: AtomicU64,
    pub repairs_timed_out: AtomicU64,
    pub shards_repaired: AtomicU64,
    pub foreground_repairs: AtomicU64,
    pub background_repairs: AtomicU64,
}

impl ReadRepairStats {
    pub fn new() -> Self {
        Self {
            repairs_started: AtomicU64::new(0),
            repairs_completed: AtomicU64::new(0),
            repairs_failed: AtomicU64::new(0),
            repairs_timed_out: AtomicU64::new(0),
            shards_repaired: AtomicU64::new(0),
            foreground_repairs: AtomicU64::new(0),
            background_repairs: AtomicU64::new(0),
        }
    }

    pub fn snapshot(&self, active_repairs: usize) -> ReadRepairStatsSnapshot {
        ReadRepairStatsSnapshot {
            repairs_started: self.repairs_started.load(Ordering::Relaxed),
            repairs_completed: self.repairs_completed.load(Ordering::Relaxed),
            repairs_failed: self.repairs_failed.load(Ordering::Relaxed),
            repairs_timed_out: self.repairs_timed_out.load(Ordering::Relaxed),
            shards_repaired: self.shards_repaired.load(Ordering::Relaxed),
            foreground_repairs: self.foreground_repairs.load(Ordering::Relaxed),
            background_repairs: self.background_repairs.load(Ordering::Relaxed),
            active_repairs,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadRepairStatsSnapshot {
    pub repairs_started: u64,
    pub repairs_completed: u64,
    pub repairs_failed: u64,
    pub repairs_timed_out: u64,
    pub shards_repaired: u64,
    pub foreground_repairs: u64,
    pub background_repairs: u64,
    pub active_repairs: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_shard(node_idx: usize) -> RepairShard {
        RepairShard {
            node_id: [node_idx as u8; 16],
            shard_index: node_idx,
            state: ShardRepairState::Fetching,
        }
    }

    #[test]
    fn test_new_repair_op() {
        let id = RepairId(1);
        let shards = vec![make_shard(0), make_shard(1), make_shard(2)];
        let op = RepairOp::new(id, 100, RepairPriority::Foreground, shards, 2, 1, 1000);

        assert_eq!(op.id, RepairId(1));
        assert_eq!(op.state, RepairOpState::Fetching);
        assert_eq!(op.fetched_count(), 0);
    }

    #[test]
    fn test_record_fetch_success() {
        let mut op = RepairOp::new(
            RepairId(1),
            100,
            RepairPriority::Foreground,
            vec![make_shard(0), make_shard(1)],
            2,
            1,
            1000,
        );

        let node_id = [0u8; 16];
        op.record_fetch(&node_id, true);

        assert_eq!(op.fetched_count(), 1);
    }

    #[test]
    fn test_record_fetch_failure() {
        let mut op = RepairOp::new(
            RepairId(1),
            100,
            RepairPriority::Foreground,
            vec![make_shard(0), make_shard(1)],
            2,
            1,
            1000,
        );

        let node_id = [0u8; 16];
        op.record_fetch(&node_id, false);

        assert_eq!(op.fetched_count(), 0);
        assert_eq!(op.shards[0].state, ShardRepairState::Failed);
    }

    #[test]
    fn test_can_reconstruct_true() {
        let mut op = RepairOp::new(
            RepairId(1),
            100,
            RepairPriority::Foreground,
            vec![make_shard(0), make_shard(1), make_shard(2)],
            2,
            1,
            1000,
        );

        op.record_fetch(&[0u8; 16], true);
        op.record_fetch(&[1u8; 16], true);

        assert!(op.can_reconstruct());
    }

    #[test]
    fn test_can_reconstruct_false() {
        let mut op = RepairOp::new(
            RepairId(1),
            100,
            RepairPriority::Foreground,
            vec![make_shard(0), make_shard(1), make_shard(2)],
            2,
            1,
            1000,
        );

        op.record_fetch(&[0u8; 16], true);

        assert!(!op.can_reconstruct());
    }

    #[test]
    fn test_begin_reconstruct_success() {
        let mut op = RepairOp::new(
            RepairId(1),
            100,
            RepairPriority::Foreground,
            vec![make_shard(0), make_shard(1), make_shard(2)],
            2,
            1,
            1000,
        );

        op.record_fetch(&[0u8; 16], true);
        op.record_fetch(&[1u8; 16], true);

        let result = op.begin_reconstruct();
        assert!(result.is_ok());
        assert_eq!(op.state, RepairOpState::Reconstructing);
    }

    #[test]
    fn test_begin_reconstruct_insufficient() {
        let mut op = RepairOp::new(
            RepairId(1),
            100,
            RepairPriority::Foreground,
            vec![make_shard(0), make_shard(1), make_shard(2)],
            2,
            1,
            1000,
        );

        op.record_fetch(&[0u8; 16], true);

        let result = op.begin_reconstruct();
        assert!(result.is_err());
        match result {
            Err(RepairError::InsufficientShards { available, needed }) => {
                assert_eq!(available, 1);
                assert_eq!(needed, 2);
            }
            _ => panic!("expected InsufficientShards"),
        }
    }

    #[test]
    fn test_complete_repair() {
        let mut op = RepairOp::new(
            RepairId(1),
            100,
            RepairPriority::Foreground,
            vec![make_shard(0), make_shard(1), make_shard(2)],
            2,
            1,
            1000,
        );

        op.record_fetch(&[0u8; 16], true);
        op.record_fetch(&[1u8; 16], true);
        op.begin_reconstruct().unwrap();
        op.begin_writeback();
        op.complete();

        assert_eq!(op.state, RepairOpState::Complete);
        assert_eq!(op.shards[0].state, ShardRepairState::Fetched);
        assert_eq!(op.shards[1].state, ShardRepairState::Fetched);
        assert_eq!(op.shards[2].state, ShardRepairState::Fetching);
    }

    #[test]
    fn test_repair_timeout() {
        let mut op = RepairOp::new(
            RepairId(1),
            100,
            RepairPriority::Foreground,
            vec![make_shard(0)],
            1,
            1,
            1000,
        );

        let timed_out = op.check_timeout(40000);
        assert!(timed_out);
        assert_eq!(op.state, RepairOpState::TimedOut);
    }

    #[test]
    fn test_repair_timeout_not_expired() {
        let mut op = RepairOp::new(
            RepairId(1),
            100,
            RepairPriority::Foreground,
            vec![make_shard(0)],
            1,
            1,
            1000,
        );

        let timed_out = op.check_timeout(20000);
        assert!(!timed_out);
        assert_eq!(op.state, RepairOpState::Fetching);
    }

    #[test]
    fn test_manager_start_too_many() {
        let config = ReadRepairConfig {
            max_concurrent: 2,
            ..Default::default()
        };
        let manager = ReadRepairManager::new(config);

        let shards = vec![make_shard(0), make_shard(1)];

        let _ = manager.start_repair(1, RepairPriority::Foreground, shards.clone(), 1, 1, 1000);
        let _ = manager.start_repair(2, RepairPriority::Foreground, shards.clone(), 1, 1, 1000);

        let result = manager.start_repair(3, RepairPriority::Foreground, shards, 1, 1, 1000);
        assert!(result.is_err());
        match result {
            Err(RepairError::TooManyConcurrent(max)) => assert_eq!(max, 2),
            _ => panic!("expected TooManyConcurrent"),
        }
    }

    #[test]
    fn test_manager_check_timeouts() {
        let manager = ReadRepairManager::new(Default::default());

        let shards = vec![make_shard(0)];
        let id = manager
            .start_repair(1, RepairPriority::Foreground, shards, 1, 1, 1000)
            .unwrap();

        let timed = manager.check_timeouts(40000);
        assert_eq!(timed.len(), 1);
        assert_eq!(timed[0], id);
    }

    #[test]
    fn test_manager_active_count() {
        let manager = ReadRepairManager::new(Default::default());

        assert_eq!(manager.active_count(), 0);

        let shards = vec![make_shard(0)];
        let _ = manager
            .start_repair(1, RepairPriority::Foreground, shards.clone(), 1, 1, 1000)
            .unwrap();

        assert_eq!(manager.active_count(), 1);
    }

    #[test]
    fn test_priority_ordering() {
        assert!(RepairPriority::Foreground > RepairPriority::Background);
    }

    #[test]
    fn test_stats_counts() {
        let manager = ReadRepairManager::new(Default::default());

        let shards = vec![make_shard(0), make_shard(1), make_shard(2)];
        let id = manager
            .start_repair(1, RepairPriority::Foreground, shards.clone(), 2, 1, 1000)
            .unwrap();

        manager.record_fetch(id, &[0u8; 16], true);
        manager.record_fetch(id, &[1u8; 16], true);
        manager.begin_reconstruct(id).unwrap();
        manager.complete_repair(id).unwrap();

        let snapshot = manager.stats().snapshot(manager.active_count());
        assert_eq!(snapshot.repairs_started, 1);
        assert_eq!(snapshot.repairs_completed, 1);
        assert_eq!(snapshot.foreground_repairs, 1);
    }

    #[test]
    fn test_missing_vs_failed_shards() {
        let mut op = RepairOp::new(
            RepairId(1),
            100,
            RepairPriority::Foreground,
            vec![
                RepairShard {
                    node_id: [0u8; 16],
                    shard_index: 0,
                    state: ShardRepairState::Missing,
                },
                RepairShard {
                    node_id: [1u8; 16],
                    shard_index: 1,
                    state: ShardRepairState::Failed,
                },
                RepairShard {
                    node_id: [2u8; 16],
                    shard_index: 2,
                    state: ShardRepairState::Fetched,
                },
            ],
            2,
            1,
            1000,
        );

        assert_eq!(op.missing_count(), 1);
        assert_eq!(op.fetched_count(), 1);
    }
}
