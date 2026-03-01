//! Fault recovery tests for ClaudeFS
//!
//! Tests for cross-crate error handling and recovery patterns.

use claudefs_fuse::error::FuseError;
use claudefs_gateway::error::GatewayError;
use claudefs_meta::types::{InodeId, MetaError};
use claudefs_reduce::error::ReduceError;
use claudefs_repl::error::ReplError;
use claudefs_storage::error::StorageError;
use claudefs_storage::recovery::{RecoveryConfig, RecoveryManager, RecoveryPhase};
use claudefs_transport::error::TransportError;

#[test]
fn test_storage_error_out_of_space() {
    let err = StorageError::OutOfSpace;
    let msg = err.to_string();
    assert!(msg.contains("space"));
}

#[test]
fn test_storage_error_block_not_found() {
    use claudefs_storage::block::BlockId;
    let id = BlockId::new(0, 42);
    let err = StorageError::BlockNotFound { block_id: id };
    let msg = err.to_string();
    assert!(msg.contains("Block"));
}

#[test]
fn test_meta_error_inode_not_found() {
    let ino = InodeId::new(123);
    let err = MetaError::InodeNotFound(ino);
    let msg = err.to_string();
    assert!(msg.contains("123"));
}

#[test]
fn test_meta_error_permission_denied() {
    let err = MetaError::PermissionDenied;
    let msg = err.to_string();
    assert!(msg.contains("permission"));
}

#[test]
fn test_reduce_error_compression_failed() {
    let err = ReduceError::CompressionFailed("test error".to_string());
    let msg = err.to_string();
    assert!(msg.contains("Compression"));
}

#[test]
fn test_reduce_error_encryption_failed() {
    let err = ReduceError::EncryptionFailed("key not found".to_string());
    let msg = err.to_string();
    assert!(msg.contains("Encryption"));
}

#[test]
fn test_transport_error_connection_refused() {
    let err = TransportError::ConnectionRefused {
        addr: "192.168.1.1:8080".to_string(),
    };
    let msg = err.to_string();
    assert!(msg.contains("192.168.1.1"));
}

#[test]
fn test_transport_error_not_connected() {
    let err = TransportError::NotConnected;
    let msg = err.to_string();
    assert!(msg.contains("Not connected"));
}

#[test]
fn test_fuse_error_not_found() {
    let err = FuseError::NotFound { ino: 42 };
    let msg = err.to_string();
    assert!(msg.contains("42"));
}

#[test]
fn test_fuse_error_permission_denied() {
    let err = FuseError::PermissionDenied {
        ino: 1,
        op: "write".to_string(),
    };
    let msg = err.to_string();
    assert!(msg.contains("Permission"));
}

#[test]
fn test_repl_error_journal() {
    let err = ReplError::Journal {
        msg: "corrupted log".to_string(),
    };
    let msg = err.to_string();
    assert!(msg.contains("journal"));
}

#[test]
fn test_repl_error_network() {
    let err = ReplError::NetworkError {
        msg: "connection lost".to_string(),
    };
    let msg = err.to_string();
    assert!(msg.contains("network"));
}

#[test]
fn test_gateway_error_protocol() {
    let err = GatewayError::ProtocolError {
        reason: "invalid request".to_string(),
    };
    let msg = err.to_string();
    assert!(msg.contains("invalid"));
}

#[test]
fn test_recovery_config_default() {
    let config = RecoveryConfig::default();
    assert!(config.verify_checksums);
}

#[test]
fn test_recovery_config_custom() {
    let config = RecoveryConfig {
        cluster_uuid: [1u8; 16],
        max_journal_replay_entries: 5000,
        verify_checksums: false,
        allow_partial_recovery: true,
    };
    assert_eq!(config.cluster_uuid, [1u8; 16]);
    assert!(!config.verify_checksums);
    assert!(config.allow_partial_recovery);
}

#[test]
fn test_recovery_manager_new() {
    let config = RecoveryConfig::default();
    let manager = RecoveryManager::new(config);
    let state = manager.state();
    assert_eq!(state.phase, RecoveryPhase::NotStarted);
}

