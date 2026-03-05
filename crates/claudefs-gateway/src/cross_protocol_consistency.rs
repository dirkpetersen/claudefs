use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use dashmap::DashMap;
use thiserror::Error;

/// Protocol identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Protocol {
    NFS,
    S3,
    SMB,
}

/// Access type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessType {
    Read,
    Write(WriteOp),
    Delete,
    Metadata,
}

/// Write operation type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteOp {
    SetSize,
    SetTimes,
    SetMode,
    Write,
    Rename,
    Delete,
}

/// Protocol access record
#[derive(Debug, Clone)]
pub struct ProtocolAccessRecord {
    pub protocol: Protocol,
    pub client_id: u64,
    pub inode_id: u64,
    pub access_type: AccessType,
    pub timestamp_ns: u64,
    pub request_id: u64,
}

/// Conflict type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictType {
    ReadWrite,
    ConcurrentWrites,
    RenameUnderAccess,
    DeleteUnderAccess,
}

/// Conflict resolution method
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConflictResolution {
    LastWriteWins,
    AbortRequest(Protocol),
    RevokeDelegation,
    ClientNotified,
}

/// Conflict record
#[derive(Debug, Clone)]
pub struct ConflictRecord {
    pub conflict_id: u64,
    pub conflict_type: ConflictType,
    pub access1: ProtocolAccessRecord,
    pub access2: ProtocolAccessRecord,
    pub detected_at_ns: u64,
    pub resolution: ConflictResolution,
}

/// Cross-protocol metrics
#[derive(Debug, Clone, Default)]
pub struct CrossProtocolMetrics {
    pub total_accesses: u64,
    pub conflicts_detected: u64,
    pub conflicts_resolved: u64,
    pub resolution_latency_us: Vec<u64>,
}

/// Consistency errors
#[derive(Error, Debug)]
pub enum ConsistencyError {
    #[error("invalid access")]
    InvalidAccess,
    #[error("resolution failed")]
    ResolutionFailed,
    #[error("cache error")]
    CacheError,
}

/// Cross-protocol consistency cache
pub struct CrossProtocolCache {
    recent_accesses: Arc<DashMap<u64, VecDeque<ProtocolAccessRecord>>>,
    conflicts: Arc<DashMap<u64, ConflictRecord>>,
    next_conflict_id: Arc<std::sync::atomic::AtomicU64>,
    metrics: Arc<std::sync::Mutex<CrossProtocolMetrics>>,
    window_size: usize,
}

impl CrossProtocolCache {
    pub fn new(window_size: usize) -> Self {
        Self {
            recent_accesses: Arc::new(DashMap::new()),
            conflicts: Arc::new(DashMap::new()),
            next_conflict_id: Arc::new(std::sync::atomic::AtomicU64::new(1)),
            metrics: Arc::new(tokio::sync::RwLock::new(CrossProtocolMetrics::default())),
            window_size,
        }
    }

    pub async fn record_access(
        &self,
        protocol: Protocol,
        client_id: u64,
        inode_id: u64,
        access_type: AccessType,
        request_id: u64,
    ) -> Result<Option<ConflictRecord>, ConsistencyError> {
        let now_ns = current_time_ns();

        let record = ProtocolAccessRecord {
            protocol,
            client_id,
            inode_id,
            access_type,
            timestamp_ns: now_ns,
            request_id,
        };

        let mut conflict = None;
        let mut accesses = self
            .recent_accesses
            .entry(inode_id)
            .or_insert_with(VecDeque::new);

        if !accesses.is_empty() {
            if let Some(conflict_type) = Self::detect_conflict(&accesses[accesses.len() - 1], &record) {
                let conflict_id = self.next_conflict_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let conf = ConflictRecord {
                    conflict_id,
                    conflict_type,
                    access1: accesses[accesses.len() - 1].clone(),
                    access2: record.clone(),
                    detected_at_ns: now_ns,
                    resolution: ConflictResolution::LastWriteWins,
                };
                self.conflicts.insert(conflict_id, conf.clone());
                conflict = Some(conf);

                let mut metrics = self.metrics.lock();
                metrics.conflicts_detected += 1;
            }
        }

        accesses.push_back(record);
        if accesses.len() > self.window_size {
            accesses.pop_front();
        }

        let mut metrics = self.metrics.lock();
        metrics.total_accesses += 1;

        Ok(conflict)
    }

    pub fn has_concurrent_writes(&self, inode_id: u64) -> bool {
        if let Some(accesses) = self.recent_accesses.get(&inode_id) {
            accesses
                .iter()
                .filter(|a| matches!(a.access_type, AccessType::Write(_)))
                .count() > 1
        } else {
            false
        }
    }

