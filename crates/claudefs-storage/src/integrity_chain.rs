//! End-to-end data integrity verification for ClaudeFS storage.
//!
//! This module provides integrity chain tracking through the I/O pipeline,
//! allowing verification of data at various stages from client write to
//! S3 tiering.

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;
use uuid::Uuid;

use crate::error::{StorageError, StorageResult};

/// Integrity algorithm used for checksum computation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IntegrityAlgorithm {
    /// CRC-32 checksum (IEEE polynomial).
    Crc32,
    /// CRC-64 checksum (ISO polynomial).
    Crc64,
    /// BLAKE3 cryptographic hash.
    Blake3,
    /// xxHash64 fast non-cryptographic hash.
    Xxhash64,
}

/// Pipeline stage where integrity verification occurs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PipelineStage {
    /// Initial client write operation.
    ClientWrite,
    /// Deduplication stage.
    Dedup,
    /// Compression stage.
    Compress,
    /// Encryption stage.
    Encrypt,
    /// Segment packing stage.
    SegmentPack,
    /// Erasure coding encoding stage.
    EcEncode,
    /// Local NVMe storage stage.
    LocalStore,
    /// Cross-site replication stage.
    Replicate,
    /// Tiering to S3 object storage.
    TierToS3,
    /// Read-back verification stage.
    ReadBack,
}

/// A single verification point in the integrity chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationPoint {
    /// The pipeline stage this point represents.
    pub stage: PipelineStage,
    /// Hex string representation of the checksum.
    pub checksum: String,
    /// Algorithm used for this verification point.
    pub algorithm: IntegrityAlgorithm,
    /// Unix timestamp in milliseconds when this point was created.
    pub timestamp: u64,
    /// Length of data in bytes at this point.
    pub data_length: u64,
    /// Whether this point has been verified.
    pub verified: bool,
}

/// The complete integrity chain tracking data through the pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntegrityChain {
    /// Unique identifier for this chain.
    pub id: String,
    /// Identifier for the data being tracked.
    pub data_id: String,
    /// Unix timestamp in milliseconds when this chain was created.
    pub created_at: u64,
    /// Unix timestamp in milliseconds when this chain expires (for GC).
    pub expires_at: u64,
    /// Verification points in order of pipeline stages.
    pub points: Vec<VerificationPoint>,
}

/// Result of an integrity verification operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VerificationResult {
    /// Data successfully verified at the specified point.
    Valid {
        /// The chain ID that was verified.
        chain_id: String,
        /// Index of the verification point.
        point_index: usize,
    },
    /// Data verification failed due to checksum mismatch.
    Invalid {
        /// The chain ID that failed verification.
        chain_id: String,
        /// Index of the verification point.
        point_index: usize,
        /// Reason for the failure.
        reason: String,
    },
    /// Requested verification point does not exist in chain.
    MissingPoint {
        /// The chain ID.
        chain_id: String,
        /// The missing pipeline stage.
        stage: PipelineStage,
    },
    /// Chain has expired and was garbage collected.
    ChainExpired {
        /// The expired chain ID.
        chain_id: String,
    },
    /// Chain does not exist in the manager.
    ChainNotFound {
        /// The missing chain ID.
        chain_id: String,
    },
}

/// Configuration for integrity verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntegrityConfig {
    /// Default algorithm to use for new chains.
    pub default_algorithm: IntegrityAlgorithm,
    /// Time-to-live for chains before garbage collection (seconds).
    pub chain_ttl_seconds: u64,
    /// Whether to verify data on read operations.
    pub verify_on_read: bool,
    /// Whether to verify data on write operations.
    pub verify_on_write: bool,
    /// Whether to emit alerts on verification failures.
    pub alert_on_failure: bool,
}

impl Default for IntegrityConfig {
    fn default() -> Self {
        Self {
            default_algorithm: IntegrityAlgorithm::Crc32,
            chain_ttl_seconds: 86400, // 24 hours
            verify_on_read: true,
            verify_on_write: true,
            alert_on_failure: true,
        }
    }
}

/// Statistics for integrity verification operations.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IntegrityStats {
    /// Total number of active integrity chains.
    pub total_chains: u64,
    /// Total number of verification operations performed.
    pub total_verifications: u64,
    /// Number of successful verifications.
    pub successful_verifications: u64,
    /// Number of failed verifications.
    pub failed_verifications: u64,
    /// Number of chains removed by garbage collection.
    pub chains_removed: u64,
    /// Unix timestamp in milliseconds of the last GC run.
    pub last_gc_run: u64,
}