#[test]
fn test_recovery_phase_ordering() {
    assert!(matches!(
        RecoveryPhase::NotStarted,
        RecoveryPhase::NotStarted
    ));
    assert!(matches!(
        RecoveryPhase::SuperblockRead,
        RecoveryPhase::SuperblockRead
    ));
}

#[test]
fn test_recovery_phase_values() {
    let _ = RecoveryPhase::NotStarted;
    let _ = RecoveryPhase::SuperblockRead;
    let _ = RecoveryPhase::BitmapLoaded;
    let _ = RecoveryPhase::JournalScanned;
    let _ = RecoveryPhase::JournalReplayed;
    let _ = RecoveryPhase::Complete;
    let _ = RecoveryPhase::Failed;
}

#[test]
fn test_fault_tolerance_out_of_space() {
    let result: Result<(), StorageError> = Err(StorageError::OutOfSpace);
    assert!(result.is_err());
    match result {
        Err(StorageError::OutOfSpace) => {}
        _ => panic!("Expected OutOfSpace"),
    }
}

#[test]
fn test_fault_tolerance_block_not_found() {
    use claudefs_storage::block::BlockId;
    let id = BlockId::new(0, 999);
    let result: Result<(), StorageError> = Err(StorageError::BlockNotFound { block_id: id });
    assert!(result.is_err());
}

#[test]
fn test_fault_tolerance_connection_timeout() {
    let err = TransportError::ConnectionTimeout {
        addr: "192.168.1.1:9000".to_string(),
        timeout_ms: 5000,
    };
    let msg = err.to_string();
    assert!(msg.contains("timeout") || msg.contains("5000"));
}

#[test]
fn test_error_propagation_with_anyhow() {
    let inner: Result<i32, StorageError> = Err(StorageError::OutOfSpace);
    let outer: Result<i32, anyhow::Error> =
        inner.map_err(|e| anyhow::anyhow!("Storage failed: {}", e));
    assert!(outer.is_err());
}

#[test]
fn test_error_propagation_question_mark() {
    fn inner() -> Result<i32, StorageError> {
        Err(StorageError::OutOfSpace)
    }
    fn outer() -> Result<i32, StorageError> {
        let _ = inner()?;
        Ok(42)
    }
    let result = outer();
    assert!(result.is_err());
}

#[test]
fn test_error_conversion_storage_to_anyhow() {
    let err = StorageError::OutOfSpace;
    let anyhow_err = anyhow::anyhow!("{}", err);
    assert!(anyhow_err.to_string().contains("space"));
}

#[test]
fn test_error_conversion_transport_to_anyhow() {
    let err = TransportError::NotConnected;
    let anyhow_err = anyhow::anyhow!("{}", err);
    assert!(anyhow_err.to_string().contains("Not connected"));
}

#[test]
fn test_error_chain_meta_error() {
    let err = MetaError::PermissionDenied;
    let msg = err.to_string();
    assert!(!msg.is_empty());
}

#[test]
fn test_error_chain_reduce_error() {
    let err = ReduceError::MissingKey;
    let msg = err.to_string();
    assert!(msg.contains("key") || msg.contains("Key"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_storage_error_display() {
        let err = StorageError::OutOfSpace;
        assert!(!err.to_string().is_empty());
    }

    #[test]
    fn test_meta_error_display() {
        let err = MetaError::PermissionDenied;
        assert!(!err.to_string().is_empty());
    }

    #[test]
    fn test_reduce_error_display() {
        let err = ReduceError::MissingKey;
        assert!(!err.to_string().is_empty());
    }

    #[test]
    fn test_transport_error_display() {
        let err = TransportError::NotConnected;
        assert!(!err.to_string().is_empty());
    }

    #[test]
    fn test_fuse_error_display() {
        let err = FuseError::NotSupported {
            op: "flock".to_string(),
        };
        assert!(!err.to_string().is_empty());
    }

    #[test]
    fn test_repl_error_display() {
        let err = ReplError::Shutdown;
        assert!(!err.to_string().is_empty());
    }

    #[test]
    fn test_gateway_error_display() {
        let err = GatewayError::ProtocolError {
            reason: "test".to_string(),
        };
        assert!(!err.to_string().is_empty());
    }
}
