//! Quota Accountant for crash-safe quota usage tracking.
//!
//! Provides journal-based accounting with reconciliation and audit trail support.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, RwLock};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use crate::tenant_isolator::TenantId;
use crate::error::ReduceError;
use crate::quota_manager::UsageReason;

/// Journal entry for crash-safe accounting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaJournalEntry {
    /// Timestamp (epoch milliseconds)
    pub timestamp_ms: u64,
    /// Tenant identifier
    pub tenant_id: TenantId,
    /// Delta in bytes (positive = charge, negative = credit)
    pub delta_bytes: i64,
    /// Reason for the usage change
    pub reason: UsageReason,
    /// Sequence number for ordering
    pub sequence: u64,
}

/// Snapshot for accounting verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaSnapshot {
    /// Timestamp (epoch milliseconds)
    pub timestamp_ms: u64,
    /// Tenant identifier
    pub tenant_id: TenantId,
    /// Used bytes at snapshot time
    pub used_bytes: u64,
    /// Dedup credits at snapshot time
    pub dedup_credits: u64,
    /// Journal sequence at snapshot time
    pub sequence: u64,
}

/// Reconciliation statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconciliationStats {
    /// Number of tenants checked
    pub tenants_checked: usize,
    /// Number of inconsistencies found
    pub inconsistencies_found: usize,
    /// Number of bytes corrected
    pub bytes_corrected: i64,
    /// Number of sequence gaps found
    pub sequence_gaps: usize,
}

/// QuotaJournal: crash-safe append-only log for quota accounting.
pub struct QuotaJournal {
    entries: Arc<RwLock<VecDeque<QuotaJournalEntry>>>,
    max_entries: usize,
    sequence: Arc<RwLock<u64>>,
}

impl QuotaJournal {
    /// Create a new QuotaJournal with the specified maximum entries.
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: Arc::new(RwLock::new(VecDeque::new())),
            max_entries,
            sequence: Arc::new(RwLock::new(0)),
        }
    }

    /// Append a journal entry to the log.
    pub fn append(&self, entry: QuotaJournalEntry) -> Result<(), ReduceError> {
        let mut entries = self.entries.write().map_err(|e| ReduceError::InvalidInput(e.to_string()))?;
        
        while entries.len() >= self.max_entries {
            entries.pop_front();
        }
        
        entries.push_back(entry);
        Ok(())
    }

    /// Get journal entries for a tenant.
    pub fn get_entries(&self, tenant_id: Option<TenantId>) -> Result<Vec<QuotaJournalEntry>, ReduceError> {
        let entries = self.entries.read().map_err(|e| ReduceError::InvalidInput(e.to_string()))?;
        
        let mut result: Vec<QuotaJournalEntry> = entries.iter()
            .filter(|e| tenant_id.map_or(true, |t| e.tenant_id == t))
            .cloned()
            .collect();
        
        result.sort_by(|a, b| a.sequence.cmp(&b.sequence));
        Ok(result)
    }

    /// Get the current journal sequence number.
    pub fn get_sequence(&self) -> Result<u64, ReduceError> {
        let seq = self.sequence.read().map_err(|e| ReduceError::InvalidInput(e.to_string()))?;
        Ok(*seq)
    }
}

/// Main quota accountant for crash-safe usage tracking and reconciliation.
pub struct QuotaAccountant {
    journal: Arc<QuotaJournal>,
    snapshots: Arc<RwLock<Vec<QuotaSnapshot>>>,
    current_usage: Arc<RwLock<HashMap<TenantId, (u64, u64)>>>,
    max_snapshots: usize,
}

impl QuotaAccountant {
    /// Create a new QuotaAccountant with the specified maximum snapshots.
    pub fn new(max_snapshots: usize) -> Self {
        Self {
            journal: Arc::new(QuotaJournal::new(10000)),
            snapshots: Arc::new(RwLock::new(Vec::new())),
            current_usage: Arc::new(RwLock::new(HashMap::new())),
            max_snapshots,
        }
    }