/// Error types specific to integrity verification.
#[derive(Debug, Error, Serialize, Deserialize)]
pub enum IntegrityError {
    /// The requested algorithm is not supported.
    #[error("Algorithm not supported: {0}")]
    AlgorithmNotSupported(String),
    /// Checksum mismatch between expected and actual values.
    #[error("Checksum mismatch: expected {expected}, actual {actual}")]
    ChecksumMismatch {
        /// Expected checksum value.
        expected: String,
        /// Actual checksum value.
        actual: String,
    },
    /// The requested chain does not exist.
    #[error("Chain not found: {0}")]
    ChainNotFound(String),
    /// Invalid data provided for operation.
    #[error("Invalid data: {0}")]
    InvalidData(String),
    /// I/O error during operation.
    #[error("I/O error: {0}")]
    IoError(String),
}

impl From<IntegrityError> for StorageError {
    fn from(e: IntegrityError) -> Self {
        match e {
            IntegrityError::ChainNotFound(id) => {
                StorageError::AllocatorError(format!("Chain not found: {}", id))
            }
            IntegrityError::IoError(msg) => {
                StorageError::IoError(std::io::Error::new(std::io::ErrorKind::Other, msg))
            }
            IntegrityError::InvalidData(msg) => StorageError::SerializationError { reason: msg },
            IntegrityError::AlgorithmNotSupported(msg) => StorageError::AllocatorError(msg),
            IntegrityError::ChecksumMismatch { .. } => {
                StorageError::AllocatorError("Checksum mismatch".to_string())
            }
        }
    }
}

/// Manages integrity chains for data verification throughout the I/O pipeline.
pub struct IntegrityManager {
    config: IntegrityConfig,
    chains: HashMap<String, IntegrityChain>,
    stage_index: HashMap<PipelineStage, Vec<String>>,
    stats: Mutex<IntegrityStats>,
}

impl IntegrityManager {
    /// Creates a new IntegrityManager with the given configuration.
    pub fn new(config: IntegrityConfig) -> Self {
        Self {
            config,
            chains: HashMap::new(),
            stage_index: HashMap::new(),
            stats: Mutex::new(IntegrityStats::default()),
        }
    }

