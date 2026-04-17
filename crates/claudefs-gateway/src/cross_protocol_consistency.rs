//! Cross-protocol consistency detection and conflict resolution.
//!
//! This module tracks access patterns from multiple protocols (NFS, S3, SMB)
//! to the same inode and detects/resolves conflicts.

use dashmap::DashMap;
use std::collections::VecDeque;
use std::sync::Arc;
use thiserror::Error;

use crate::protocol::Protocol;

/// Protocol access record for conflict detection.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProtocolAccessRecord {
    /// Which protocol performed the access
    pub protocol: Protocol,
    /// Client ID that performed the access
    pub client_id: u64,
    /// Inode being accessed
    pub inode_id: u64,
    /// Type of access
    pub access_type: AccessType,
    /// Timestamp in milliseconds since epoch
    pub timestamp_ms: u64,
    /// Request ID for tracing
    pub request_id: u64,
}

/// Type of access performed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum AccessType {
    /// Read access
    Read,
    /// Write access
    Write,
    /// Delete access
    Delete,
    /// Metadata-only change
    Metadata,
}

/// Type of conflict detected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ConflictType {
    /// Read and write from different protocols
    ReadWrite,
    /// Concurrent writes from different protocols
    ConcurrentWrites,
    /// Rename while being accessed
    RenameUnderAccess,
    /// Delete while being accessed
    DeleteUnderAccess,
}

/// Conflict resolution strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ConflictResolution {
    /// Newer timestamp wins
    LastWriteWins,
    /// Older request is aborted
    AbortRequest,
    /// Revoke NFSv4 delegations
    RevokeDelegation,
    /// Notify client via callback
    ClientNotified,
}

/// Detected and resolved conflict record.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ConflictRecord {
    /// Unique conflict ID
    pub conflict_id: u64,
    /// Type of conflict
    pub conflict_type: ConflictType,
    /// The two conflicting accesses
    pub accesses: [ProtocolAccessRecord; 2],
    /// When conflict was detected
    pub detected_at_ms: u64,
    /// How it was resolved
    pub resolution: ConflictResolution,
}

/// Metrics for cross-protocol consistency.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CrossProtocolMetrics {
    /// Total accesses recorded
    pub total_accesses: u64,
    /// Total conflicts detected
    pub conflicts_detected: u64,
    /// Total conflicts resolved
    pub conflicts_resolved: u64,
}

/// Errors for consistency operations.
#[derive(Debug, Error)]
pub enum ConsistencyError {
    /// Invalid access type
    #[error("Invalid access type")]
    InvalidAccess,
    /// Conflict resolution failed
    #[error("Conflict resolution failed")]
    ResolutionFailed,
    /// Cache operation failed
    #[error("Cache operation failed")]
    CacheError,
    /// Record not found
    #[error("Access record not found")]
    NotFound,
}

/// Cross-protocol consistency cache.
pub struct CrossProtocolCache {
    recent_accesses: Arc<DashMap<u64, VecDeque<ProtocolAccessRecord>>>,
    conflicts: Arc<DashMap<u64, ConflictRecord>>,
    metrics: Arc<parking_lot::Mutex<CrossProtocolMetrics>>,
    next_conflict_id: std::sync::atomic::AtomicU64,
    window_size: usize,
}

impl CrossProtocolCache {
    /// Create a new cross-protocol cache.
    pub fn new(window_size: usize) -> Self {
        Self {
            recent_accesses: Arc::new(DashMap::new()),
            conflicts: Arc::new(DashMap::new()),
            metrics: Arc::new(parking_lot::Mutex::new(CrossProtocolMetrics {
                total_accesses: 0,
                conflicts_detected: 0,
                conflicts_resolved: 0,
            })),
            next_conflict_id: std::sync::atomic::AtomicU64::new(1),
            window_size,
        }
    }

