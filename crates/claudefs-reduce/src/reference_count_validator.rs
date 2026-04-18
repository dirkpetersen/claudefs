use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::time::Instant;

use crate::error::ReduceError;

pub type BlockId = [u8; 32];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarkAndSweepAudit {
    pub reachable_blocks: HashSet<BlockId>,
    pub orphaned_blocks: Vec<BlockId>,
    pub corrupted_refcounts: Vec<(BlockId, u64)>,
    pub last_audit_time_ms: u64,
    pub audit_duration_ms: u64,
    pub blocks_scanned: u64,
}

impl Default for MarkAndSweepAudit {
    fn default() -> Self {
        Self {
            reachable_blocks: HashSet::new(),
            orphaned_blocks: Vec::new(),
            corrupted_refcounts: Vec::new(),
            last_audit_time_ms: 0,
            audit_duration_ms: 0,
            blocks_scanned: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReconciliationAction {
    DeleteOrphan(BlockId),
    FixRefcount {
        block_id: BlockId,
        correct_count: u64,
    },
    Quarantine(BlockId),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconciliationResult {
    pub actions: Vec<ReconciliationAction>,
    pub blocks_deleted: u64,
    pub refcounts_corrected: u64,
    pub blocks_quarantined: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ValidatorStats {
    pub total_audits: u64,
    pub total_orphans_detected: u64,
    pub total_corruptions_detected: u64,
    pub last_audit_duration_ms: u64,
}

pub trait ReferenceCountValidator: Send + Sync {
    fn audit(&mut self) -> Result<MarkAndSweepAudit, ReduceError>;
    fn reconcile(&mut self, audit: &MarkAndSweepAudit)
        -> Result<ReconciliationResult, ReduceError>;
    fn mark_reachable(&mut self, block_id: BlockId);
    fn get_stats(&self) -> ValidatorStats;
}

pub struct RefCountValidator {
    reachable: HashSet<BlockId>,
    stats: ValidatorStats,
    in_memory_refcounts: HashMap<BlockId, u64>,
    is_audit_running: bool,
}

impl RefCountValidator {
    pub fn new() -> Self {
        Self {
            reachable: HashSet::new(),
            stats: ValidatorStats::default(),
            in_memory_refcounts: HashMap::new(),
            is_audit_running: false,
        }
    }

    pub fn set_refcounts(&mut self, refcounts: HashMap<BlockId, u64>) {
        self.in_memory_refcounts = refcounts;
    }

    pub fn add_block(&mut self, block_id: BlockId, refcount: u64) {
        self.in_memory_refcounts.insert(block_id, refcount);
    }

    fn check_consistency(&self) -> (Vec<BlockId>, Vec<(BlockId, u64)>) {
        let mut orphans = Vec::new();
        let mut corrupted = Vec::new();

        for (block_id, &refcount) in &self.in_memory_refcounts {
            if refcount > 0 && !self.reachable.contains(block_id) {
                orphans.push(*block_id);
            }
            if refcount > 100000 {
                corrupted.push((*block_id, refcount));
            }
        }

        (orphans, corrupted)
    }
}

impl Default for RefCountValidator {
    fn default() -> Self {
        Self::new()
    }
}

impl ReferenceCountValidator for RefCountValidator {
    fn audit(&mut self) -> Result<MarkAndSweepAudit, ReduceError> {
        if self.is_audit_running {
            return Err(ReduceError::GcAuditFailed(
                "Audit already running".to_string(),
            ));
        }

        self.is_audit_running = true;
        let start = Instant::now();

        let blocks_scanned = self.in_memory_refcounts.len() as u64;

        let (orphans, corrupted) = self.check_consistency();

        let elapsed = start.elapsed().as_millis() as u64;

        // Take reachable blocks AFTER consistency check
        let mut reachable_blocks = HashSet::new();
        std::mem::swap(&mut self.reachable, &mut reachable_blocks);

        let audit = MarkAndSweepAudit {
            reachable_blocks,
            orphaned_blocks: orphans,
            corrupted_refcounts: corrupted,
            last_audit_time_ms: self.stats.total_audits,
            audit_duration_ms: elapsed,
            blocks_scanned,
        };

        self.stats.total_audits += 1;
        self.stats.total_orphans_detected += audit.orphaned_blocks.len() as u64;
        self.stats.total_corruptions_detected += audit.corrupted_refcounts.len() as u64;
        self.stats.last_audit_duration_ms = audit.audit_duration_ms;

        self.is_audit_running = false;

        Ok(audit)
    }

    fn reconcile(
        &mut self,
        audit: &MarkAndSweepAudit,
    ) -> Result<ReconciliationResult, ReduceError> {
        let mut actions = Vec::new();
        let mut blocks_deleted = 0u64;
        let mut refcounts_corrected = 0u64;
        let blocks_quarantined = 0u64;

        for block_id in &audit.orphaned_blocks {
            actions.push(ReconciliationAction::DeleteOrphan(*block_id));
            self.in_memory_refcounts.remove(block_id);
            blocks_deleted += 1;
        }

        for (block_id, _) in &audit.corrupted_refcounts {
            let correct_count = 0;
            actions.push(ReconciliationAction::FixRefcount {
                block_id: *block_id,
                correct_count,
            });
            self.in_memory_refcounts.insert(*block_id, correct_count);
            refcounts_corrected += 1;
        }

        Ok(ReconciliationResult {
            actions,
            blocks_deleted,
            refcounts_corrected,
            blocks_quarantined,
        })
    }

    fn mark_reachable(&mut self, block_id: BlockId) {
        self.reachable.insert(block_id);
    }

    fn get_stats(&self) -> ValidatorStats {
        self.stats.clone()
    }
}