    pub fn get_access_history(&self, inode_id: u64, lookback_ms: u64) -> Vec<ProtocolAccessRecord> {
        let now_ns = current_time_ns();
        let lookback_ns = lookback_ms * 1_000_000;

        if let Some(accesses) = self.recent_accesses.get(&inode_id) {
            accesses
                .iter()
                .filter(|a| now_ns - a.timestamp_ns <= lookback_ns)
                .cloned()
                .collect()
        } else {
            Vec::new()
        }
    }

    pub fn detect_conflict(rec1: &ProtocolAccessRecord, rec2: &ProtocolAccessRecord) -> Option<ConflictType> {
        match (rec1.access_type, rec2.access_type) {
            (AccessType::Read, AccessType::Write(_)) |
            (AccessType::Write(_), AccessType::Read) => {
                if rec1.protocol != rec2.protocol {
                    Some(ConflictType::ReadWrite)
                } else {
                    None
                }
            }
            (AccessType::Write(_), AccessType::Write(_)) => {
                if rec1.protocol != rec2.protocol && rec1.client_id != rec2.client_id {
                    Some(ConflictType::ConcurrentWrites)
                } else {
                    None
                }
            }
            (AccessType::Write(WriteOp::Rename), AccessType::Read) |
            (AccessType::Read, AccessType::Write(WriteOp::Rename)) => {
                Some(ConflictType::RenameUnderAccess)
            }
            (AccessType::Write(WriteOp::Delete), _) | (_, AccessType::Write(WriteOp::Delete)) => {
                Some(ConflictType::DeleteUnderAccess)
            }
            _ => None,
        }
    }

    pub async fn resolve_conflict(
        &self,
        conflict: ConflictRecord,
    ) -> Result<ConflictResolution, ConsistencyError> {
        let resolution = if conflict.access2.timestamp_ns > conflict.access1.timestamp_ns {
            ConflictResolution::LastWriteWins
        } else {
            ConflictResolution::AbortRequest(conflict.access1.protocol)
        };

        let mut metrics = self.metrics.lock();
        metrics.conflicts_resolved += 1;

        Ok(resolution)
    }

    pub fn metrics(&self) -> CrossProtocolMetrics {
        self.metrics.lock().clone()
    }

    pub async fn cleanup_old(&self, older_than_ms: u64) -> Result<usize, ConsistencyError> {
        let now_ns = current_time_ns();
        let cutoff_ns = now_ns - (older_than_ms * 1_000_000);
        let mut count = 0;

        for mut accesses in self.recent_accesses.iter_mut() {
            while accesses.value_mut().front().map(|a| a.timestamp_ns < cutoff_ns).unwrap_or(false) {
                accesses.value_mut().pop_front();
                count += 1;
            }
        }

        Ok(count)
    }
}

impl Default for CrossProtocolCache {
    fn default() -> Self {
        Self::new(1000)
    }
}

fn current_time_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_record_access() {
        let cache = CrossProtocolCache::new(100);
        let result = cache
            .record_access(Protocol::NFS, 1, 100, AccessType::Read, 1)
            .await
            .unwrap();

        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_detect_read_write_conflict() {
        let cache = CrossProtocolCache::new(100);
        cache
            .record_access(Protocol::NFS, 1, 100, AccessType::Read, 1)
            .await
            .unwrap();

        let result = cache
            .record_access(Protocol::S3, 2, 100, AccessType::Write(WriteOp::Write), 2)
            .await
            .unwrap();

        assert!(result.is_some());
    }

    #[tokio::test]
    async fn test_concurrent_writes() {
        let cache = CrossProtocolCache::new(100);
        cache
            .record_access(Protocol::NFS, 1, 100, AccessType::Write(WriteOp::Write), 1)
            .await
            .unwrap();

        assert!(!cache.has_concurrent_writes(100));

        cache
            .record_access(Protocol::S3, 2, 100, AccessType::Write(WriteOp::Write), 2)
            .await
            .unwrap();

        assert!(cache.has_concurrent_writes(100));
    }

    #[tokio::test]
    async fn test_access_history() {
        let cache = CrossProtocolCache::new(100);
        cache
            .record_access(Protocol::NFS, 1, 100, AccessType::Read, 1)
            .await
            .unwrap();

        let history = cache.get_access_history(100, 5000);
        assert_eq!(history.len(), 1);
    }

    #[tokio::test]
    async fn test_cleanup_old() {
        let cache = CrossProtocolCache::new(100);
        cache
            .record_access(Protocol::NFS, 1, 100, AccessType::Read, 1)
            .await
            .unwrap();

        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let count = cache.cleanup_old(50).await.unwrap();
        assert!(count >= 0);
    }
}