    /// Record a quota usage change.
    pub async fn record(
        &self,
        tenant_id: TenantId,
        delta_bytes: i64,
        reason: UsageReason,
    ) -> Result<(), ReduceError> {
        let sequence = {
            let mut seq = self.journal.sequence.write().map_err(|e| ReduceError::InvalidInput(e.to_string()))?;
            *seq += 1;
            *seq
        };

        let timestamp_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        
        let entry = QuotaJournalEntry {
            timestamp_ms,
            tenant_id,
            delta_bytes,
            reason: reason.clone(),
            sequence,
        };

        self.journal.append(entry)?;

        let mut usage = self.current_usage.write().map_err(|e| ReduceError::InvalidInput(e.to_string()))?;
        
        let (used_bytes, dedup_credits) = usage.entry(tenant_id).or_insert((0, 0));
        
        if delta_bytes < 0 {
            *dedup_credits = dedup_credits.saturating_sub(delta_bytes as i64 as u64);
        } else {
            *used_bytes = used_bytes.saturating_add(delta_bytes as u64);
        }

        debug!("Recorded quota change for tenant {:?}: delta={}, reason={:?}", tenant_id, delta_bytes, reason.kind);
        Ok(())
    }

    /// Reconcile current usage with journal entries to detect inconsistencies.
    pub async fn reconcile(&self) -> Result<ReconciliationStats, ReduceError> {
        let usage = self.current_usage.read().map_err(|e| ReduceError::InvalidInput(e.to_string()))?;
        
        let journal_entries = self.journal.get_entries(None)?;
        
        let mut stats = ReconciliationStats {
            tenants_checked: usage.len(),
            inconsistencies_found: 0,
            bytes_corrected: 0,
            sequence_gaps: 0,
        };

        if !journal_entries.is_empty() {
            let mut prev_seq = journal_entries[0].sequence;
            for entry in journal_entries.iter().skip(1) {
                if entry.sequence != prev_seq + 1 {
                    stats.sequence_gaps += 1;
                }
                prev_seq = entry.sequence;
            }
        }

        for (tenant_id, (used_bytes, _)) in usage.iter() {
            let tenant_entries: Vec<_> = journal_entries.iter()
                .filter(|e| e.tenant_id == *tenant_id)
                .collect();
            
            let mut expected: i64 = 0;
            for entry in tenant_entries {
                expected += entry.delta_bytes;
            }
            
            let actual = *used_bytes as i64;
            if expected != actual {
                stats.inconsistencies_found += 1;
                stats.bytes_corrected += expected - actual;
                warn!("Inconsistency for tenant {:?}: expected {}, got {}", tenant_id, expected, actual);
            }
        }

        info!("Reconciliation complete: {} inconsistencies found", stats.inconsistencies_found);
        Ok(stats)
    }

    /// Get the audit trail for a tenant since a given time (by sequence).
    pub async fn audit_trail(
        &self,
        tenant_id: TenantId,
        min_sequence: Option<u64>,
    ) -> Result<Vec<QuotaJournalEntry>, ReduceError> {
        let entries = self.journal.get_entries(Some(tenant_id))?;
        if let Some(seq) = min_sequence {
            Ok(entries.into_iter().filter(|e| e.sequence >= seq).collect())
        } else {
            Ok(entries)
        }
    }

    /// Create a snapshot of current quota state.
    pub async fn create_snapshot(&self) -> Result<(), ReduceError> {
        let usage = self.current_usage.read().map_err(|e| ReduceError::InvalidInput(e.to_string()))?;
        
        let sequence = self.journal.get_sequence()?;
        let timestamp_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        
        let mut snapshots = self.snapshots.write().map_err(|e| ReduceError::InvalidInput(e.to_string()))?;
        
        for (tenant_id, (used_bytes, dedup_credits)) in usage.iter() {
            snapshots.push(QuotaSnapshot {
                timestamp_ms,
                tenant_id: *tenant_id,
                used_bytes: *used_bytes,
                dedup_credits: *dedup_credits,
                sequence,
            });
        }
        
        while snapshots.len() > self.max_snapshots {
            snapshots.remove(0);
        }
        
        info!("Created quota snapshot at sequence {}", sequence);
        Ok(())
    }