    /// Creates a new integrity chain for tracking data through the pipeline.
    pub fn create_chain(
        &mut self,
        data_id: String,
        ttl_seconds: Option<u64>,
    ) -> StorageResult<IntegrityChain> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| StorageError::AllocatorError(e.to_string()))?
            .as_millis() as u64;

        let ttl = ttl_seconds.unwrap_or(self.config.chain_ttl_seconds);
        let expires_at = now + ttl * 1000;

        let chain = IntegrityChain {
            id: Uuid::new_v4().to_string(),
            data_id,
            created_at: now,
            expires_at,
            points: Vec::new(),
        };

        let chain_id = chain.id.clone();
        self.chains.insert(chain_id, chain.clone());

        let mut stats = self.stats.lock();
        stats.total_chains += 1;

        Ok(chain)
    }

    /// Adds a verification point to an existing chain.
    pub fn add_point(&mut self, chain_id: &str, point: VerificationPoint) -> StorageResult<()> {
        let chain = self.chains.get_mut(chain_id).ok_or_else(|| {
            StorageError::AllocatorError(format!("Chain not found: {}", chain_id))
        })?;

        chain.points.push(point.clone());

        self.stage_index
            .entry(point.stage)
            .or_default()
            .push(chain_id.to_string());

        Ok(())
    }

    /// Verifies data at a specific pipeline stage.
    pub fn verify_point(
        &self,
        chain_id: &str,
        stage: PipelineStage,
        data: &[u8],
    ) -> StorageResult<VerificationResult> {
        let chain = self.chains.get(chain_id);

        let chain = match chain {
            Some(c) => {
                if c.expires_at
                    < std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_err(|e| StorageError::AllocatorError(e.to_string()))?
                        .as_millis() as u64
                {
                    return Ok(VerificationResult::ChainExpired {
                        chain_id: chain_id.to_string(),
                    });
                }
                c
            }
            None => {
                return Ok(VerificationResult::ChainNotFound {
                    chain_id: chain_id.to_string(),
                });
            }
        };

        let point_index = chain.points.iter().position(|p| p.stage == stage);

        let point_index = match point_index {
            Some(idx) => idx,
            None => {
                return Ok(VerificationResult::MissingPoint {
                    chain_id: chain_id.to_string(),
                    stage,
                });
            }
        };

        let point = &chain.points[point_index];
        let computed = self.compute_checksum(data, &point.algorithm)?;

        let mut stats = self.stats.lock();
        stats.total_verifications += 1;

        if computed == point.checksum {
            stats.successful_verifications += 1;
            Ok(VerificationResult::Valid {
                chain_id: chain_id.to_string(),
                point_index,
            })
        } else {
            stats.failed_verifications += 1;
            Ok(VerificationResult::Invalid {
                chain_id: chain_id.to_string(),
                point_index,
                reason: format!(
                    "Checksum mismatch: expected {}, got {}",
                    point.checksum, computed
                ),
            })
        }
    }

    /// Verifies all points in a chain using provided data.
    pub fn verify_chain(
        &self,
        chain_id: &str,
        data_map: &HashMap<PipelineStage, Vec<u8>>,
    ) -> Vec<VerificationResult> {
        let chain = match self.chains.get(chain_id) {
            Some(c) => c,
            None => {
                return vec![VerificationResult::ChainNotFound {
                    chain_id: chain_id.to_string(),
                }];
            }
        };

        if chain.expires_at
            < std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0)
        {
            return vec![VerificationResult::ChainExpired {
                chain_id: chain_id.to_string(),
            }];
        }

        let mut results = Vec::new();

        for point in &chain.points {
            if let Some(data) = data_map.get(&point.stage) {
                let computed = match self.compute_checksum(data, &point.algorithm) {
                    Ok(c) => c,
                    Err(e) => {
                        results.push(VerificationResult::Invalid {
                            chain_id: chain_id.to_string(),
                            point_index: 0,
                            reason: e.to_string(),
                        });
                        continue;
                    }
                };

                let mut stats = self.stats.lock();
                stats.total_verifications += 1;

                if computed == point.checksum {
                    stats.successful_verifications += 1;
                    results.push(VerificationResult::Valid {
                        chain_id: chain_id.to_string(),
                        point_index: results.len(),
                    });
                } else {
                    stats.failed_verifications += 1;
                    results.push(VerificationResult::Invalid {
                        chain_id: chain_id.to_string(),
                        point_index: results.len(),
                        reason: format!("Checksum mismatch at stage {:?}", point.stage),
                    });
                }
            } else {
                results.push(VerificationResult::MissingPoint {
                    chain_id: chain_id.to_string(),
                    stage: point.stage,
                });
            }
        }

        results
    }

    /// Retrieves an integrity chain by ID.
    pub fn get_chain(&self, chain_id: &str) -> Option<IntegrityChain> {
        self.chains.get(chain_id).cloned()
    }

    /// Removes an integrity chain.
    pub fn remove_chain(&mut self, chain_id: &str) -> StorageResult<()> {
        if let Some(chain) = self.chains.remove(chain_id) {
            for point in &chain.points {
                if let Some(ids) = self.stage_index.get_mut(&point.stage) {
                    ids.retain(|id| id != chain_id);
                }
            }
            Ok(())
        } else {
            Err(StorageError::AllocatorError(format!(
                "Chain not found: {}",
                chain_id
            )))
        }
    }

    /// Performs garbage collection of expired chains.
    pub fn gc_expired_chains(&mut self) -> StorageResult<u64> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| StorageError::AllocatorError(e.to_string()))?
            .as_millis() as u64;

        let mut removed_count = 0;
        let expired_ids: Vec<String> = self
            .chains
            .iter()
            .filter(|(_, chain)| chain.expires_at < now)
            .map(|(id, _)| id.clone())
            .collect();

        for chain_id in expired_ids {
            if let Some(chain) = self.chains.remove(&chain_id) {
                for point in &chain.points {
                    if let Some(ids) = self.stage_index.get_mut(&point.stage) {
                        ids.retain(|id| id != &chain_id);
                    }
                }
                removed_count += 1;
            }
        }

        let mut stats = self.stats.lock();
        stats.chains_removed += removed_count;
        stats.total_chains = stats.total_chains.saturating_sub(removed_count);
        stats.last_gc_run = now;

        Ok(removed_count)
    }

    /// Returns the number of active chains.
    pub fn chain_count(&self) -> usize {
        self.chains.len()
    }

    /// Returns chain IDs for chains containing a specific stage.
    pub fn chains_for_stage(&self, stage: PipelineStage) -> Vec<String> {
        self.stage_index.get(&stage).cloned().unwrap_or_default()
    }

    /// Returns current integrity verification statistics.
    pub fn stats(&self) -> IntegrityStats {
        self.stats.lock().clone()
    }

    /// Computes checksum for data using the specified algorithm.
    pub fn compute_checksum(
        &self,
        data: &[u8],
        algorithm: &IntegrityAlgorithm,
    ) -> StorageResult<String> {
        match algorithm {
            IntegrityAlgorithm::Crc32 => {
                let hash = crc32fast::hash(data);
                Ok(format!("{:08x}", hash))
            }
            IntegrityAlgorithm::Crc64 => {
                use std::io::Write;
                let mut digest = crc64fast::Digest::new();
                digest
                    .write_all(data)
                    .map_err(|e| StorageError::IoError(e.to_string()))?;
                let hash = digest.sum64();
                Ok(format!("{:016x}", hash))
            }
            IntegrityAlgorithm::Blake3 => {
                let hash = blake3::hash(data);
                Ok(hash.to_hex().to_string())
            }
            IntegrityAlgorithm::Xxhash64 => {
                use xxhash_rust::xxh3::xxh3_64;
                let hash = xxh3_64(data);
                Ok(format!("{:016x}", hash))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_integrity_algorithm_serialization() {
        let algo = IntegrityAlgorithm::Crc32;
        let serialized = serde_json::to_string(&algo).unwrap();
        assert!(serialized.contains("Crc32"));

        let deserialized: IntegrityAlgorithm = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized, IntegrityAlgorithm::Crc32);

        for algo_variant in [
            IntegrityAlgorithm::Crc32,
            IntegrityAlgorithm::Crc64,
            IntegrityAlgorithm::Blake3,
            IntegrityAlgorithm::Xxhash64,
        ] {
            let ser = serde_json::to_string(&algo_variant).unwrap();
            let de: IntegrityAlgorithm = serde_json::from_str(&ser).unwrap();
            assert_eq!(algo_variant, de);
        }
    }

    #[test]
    fn test_pipeline_stage_serialization() {
        let stage = PipelineStage::ClientWrite;
        let serialized = serde_json::to_string(&stage).unwrap();
        assert!(serialized.contains("ClientWrite"));

        let deserialized: PipelineStage = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized, PipelineStage::ClientWrite);

        for stage_variant in [
            PipelineStage::ClientWrite,
            PipelineStage::Dedup,
            PipelineStage::Compress,
            PipelineStage::Encrypt,
            PipelineStage::SegmentPack,
            PipelineStage::EcEncode,
            PipelineStage::LocalStore,
            PipelineStage::Replicate,
            PipelineStage::TierToS3,
            PipelineStage::ReadBack,
        ] {
            let ser = serde_json::to_string(&stage_variant).unwrap();
            let de: PipelineStage = serde_json::from_str(&ser).unwrap();
            assert_eq!(stage_variant, de);
        }
    }

    #[test]
    fn test_verification_point_creation() {
        let point = VerificationPoint {
            stage: PipelineStage::ClientWrite,
            checksum: "abcd1234".to_string(),
            algorithm: IntegrityAlgorithm::Crc32,
            timestamp: 1234567890,
            data_length: 4096,
            verified: false,
        };

        assert_eq!(point.stage, PipelineStage::ClientWrite);
        assert_eq!(point.checksum, "abcd1234");
        assert_eq!(point.data_length, 4096);
    }

    #[test]
    fn test_verification_point_serialization() {
        let point = VerificationPoint {
            stage: PipelineStage::Compress,
            checksum: "deadbeef".to_string(),
            algorithm: IntegrityAlgorithm::Blake3,
            timestamp: 9999999999,
            data_length: 8192,
            verified: true,
        };

        let serialized = serde_json::to_string(&point).unwrap();
        let deserialized: VerificationPoint = serde_json::from_str(&serialized).unwrap();

        assert_eq!(deserialized.stage, PipelineStage::Compress);
        assert_eq!(deserialized.checksum, "deadbeef");
        assert_eq!(deserialized.verified, true);
    }

    #[test]
    fn test_integrity_chain_creation() {
        let chain = IntegrityChain {
            id: "test-id-123".to_string(),
            data_id: "data-456".to_string(),
            created_at: 1000000,
            expires_at: 2000000,
            points: Vec::new(),
        };

        assert_eq!(chain.id, "test-id-123");
        assert_eq!(chain.data_id, "data-456");
        assert_eq!(chain.points.len(), 0);
    }

    #[test]
    fn test_integrity_chain_with_points() {
        let chain = IntegrityChain {
            id: "chain-789".to_string(),
            data_id: "data-abc".to_string(),
            created_at: 500000,
            expires_at: 1500000,
            points: vec![
                VerificationPoint {
                    stage: PipelineStage::ClientWrite,
                    checksum: "1111".to_string(),
                    algorithm: IntegrityAlgorithm::Crc32,
                    timestamp: 500000,
                    data_length: 1024,
                    verified: true,
                },
                VerificationPoint {
                    stage: PipelineStage::Compress,
                    checksum: "2222".to_string(),
                    algorithm: IntegrityAlgorithm::Crc32,
                    timestamp: 600000,
                    data_length: 512,
                    verified: false,
                },
            ],
        };

        assert_eq!(chain.points.len(), 2);
        assert_eq!(chain.points[0].stage, PipelineStage::ClientWrite);
        assert_eq!(chain.points[1].stage, PipelineStage::Compress);
    }

    #[test]
    fn test_verification_result_variants() {
        let valid = VerificationResult::Valid {
            chain_id: "chain1".to_string(),
            point_index: 0,
        };
        assert!(matches!(valid, VerificationResult::Valid { .. }));

        let invalid = VerificationResult::Invalid {
            chain_id: "chain1".to_string(),
            point_index: 1,
            reason: "mismatch".to_string(),
        };
        assert!(matches!(invalid, VerificationResult::Invalid { .. }));

        let missing = VerificationResult::MissingPoint {
            chain_id: "chain1".to_string(),
            stage: PipelineStage::Encrypt,
        };
        assert!(matches!(missing, VerificationResult::MissingPoint { .. }));

        let expired = VerificationResult::ChainExpired {
            chain_id: "chain1".to_string(),
        };
        assert!(matches!(expired, VerificationResult::ChainExpired { .. }));

        let not_found = VerificationResult::ChainNotFound {
            chain_id: "chain1".to_string(),
        };
        assert!(matches!(
            not_found,
            VerificationResult::ChainNotFound { .. }
        ));
    }

    #[test]
    fn test_verification_result_serialization() {
        let results = vec![
            VerificationResult::Valid {
                chain_id: "c1".to_string(),
                point_index: 0,
            },
            VerificationResult::Invalid {
                chain_id: "c2".to_string(),
                point_index: 1,
                reason: "checksum mismatch".to_string(),
            },
        ];

        let serialized = serde_json::to_string(&results).unwrap();
        let deserialized: Vec<VerificationResult> = serde_json::from_str(&serialized).unwrap();

        assert_eq!(deserialized.len(), 2);
    }

    #[test]
    fn test_integrity_config_defaults() {
        let config = IntegrityConfig::default();

        assert_eq!(config.default_algorithm, IntegrityAlgorithm::Crc32);
        assert_eq!(config.chain_ttl_seconds, 86400);
        assert_eq!(config.verify_on_read, true);
        assert_eq!(config.verify_on_write, true);
        assert_eq!(config.alert_on_failure, true);
    }

    #[test]
    fn test_integrity_config_serialization() {
        let config = IntegrityConfig {
            default_algorithm: IntegrityAlgorithm::Blake3,
            chain_ttl_seconds: 3600,
            verify_on_read: false,
            verify_on_write: true,
            alert_on_failure: false,
        };

        let serialized = serde_json::to_string(&config).unwrap();
        let deserialized: IntegrityConfig = serde_json::from_str(&serialized).unwrap();

        assert_eq!(deserialized.default_algorithm, IntegrityAlgorithm::Blake3);
        assert_eq!(deserialized.chain_ttl_seconds, 3600);
    }

    #[test]
    fn test_integrity_stats_default() {
        let stats = IntegrityStats::default();

        assert_eq!(stats.total_chains, 0);
        assert_eq!(stats.total_verifications, 0);
        assert_eq!(stats.successful_verifications, 0);
        assert_eq!(stats.failed_verifications, 0);
        assert_eq!(stats.chains_removed, 0);
        assert_eq!(stats.last_gc_run, 0);
    }

    #[test]
    fn test_integrity_stats_serialization() {
        let stats = IntegrityStats {
            total_chains: 10,
            total_verifications: 100,
            successful_verifications: 95,
            failed_verifications: 5,
            chains_removed: 2,
            last_gc_run: 9999999999,
        };

        let serialized = serde_json::to_string(&stats).unwrap();
        let deserialized: IntegrityStats = serde_json::from_str(&serialized).unwrap();

        assert_eq!(deserialized.total_chains, 10);
        assert_eq!(deserialized.total_verifications, 100);
    }

    #[test]
    fn test_integrity_error_variants() {
        let algo_err = IntegrityError::AlgorithmNotSupported("Unknown".to_string());
        assert!(matches!(algo_err, IntegrityError::AlgorithmNotSupported(_)));

        let checksum_err = IntegrityError::ChecksumMismatch {
            expected: "abc".to_string(),
            actual: "def".to_string(),
        };
        assert!(matches!(
            checksum_err,
            IntegrityError::ChecksumMismatch { .. }
        ));

        let chain_err = IntegrityError::ChainNotFound("missing".to_string());
        assert!(matches!(chain_err, IntegrityError::ChainNotFound(_)));

        let data_err = IntegrityError::InvalidData("bad input".to_string());
        assert!(matches!(data_err, IntegrityError::InvalidData(_)));

        let io_err = IntegrityError::IoError("disk error".to_string());
        assert!(matches!(io_err, IntegrityError::IoError(_)));
    }

    #[test]
    fn test_integrity_error_serialization() {
        let err = IntegrityError::ChecksumMismatch {
            expected: "expected_value".to_string(),
            actual: "actual_value".to_string(),
        };

        let serialized = serde_json::to_string(&err).unwrap();
        let deserialized: IntegrityError = serde_json::from_str(&serialized).unwrap();

        assert!(matches!(
            deserialized,
            IntegrityError::ChecksumMismatch { .. }
        ));
    }

    #[test]
    fn test_integrity_manager_new() {
        let config = IntegrityConfig::default();
        let manager = IntegrityManager::new(config);

        assert_eq!(manager.chain_count(), 0);
        assert_eq!(manager.stats().total_chains, 0);
    }

    #[test]
    fn test_integrity_manager_create_chain() {
        let config = IntegrityConfig::default();
        let mut manager = IntegrityManager::new(config);

        let chain = manager.create_chain("data-123".to_string(), None).unwrap();

        assert!(!chain.id.is_empty());
        assert_eq!(chain.data_id, "data-123");
        assert_eq!(manager.chain_count(), 1);
    }

    #[test]
    fn test_integrity_manager_create_chain_with_ttl() {
        let config = IntegrityConfig::default();
        let mut manager = IntegrityManager::new(config);

        let chain = manager
            .create_chain("data-456".to_string(), Some(3600))
            .unwrap();

        assert!(chain.expires_at > chain.created_at);
    }

    #[test]
    fn test_integrity_manager_add_point() {
        let config = IntegrityConfig::default();
        let mut manager = IntegrityManager::new(config);

        let chain = manager.create_chain("data-789".to_string(), None).unwrap();

        let point = VerificationPoint {
            stage: PipelineStage::ClientWrite,
            checksum: "abcd1234".to_string(),
            algorithm: IntegrityAlgorithm::Crc32,
            timestamp: 1000000,
            data_length: 4096,
            verified: false,
        };

        manager.add_point(&chain.id, point).unwrap();

        let retrieved = manager.get_chain(&chain.id).unwrap();
        assert_eq!(retrieved.points.len(), 1);
        assert_eq!(retrieved.points[0].stage, PipelineStage::ClientWrite);
    }

    #[test]
    fn test_integrity_manager_verify_point_valid() {
        let config = IntegrityConfig::default();
        let mut manager = IntegrityManager::new(config);

        let chain = manager.create_chain("data-abc".to_string(), None).unwrap();

        let data = b"test data for verification";
        let checksum = manager
            .compute_checksum(data, &IntegrityAlgorithm::Crc32)
            .unwrap();

        let point = VerificationPoint {
            stage: PipelineStage::ClientWrite,
            checksum: checksum.clone(),
            algorithm: IntegrityAlgorithm::Crc32,
            timestamp: 1000000,
            data_length: data.len() as u64,
            verified: false,
        };

        manager.add_point(&chain.id, point).unwrap();

        let result = manager
            .verify_point(&chain.id, PipelineStage::ClientWrite, data)
            .unwrap();

        assert!(matches!(result, VerificationResult::Valid { .. }));
    }

    #[test]
    fn test_integrity_manager_verify_point_invalid() {
        let config = IntegrityConfig::default();
        let mut manager = IntegrityManager::new(config);

        let chain = manager.create_chain("data-def".to_string(), None).unwrap();

        let point = VerificationPoint {
            stage: PipelineStage::ClientWrite,
            checksum: "wrong_checksum".to_string(),
            algorithm: IntegrityAlgorithm::Crc32,
            timestamp: 1000000,
            data_length: 100,
            verified: false,
        };

        manager.add_point(&chain.id, point).unwrap();

        let result = manager
            .verify_point(&chain.id, PipelineStage::ClientWrite, b"some data")
            .unwrap();

        assert!(matches!(result, VerificationResult::Invalid { .. }));
    }

    #[test]
    fn test_integrity_manager_verify_chain() {
        let config = IntegrityConfig::default();
        let mut manager = IntegrityManager::new(config);

        let chain = manager.create_chain("data-ghi".to_string(), None).unwrap();

        let data1 = b"stage 1 data";
        let data2 = b"stage 2 data";

        let checksum1 = manager
            .compute_checksum(data1, &IntegrityAlgorithm::Crc32)
            .unwrap();
        let checksum2 = manager
            .compute_checksum(data2, &IntegrityAlgorithm::Crc32)
            .unwrap();

        manager
            .add_point(
                &chain.id,
                VerificationPoint {
                    stage: PipelineStage::ClientWrite,
                    checksum: checksum1,
                    algorithm: IntegrityAlgorithm::Crc32,
                    timestamp: 1000000,
                    data_length: data1.len() as u64,
                    verified: false,
                },
            )
            .unwrap();

        manager
            .add_point(
                &chain.id,
                VerificationPoint {
                    stage: PipelineStage::Compress,
                    checksum: checksum2,
                    algorithm: IntegrityAlgorithm::Crc32,
                    timestamp: 2000000,
                    data_length: data2.len() as u64,
                    verified: false,
                },
            )
            .unwrap();

        let mut data_map = HashMap::new();
        data_map.insert(PipelineStage::ClientWrite, data1.to_vec());
        data_map.insert(PipelineStage::Compress, data2.to_vec());

        let results = manager.verify_chain(&chain.id, &data_map);

        assert_eq!(results.len(), 2);
        assert!(matches!(results[0], VerificationResult::Valid { .. }));
        assert!(matches!(results[1], VerificationResult::Valid { .. }));
    }

    #[test]
    fn test_integrity_manager_get_chain() {
        let config = IntegrityConfig::default();
        let mut manager = IntegrityManager::new(config);

        let chain = manager.create_chain("data-jkl".to_string(), None).unwrap();
        let chain_id = chain.id.clone();

        let retrieved = manager.get_chain(&chain_id);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().data_id, "data-jkl");

        let missing = manager.get_chain("non-existent");
        assert!(missing.is_none());
    }

    #[test]
    fn test_integrity_manager_remove_chain_existing() {
        let config = IntegrityConfig::default();
        let mut manager = IntegrityManager::new(config);

        let chain = manager.create_chain("data-mno".to_string(), None).unwrap();
        let chain_id = chain.id.clone();

        assert_eq!(manager.chain_count(), 1);

        manager.remove_chain(&chain_id).unwrap();
        assert_eq!(manager.chain_count(), 0);
    }

    #[test]
    fn test_integrity_manager_remove_chain_non_existing() {
        let config = IntegrityConfig::default();
        let mut manager = IntegrityManager::new(config);

        let result = manager.remove_chain("non-existent-id");
        assert!(result.is_err());
    }

    #[test]
    fn test_integrity_manager_gc_expired_chains() {
        let config = IntegrityConfig {
            chain_ttl_seconds: 0,
            ..Default::default()
        };
        let mut manager = IntegrityManager::new(config);

        manager.create_chain("data-1".to_string(), Some(0)).unwrap();
        manager.create_chain("data-2".to_string(), Some(0)).unwrap();
        manager.create_chain("data-3".to_string(), Some(1)).unwrap();

        std::thread::sleep(std::time::Duration::from_millis(1100));

        let removed = manager.gc_expired_chains().unwrap();
        assert_eq!(removed, 2);
        assert_eq!(manager.chain_count(), 1);
    }

    #[test]
    fn test_integrity_manager_chain_count() {
        let config = IntegrityConfig::default();
        let mut manager = IntegrityManager::new(config);

        assert_eq!(manager.chain_count(), 0);

        manager.create_chain("data-1".to_string(), None).unwrap();
        manager.create_chain("data-2".to_string(), None).unwrap();
        manager.create_chain("data-3".to_string(), None).unwrap();

        assert_eq!(manager.chain_count(), 3);
    }

    #[test]
    fn test_integrity_manager_chains_for_stage() {
        let config = IntegrityConfig::default();
        let mut manager = IntegrityManager::new(config);

        let chain1 = manager.create_chain("data-pqr".to_string(), None).unwrap();
        let chain2 = manager.create_chain("data-stu".to_string(), None).unwrap();

        manager
            .add_point(
                &chain1.id,
                VerificationPoint {
                    stage: PipelineStage::ClientWrite,
                    checksum: "1111".to_string(),
                    algorithm: IntegrityAlgorithm::Crc32,
                    timestamp: 1000000,
                    data_length: 100,
                    verified: false,
                },
            )
            .unwrap();

        manager
            .add_point(
                &chain2.id,
                VerificationPoint {
                    stage: PipelineStage::ClientWrite,
                    checksum: "2222".to_string(),
                    algorithm: IntegrityAlgorithm::Crc32,
                    timestamp: 2000000,
                    data_length: 200,
                    verified: false,
                },
            )
            .unwrap();

        manager
            .add_point(
                &chain2.id,
                VerificationPoint {
                    stage: PipelineStage::Compress,
                    checksum: "3333".to_string(),
                    algorithm: IntegrityAlgorithm::Crc32,
                    timestamp: 3000000,
                    data_length: 150,
                    verified: false,
                },
            )
            .unwrap();

        let write_chains = manager.chains_for_stage(PipelineStage::ClientWrite);
        assert_eq!(write_chains.len(), 2);

        let compress_chains = manager.chains_for_stage(PipelineStage::Compress);
        assert_eq!(compress_chains.len(), 1);

        let encrypt_chains = manager.chains_for_stage(PipelineStage::Encrypt);
        assert_eq!(encrypt_chains.len(), 0);
    }

    #[test]
    fn test_integrity_manager_stats() {
        let config = IntegrityConfig::default();
        let mut manager = IntegrityManager::new(config);

        let stats = manager.stats();
        assert_eq!(stats.total_chains, 0);

        manager.create_chain("data-vwx".to_string(), None).unwrap();

        let stats = manager.stats();
        assert_eq!(stats.total_chains, 1);
    }

    #[test]
    fn test_integrity_manager_compute_checksum_crc32() {
        let config = IntegrityConfig::default();
        let manager = IntegrityManager::new(config);

        let data = b"test data";
        let checksum = manager
            .compute_checksum(data, &IntegrityAlgorithm::Crc32)
            .unwrap();

        assert_eq!(checksum.len(), 8);
        assert!(checksum.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_integrity_manager_compute_checksum_crc64() {
        let config = IntegrityConfig::default();
        let manager = IntegrityManager::new(config);

        let data = b"test data";
        let checksum = manager
            .compute_checksum(data, &IntegrityAlgorithm::Crc64)
            .unwrap();

        assert_eq!(checksum.len(), 16);
        assert!(checksum.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_integrity_manager_compute_checksum_blake3() {
        let config = IntegrityConfig::default();
        let manager = IntegrityManager::new(config);

        let data = b"test data";
        let checksum = manager
            .compute_checksum(data, &IntegrityAlgorithm::Blake3)
            .unwrap();

        assert_eq!(checksum.len(), 64);
        assert!(checksum.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_integrity_manager_compute_checksum_xxhash64() {
        let config = IntegrityConfig::default();
        let manager = IntegrityManager::new(config);

        let data = b"test data";
        let checksum = manager
            .compute_checksum(data, &IntegrityAlgorithm::Xxhash64)
            .unwrap();

        assert_eq!(checksum.len(), 16);
        assert!(checksum.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_integrity_manager_verify_point_missing_stage() {
        let config = IntegrityConfig::default();
        let mut manager = IntegrityManager::new(config);

        let chain = manager.create_chain("data-yz".to_string(), None).unwrap();

        let result = manager
            .verify_point(&chain.id, PipelineStage::Encrypt, b"data")
            .unwrap();

        assert!(matches!(result, VerificationResult::MissingPoint { .. }));
    }

    #[test]
    fn test_integrity_manager_verify_point_chain_not_found() {
        let config = IntegrityConfig::default();
        let manager = IntegrityManager::new(config);

        let result = manager
            .verify_point("non-existent", PipelineStage::ClientWrite, b"data")
            .unwrap();

        assert!(matches!(result, VerificationResult::ChainNotFound { .. }));
    }

    #[test]
    fn test_integrity_manager_verify_chain_not_found() {
        let config = IntegrityConfig::default();
        let manager = IntegrityManager::new(config);

        let data_map = HashMap::new();
        let results = manager.verify_chain("non-existent", &data_map);

        assert_eq!(results.len(), 1);
        assert!(matches!(
            results[0],
            VerificationResult::ChainNotFound { .. }
        ));
    }
}