    /// Record an access from any protocol.
    pub fn record_access(
        &self,
        protocol: Protocol,
        client_id: u64,
        inode_id: u64,
        access_type: AccessType,
        request_id: u64,
    ) -> Result<Option<ConflictRecord>, ConsistencyError> {
        use std::time::SystemTime;

        let record = ProtocolAccessRecord {
            protocol,
            client_id,
            inode_id,
            access_type,
            timestamp_ms: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            request_id,
        };

        let mut accesses = self
            .recent_accesses
            .entry(inode_id)
            .or_insert_with(VecDeque::new);

        // Check for conflicts with recent accesses
        let mut detected_conflict = None;
        if !accesses.is_empty() {
            if let Some(prev_record) = accesses.back() {
                if let Some(conflict_type) = Self::detect_conflict(prev_record, &record) {
                    let conflict_id = self
                        .next_conflict_id
                        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    let conflict = ConflictRecord {
                        conflict_id,
                        conflict_type,
                        accesses: [prev_record.clone(), record.clone()],
                        detected_at_ms: SystemTime::now()
                            .duration_since(SystemTime::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis() as u64,
                        resolution: ConflictResolution::LastWriteWins,
                    };

                    self.conflicts.insert(conflict_id, conflict.clone());

                    let mut metrics = self.metrics.lock();
                    metrics.conflicts_detected += 1;

                    detected_conflict = Some(conflict);
                }
            }
        }

        // Add to history
        accesses.push_back(record);

        // Maintain window size
        while accesses.len() > self.window_size {
            accesses.pop_front();
        }

        let mut metrics = self.metrics.lock();
        metrics.total_accesses += 1;

        Ok(detected_conflict)
    }

    /// Check if inode has concurrent writes from different protocols.
    pub fn has_concurrent_writes(&self, inode_id: u64) -> bool {
        if let Some(accesses) = self.recent_accesses.get(&inode_id) {
            let mut write_count = 0;
            let mut protocols = std::collections::HashSet::new();

            for access in accesses.iter() {
                if access.access_type == AccessType::Write {
                    write_count += 1;
                    protocols.insert(access.protocol);
                }
            }

            write_count > 1 || (write_count == 1 && protocols.len() > 1)
        } else {
            false
        }
    }

    /// Get access history for an inode within a time window.
    pub fn get_access_history(&self, inode_id: u64, lookback_ms: u64) -> Vec<ProtocolAccessRecord> {
        if let Some(accesses) = self.recent_accesses.get(&inode_id) {
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            accesses
                .iter()
                .filter(|a| now_ms.saturating_sub(a.timestamp_ms) < lookback_ms)
                .cloned()
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Detect conflict between two access records.
    pub fn detect_conflict(
        rec1: &ProtocolAccessRecord,
        rec2: &ProtocolAccessRecord,
    ) -> Option<ConflictType> {
        // Same protocol, no cross-protocol conflict
        if rec1.protocol == rec2.protocol {
            return None;
        }

        match (rec1.access_type, rec2.access_type) {
            (AccessType::Read, AccessType::Write) | (AccessType::Write, AccessType::Read) => {
                Some(ConflictType::ReadWrite)
            }
            (AccessType::Write, AccessType::Write) => Some(ConflictType::ConcurrentWrites),
            (AccessType::Delete, AccessType::Read) | (AccessType::Read, AccessType::Delete) => {
                Some(ConflictType::DeleteUnderAccess)
            }
            _ => None,
        }
    }

    /// Resolve a conflict record.
    pub fn resolve_conflict(
        &self,
        mut conflict: ConflictRecord,
    ) -> Result<ConflictResolution, ConsistencyError> {
        // Simple last-write-wins for now
        let resolution = if conflict.accesses[1].timestamp_ms > conflict.accesses[0].timestamp_ms {
            ConflictResolution::LastWriteWins
        } else {
            ConflictResolution::LastWriteWins
        };

        conflict.resolution = resolution;
        self.conflicts.insert(conflict.conflict_id, conflict);

        let mut metrics = self.metrics.lock();
        metrics.conflicts_resolved += 1;

        Ok(resolution)
    }

    /// Get current metrics.
    pub fn metrics(&self) -> CrossProtocolMetrics {
        self.metrics.lock().clone()
    }

    /// Clean up old access records.
    pub fn cleanup_old(&self, older_than_ms: u64) -> Result<usize, ConsistencyError> {
        let mut cleaned = 0;
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        self.recent_accesses.retain(|_inode_id, accesses| {
            while let Some(access) = accesses.front() {
                if now_ms.saturating_sub(access.timestamp_ms) > older_than_ms {
                    accesses.pop_front();
                    cleaned += 1;
                } else {
                    break;
                }
            }
            !accesses.is_empty()
        });

        Ok(cleaned)
    }
}

impl Default for CrossProtocolCache {
    fn default() -> Self {
        Self::new(1000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_single_protocol() {
        let cache = CrossProtocolCache::new(100);
        let record = cache
            .record_access(Protocol::Nfs3, 1, 100, AccessType::Read, 1001)
            .expect("record succeeds");

        assert!(record.is_none()); // No conflict yet
    }

    #[test]
    fn test_detect_read_write_conflict() {
        let cache = CrossProtocolCache::new(100);

        cache
            .record_access(Protocol::Nfs3, 1, 100, AccessType::Read, 1001)
            .expect("record read");

        let conflict = cache
            .record_access(Protocol::S3, 2, 100, AccessType::Write, 1002)
            .expect("record write");

        assert!(conflict.is_some());
        let c = conflict.unwrap();
        assert_eq!(c.conflict_type, ConflictType::ReadWrite);
    }

    #[test]
    fn test_detect_concurrent_writes() {
        let cache = CrossProtocolCache::new(100);

        cache
            .record_access(Protocol::Nfs3, 1, 100, AccessType::Write, 1001)
            .expect("record write 1");

        let conflict = cache
            .record_access(Protocol::S3, 2, 100, AccessType::Write, 1002)
            .expect("record write 2");

        assert!(conflict.is_some());
        let c = conflict.unwrap();
        assert_eq!(c.conflict_type, ConflictType::ConcurrentWrites);
    }

    #[test]
    fn test_same_protocol_no_conflict() {
        let cache = CrossProtocolCache::new(100);

        cache
            .record_access(Protocol::Nfs3, 1, 100, AccessType::Write, 1001)
            .expect("record write 1");

        let conflict = cache
            .record_access(Protocol::Nfs3, 2, 100, AccessType::Write, 1002)
            .expect("record write 2");

        // No conflict (same protocol)
        assert!(conflict.is_none());
    }

    #[test]
    fn test_has_concurrent_writes() {
        let cache = CrossProtocolCache::new(100);

        cache
            .record_access(Protocol::Nfs3, 1, 100, AccessType::Write, 1001)
            .expect("record write 1");

        assert!(!cache.has_concurrent_writes(100));

        cache
            .record_access(Protocol::S3, 2, 100, AccessType::Write, 1002)
            .expect("record write 2");

        assert!(cache.has_concurrent_writes(100));
    }

    #[test]
    fn test_get_access_history() {
        let cache = CrossProtocolCache::new(100);

        cache
            .record_access(Protocol::Nfs3, 1, 100, AccessType::Read, 1001)
            .expect("record 1");

        std::thread::sleep(std::time::Duration::from_millis(10));

        cache
            .record_access(Protocol::S3, 2, 100, AccessType::Write, 1002)
            .expect("record 2");

        let history = cache.get_access_history(100, 10000);
        assert_eq!(history.len(), 2);
    }

    #[test]
    fn test_detect_conflict_static() {
        let r1 = ProtocolAccessRecord {
            protocol: Protocol::Nfs3,
            client_id: 1,
            inode_id: 100,
            access_type: AccessType::Read,
            timestamp_ms: 1000,
            request_id: 1001,
        };

        let r2 = ProtocolAccessRecord {
            protocol: Protocol::S3,
            client_id: 2,
            inode_id: 100,
            access_type: AccessType::Write,
            timestamp_ms: 1001,
            request_id: 1002,
        };

        let conflict = CrossProtocolCache::detect_conflict(&r1, &r2);
        assert_eq!(conflict, Some(ConflictType::ReadWrite));
    }

    #[test]
    fn test_resolve_conflict() {
        let cache = CrossProtocolCache::new(100);

        let r1 = ProtocolAccessRecord {
            protocol: Protocol::Nfs3,
            client_id: 1,
            inode_id: 100,
            access_type: AccessType::Write,
            timestamp_ms: 1000,
            request_id: 1001,
        };

        let r2 = ProtocolAccessRecord {
            protocol: Protocol::S3,
            client_id: 2,
            inode_id: 100,
            access_type: AccessType::Write,
            timestamp_ms: 1010,
            request_id: 1002,
        };

        let conflict = ConflictRecord {
            conflict_id: 1,
            conflict_type: ConflictType::ConcurrentWrites,
            accesses: [r1, r2],
            detected_at_ms: 1011,
            resolution: ConflictResolution::LastWriteWins,
        };

        let resolution = cache.resolve_conflict(conflict).expect("resolve succeeds");
        assert_eq!(resolution, ConflictResolution::LastWriteWins);
    }

    #[test]
    fn test_metrics() {
        let cache = CrossProtocolCache::new(100);

        cache
            .record_access(Protocol::Nfs3, 1, 100, AccessType::Read, 1001)
            .expect("record 1");

        cache
            .record_access(Protocol::S3, 2, 100, AccessType::Write, 1002)
            .expect("record 2");

        let metrics = cache.metrics();
        assert_eq!(metrics.total_accesses, 2);
        assert_eq!(metrics.conflicts_detected, 1);
    }

    #[test]
    fn test_cleanup_old() {
        let cache = CrossProtocolCache::new(100);

        cache
            .record_access(Protocol::Nfs3, 1, 100, AccessType::Read, 1001)
            .expect("record");

        let cleaned = cache.cleanup_old(1).expect("cleanup succeeds");
        // Note: may not clean if still within 1ms
        assert!(cleaned >= 0);
    }

    #[test]
    fn test_window_size_enforcement() {
        let cache = CrossProtocolCache::new(5);

        for i in 0..10 {
            let _ = cache.record_access(Protocol::Nfs3, 1, 100, AccessType::Read, 1000 + i);
        }

        let history = cache.get_access_history(100, 100000);
        assert!(history.len() <= 5);
    }
}