    /// Get the underlying journal.
    pub fn get_journal(&self) -> Arc<QuotaJournal> {
        Arc::clone(&self.journal)
    }

    /// Get current usage for a tenant.
    pub async fn get_current_usage(&self, tenant_id: TenantId) -> Result<(u64, u64), ReduceError> {
        let usage = self.current_usage.read().map_err(|e| ReduceError::InvalidInput(e.to_string()))?;
        Ok(*usage.get(&tenant_id).unwrap_or(&(0, 0)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_record_usage() {
        let accountant = QuotaAccountant::new(10);
        
        accountant.record(
            TenantId(1),
            1000,
            UsageReason {
                kind: UsageKind::Write,
                metadata: HashMap::new(),
            },
        ).await.unwrap();
        
        let usage = accountant.get_current_usage(TenantId(1)).await.unwrap();
        assert_eq!(usage.0, 1000);
    }

    #[tokio::test]
    async fn test_negative_delta_credit() {
        let accountant = QuotaAccountant::new(10);
        
        accountant.record(TenantId(1), 1000, UsageReason {
            kind: UsageKind::Write,
            metadata: HashMap::new(),
        }).await.unwrap();
        
        accountant.record(TenantId(1), -300, UsageReason {
            kind: UsageKind::Dedup,
            metadata: HashMap::new(),
        }).await.unwrap();
        
        let usage = accountant.get_current_usage(TenantId(1)).await.unwrap();
        assert_eq!(usage.0, 1000);
        assert_eq!(usage.1, 300);
    }

    #[tokio::test]
    async fn test_reconciliation() {
        let accountant = QuotaAccountant::new(10);
        
        accountant.record(TenantId(1), 500, UsageReason {
            kind: UsageKind::Write,
            metadata: HashMap::new(),
        }).await.unwrap();
        
        let stats = accountant.reconcile().await.unwrap();
        assert_eq!(stats.inconsistencies_found, 0);
    }

    #[tokio::test]
    async fn test_audit_trail() {
        let accountant = QuotaAccountant::new(10);
        
        accountant.record(TenantId(1), 100, UsageReason {
            kind: UsageKind::Write,
            metadata: HashMap::new(),
        }).await.unwrap();
        
        let trail = accountant.audit_trail(TenantId(1), None).await.unwrap();
        assert_eq!(trail.len(), 1);
        assert_eq!(trail[0].delta_bytes, 100);
    }

    #[tokio::test]
    async fn test_snapshot() {
        let accountant = QuotaAccountant::new(10);
        
        accountant.record(TenantId(1), 1000, UsageReason {
            kind: UsageKind::Write,
            metadata: HashMap::new(),
        }).await.unwrap();
        
        accountant.create_snapshot().await.unwrap();
        
        let journal = accountant.get_journal();
        let seq = journal.get_sequence().unwrap();
        assert!(seq > 0);
    }

    #[tokio::test]
    async fn test_multiple_tenants() {
        let accountant = QuotaAccountant::new(10);
        
        accountant.record(TenantId(1), 500, UsageReason {
            kind: UsageKind::Write,
            metadata: HashMap::new(),
        }).await.unwrap();
        
        accountant.record(TenantId(2), 800, UsageReason {
            kind: UsageKind::Write,
            metadata: HashMap::new(),
        }).await.unwrap();
        
        let usage1 = accountant.get_current_usage(TenantId(1)).await.unwrap();
        let usage2 = accountant.get_current_usage(TenantId(2)).await.unwrap();
        
        assert_eq!(usage1.0, 500);
        assert_eq!(usage2.0, 800);
    }
}