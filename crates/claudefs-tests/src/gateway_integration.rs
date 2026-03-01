//! Gateway crate integration tests
//!
//! Tests for claudefs-gateway crate: wire validation, session management, exports.

use claudefs_gateway::config::ExportConfig;
use claudefs_gateway::export_manager::{ActiveExport, ExportManager, ExportStatus};
use claudefs_gateway::protocol::FileHandle3;
use claudefs_gateway::session::{ClientSession, SessionId, SessionManager, SessionProtocol};
use claudefs_gateway::wire::{
    validate_nfs_count, validate_nfs_fh, validate_nfs_filename, validate_nfs_path, validate_s3_key,
};

#[test]
fn test_validate_nfs_fh_empty_fails() {
    let result = validate_nfs_fh(&[]);
    assert!(result.is_err());
}

#[test]
fn test_validate_nfs_fh_one_byte_ok() {
    let result = validate_nfs_fh(&[1]);
    assert!(result.is_ok());
}

#[test]
fn test_validate_nfs_fh_64_bytes_ok() {
    let result = validate_nfs_fh(&[0u8; 64]);
    assert!(result.is_ok());
}

#[test]
fn test_validate_nfs_fh_65_bytes_fails() {
    let result = validate_nfs_fh(&[0u8; 65]);
    assert!(result.is_err());
}

#[test]
fn test_validate_nfs_fh_valid() {
    let result = validate_nfs_fh(&[1, 2, 3, 4]);
    assert!(result.is_ok());
}

#[test]
fn test_validate_nfs_filename_empty_fails() {
    let result = validate_nfs_filename("");
    assert!(result.is_err());
}

#[test]
fn test_validate_nfs_filename_normal_ok() {
    let result = validate_nfs_filename("file.txt");
    assert!(result.is_ok());
}

#[test]
fn test_validate_nfs_filename_with_slash_fails() {
    let result = validate_nfs_filename("a/b");
    assert!(result.is_err());
}

#[test]
fn test_validate_nfs_filename_with_null_fails() {
    let result = validate_nfs_filename("a\0b");
    assert!(result.is_err());
}

#[test]
fn test_validate_nfs_path_no_leading_slash_fails() {
    let result = validate_nfs_path("export");
    assert!(result.is_err());
}

#[test]
fn test_validate_nfs_path_root_ok() {
    let result = validate_nfs_path("/");
    assert!(result.is_ok());
}

#[test]
fn test_validate_nfs_path_long_path() {
    let path = "/".repeat(500);
    let result = validate_nfs_path(&path);
    assert!(result.is_ok());
}

#[test]
fn test_validate_nfs_path_with_null_fails() {
    let result = validate_nfs_path("/a\0b");
    assert!(result.is_err());
}

#[test]
fn test_validate_nfs_count_zero_fails() {
    let result = validate_nfs_count(0);
    assert!(result.is_err());
}

#[test]
fn test_validate_nfs_count_one_ok() {
    let result = validate_nfs_count(1);
    assert!(result.is_ok());
}

#[test]
fn test_validate_nfs_count_1mb_ok() {
    let result = validate_nfs_count(1_048_576);
    assert!(result.is_ok());
}

#[test]
fn test_session_id_new() {
    let id = SessionId::new(123);
    assert_eq!(id.as_u64(), 123);
}

#[test]
fn test_session_manager_create_session() {
    let manager = SessionManager::new();
    let id = manager.create_session(SessionProtocol::Nfs3, "192.168.1.1", 1000, 1000, 100);
    assert!(id.as_u64() > 0);
    assert_eq!(manager.count(), 1);
}

#[test]
fn test_session_manager_session_id_increments() {
    let manager = SessionManager::new();
    let id1 = manager.create_session(SessionProtocol::Nfs3, "192.168.1.1", 1000, 1000, 100);
    let id2 = manager.create_session(SessionProtocol::Nfs3, "192.168.1.2", 1001, 1001, 100);
    assert_ne!(id1, id2);
}

#[test]
fn test_client_session_record_op() {
    let id = SessionId::new(1);
    let mut session = ClientSession::new(id, SessionProtocol::Nfs3, "10.0.0.1", 0, 0, 100);
    session.record_op(150, 4096);
    assert_eq!(session.op_count, 1);
    assert_eq!(session.bytes_transferred, 4096);
}

#[test]
fn test_client_session_is_idle_timeout() {
    let id = SessionId::new(1);
    let session = ClientSession::new(id, SessionProtocol::Nfs3, "10.0.0.1", 0, 0, 100);
    assert!(!session.is_idle(150, 60));
    assert!(session.is_idle(200, 60));
}

#[test]
fn test_client_session_add_remove_mount() {
    let id = SessionId::new(1);
    let mut session = ClientSession::new(id, SessionProtocol::Nfs3, "10.0.0.1", 0, 0, 100);
    session.add_mount("/export");
    session.add_mount("/data");
    assert_eq!(session.mounts.len(), 2);
    session.remove_mount("/export");
    assert_eq!(session.mounts.len(), 1);
}

#[test]
fn test_export_manager_new_empty() {
    let manager = ExportManager::new();
    assert_eq!(manager.count(), 0);
}

#[test]
fn test_export_manager_add_export() {
    let manager = ExportManager::new();
    let config = ExportConfig::default_rw("/export/data");
    let result = manager.add_export(config, 100);
    assert!(result.is_ok());
    assert_eq!(manager.count(), 1);
}

#[test]
fn test_export_manager_duplicate_add_fails() {
    let manager = ExportManager::new();
    let config = ExportConfig::default_rw("/export/data");
    let _ = manager.add_export(config.clone(), 100);
    let result = manager.add_export(config, 200);
    assert!(result.is_err());
}

#[test]
fn test_export_manager_count() {
    let manager = ExportManager::new();
    let _ = manager.add_export(ExportConfig::default_rw("/export1"), 100);
    let _ = manager.add_export(ExportConfig::default_rw("/export2"), 200);
    assert_eq!(manager.count(), 2);
}

#[test]
fn test_export_manager_is_exported() {
    let manager = ExportManager::new();
    let config = ExportConfig::default_rw("/export/data");
    let _ = manager.add_export(config, 100);
    assert!(manager.is_exported("/export/data"));
    assert!(!manager.is_exported("/nonexistent"));
}

#[test]
fn test_file_handle_from_inode() {
    let fh = FileHandle3::from_inode(12345);
    assert_eq!(fh.as_inode(), Some(12345));
}

#[test]
fn test_export_config_default_rw() {
    let config = ExportConfig::default_rw("/data");
    assert_eq!(config.path, "/data");
    assert!(!config.read_only);
    assert!(config.root_squash);
}

#[test]
fn test_export_config_default_ro() {
    let config = ExportConfig::default_ro("/data");
    assert_eq!(config.path, "/data");
    assert!(config.read_only);
}

#[test]
fn test_active_export_new() {
    let config = ExportConfig::default_rw("/export");
    let fh = FileHandle3::from_inode(100);
    let export = ActiveExport::new(config, fh, 100);
    assert!(export.is_active());
    assert_eq!(export.status, ExportStatus::Active);
}

#[test]
fn test_export_status_values() {
    assert_eq!(ExportStatus::Active, ExportStatus::Active);
    assert_eq!(ExportStatus::Draining, ExportStatus::Draining);
    assert_eq!(ExportStatus::Disabled, ExportStatus::Disabled);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_nfs_filename_255_chars_ok() {
        let name = "a".repeat(255);
        let result = validate_nfs_filename(&name);
        assert!(result.is_ok());
    }

    #[test]
    fn test_session_protocol_values() {
        assert_eq!(SessionProtocol::Nfs3, SessionProtocol::Nfs3);
        assert_eq!(SessionProtocol::S3, SessionProtocol::S3);
        assert_eq!(SessionProtocol::Smb3, SessionProtocol::Smb3);
    }
}
