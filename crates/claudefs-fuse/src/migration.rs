use crate::inode::{InodeId, InodeKind};

#[derive(Debug, Clone)]
pub struct MigrationEntry {
    pub ino: InodeId,
    pub kind: InodeKind,
    pub path: String,
    pub size: u64,
    pub checksum: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MigrationPhase {
    Idle,
    Scanning,
    Copying,
    Verifying,
    Done,
    Failed { reason: String },
}

#[derive(Debug, Clone)]
pub struct MigrationConfig {
    pub max_entries: usize,
    pub verify_checksums: bool,
    pub skip_empty_dirs: bool,
    pub checkpoint_interval: u64,
}

impl Default for MigrationConfig {
    fn default() -> Self {
        MigrationConfig {
            max_entries: 1000,
            verify_checksums: true,
            skip_empty_dirs: false,
            checkpoint_interval: 100,
        }
    }
}

#[derive(Debug, Clone)]
pub struct MigrationCheckpoint {
    pub entries_scanned: u64,
    pub entries_copied: u64,
    pub bytes_copied: u64,
    pub last_path: String,
    pub errors: Vec<String>,
}

impl MigrationCheckpoint {
    pub fn new() -> Self {
        MigrationCheckpoint {
            entries_scanned: 0,
            entries_copied: 0,
            bytes_copied: 0,
            last_path: String::new(),
            errors: Vec::new(),
        }
    }

    pub fn record_copied(&mut self, path: &str, bytes: u64) {
        self.entries_copied += 1;
        self.bytes_copied += bytes;
        self.last_path = path.to_string();
    }

    pub fn add_error(&mut self, err: &str) {
        self.errors.push(err.to_string());
    }

    pub fn is_resumable(&self) -> bool {
        self.entries_scanned > 0
    }
}

impl Default for MigrationCheckpoint {
    fn default() -> Self {
        Self::new()
    }
}

pub struct MigrationManager {
    #[allow(dead_code)]
    config: MigrationConfig,
    phase: MigrationPhase,
    checkpoint: MigrationCheckpoint,
    scanned_entries: Vec<MigrationEntry>,
}

impl MigrationManager {
    pub fn new(config: MigrationConfig) -> Self {
        MigrationManager {
            config,
            phase: MigrationPhase::Idle,
            checkpoint: MigrationCheckpoint::new(),
            scanned_entries: Vec::new(),
        }
    }

    pub fn with_default_config() -> Self {
        Self::new(MigrationConfig::default())
    }

    pub fn add_scanned_entry(&mut self, entry: MigrationEntry) {
        self.scanned_entries.push(entry);
        self.checkpoint.entries_scanned += 1;
    }

    pub fn begin_scan(&mut self) {
        self.phase = MigrationPhase::Scanning;
    }

    pub fn finish_scan(&mut self) {
        self.phase = MigrationPhase::Copying;
    }

    pub fn record_copied(&mut self, path: &str, bytes: u64) {
        self.checkpoint.record_copied(path, bytes);
    }

    pub fn begin_verify(&mut self) {
        self.phase = MigrationPhase::Verifying;
    }

    pub fn complete(&mut self) {
        self.phase = MigrationPhase::Done;
    }

    pub fn fail(&mut self, reason: &str) {
        self.phase = MigrationPhase::Failed {
            reason: reason.to_string(),
        };
    }

    pub fn phase(&self) -> &MigrationPhase {
        &self.phase
    }

    pub fn checkpoint(&self) -> &MigrationCheckpoint {
        &self.checkpoint
    }

    pub fn scanned_count(&self) -> usize {
        self.scanned_entries.len()
    }

    pub fn copied_count(&self) -> u64 {
        self.checkpoint.entries_copied
    }

    pub fn bytes_copied(&self) -> u64 {
        self.checkpoint.bytes_copied
    }

    pub fn error_count(&self) -> usize {
        self.checkpoint.errors.len()
    }

    pub fn files(&self) -> Vec<&MigrationEntry> {
        self.scanned_entries
            .iter()
            .filter(|e| matches!(e.kind, InodeKind::File))
            .collect()
    }

    pub fn directories(&self) -> Vec<&MigrationEntry> {
        self.scanned_entries
            .iter()
            .filter(|e| matches!(e.kind, InodeKind::Directory))
            .collect()
    }

    pub fn compute_checksum(data: &[u8]) -> u64 {
        let mut sum: u64 = 0;
        for (i, &byte) in data.iter().enumerate() {
            let i_u64 = (i as u64).saturating_add(1);
            let contrib = (byte as u64).saturating_mul(i_u64);
            sum = sum.saturating_add(contrib) % u64::MAX;
        }
        sum
    }

    pub fn can_resume(&self) -> bool {
        self.checkpoint.entries_scanned > 0 && !self.checkpoint.last_path.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_migration_config_default_values() {
        let config = MigrationConfig::default();
        assert_eq!(config.max_entries, 1000);
        assert!(config.verify_checksums);
        assert!(!config.skip_empty_dirs);
        assert_eq!(config.checkpoint_interval, 100);
    }

    #[test]
    fn test_migration_checkpoint_new_starts_at_zero() {
        let cp = MigrationCheckpoint::new();
        assert_eq!(cp.entries_scanned, 0);
        assert_eq!(cp.entries_copied, 0);
        assert_eq!(cp.bytes_copied, 0);
        assert!(cp.last_path.is_empty());
        assert!(cp.errors.is_empty());
    }

    #[test]
    fn test_migration_checkpoint_record_copied_updates_counters() {
        let mut cp = MigrationCheckpoint::new();
        cp.record_copied("/foo", 100);
        assert_eq!(cp.entries_copied, 1);
        assert_eq!(cp.bytes_copied, 100);
        assert_eq!(cp.last_path, "/foo");
    }

    #[test]
    fn test_migration_checkpoint_add_error_appends() {
        let mut cp = MigrationCheckpoint::new();
        cp.add_error("err1");
        cp.add_error("err2");
        assert_eq!(cp.errors.len(), 2);
        assert_eq!(cp.errors[0], "err1");
    }

    #[test]
    fn test_migration_checkpoint_is_resumable_false_when_zero_scanned() {
        let cp = MigrationCheckpoint::new();
        assert!(!cp.is_resumable());
    }

    #[test]
    fn test_migration_checkpoint_is_resumable_true_when_scanned() {
        let mut cp = MigrationCheckpoint::new();
        cp.entries_scanned = 1;
        assert!(cp.is_resumable());
    }

    #[test]
    fn test_migration_entry_file_fields_accessible() {
        let entry = MigrationEntry {
            ino: 1,
            kind: InodeKind::File,
            path: "/file.txt".into(),
            size: 100,
            checksum: Some(12345),
        };
        assert_eq!(entry.ino, 1);
        assert_eq!(entry.kind, InodeKind::File);
        assert_eq!(entry.size, 100);
    }

    #[test]
    fn test_migration_entry_directory_fields_accessible() {
        let entry = MigrationEntry {
            ino: 2,
            kind: InodeKind::Directory,
            path: "/dir".into(),
            size: 4096,
            checksum: None,
        };
        assert_eq!(entry.kind, InodeKind::Directory);
    }

    #[test]
    fn test_migration_manager_new_starts_idle() {
        let mgr = MigrationManager::with_default_config();
        assert_eq!(*mgr.phase(), MigrationPhase::Idle);
    }

    #[test]
    fn test_migration_manager_begin_scan_sets_scanning() {
        let mut mgr = MigrationManager::with_default_config();
        mgr.begin_scan();
        assert_eq!(*mgr.phase(), MigrationPhase::Scanning);
    }

    #[test]
    fn test_migration_manager_finish_scan_sets_copying() {
        let mut mgr = MigrationManager::with_default_config();
        mgr.begin_scan();
        mgr.finish_scan();
        assert_eq!(*mgr.phase(), MigrationPhase::Copying);
    }

    #[test]
    fn test_migration_manager_begin_verify_sets_verifying() {
        let mut mgr = MigrationManager::with_default_config();
        mgr.begin_verify();
        assert_eq!(*mgr.phase(), MigrationPhase::Verifying);
    }

    #[test]
    fn test_migration_manager_complete_sets_done() {
        let mut mgr = MigrationManager::with_default_config();
        mgr.complete();
        assert_eq!(*mgr.phase(), MigrationPhase::Done);
    }

    #[test]
    fn test_migration_manager_fail_sets_failed_with_reason() {
        let mut mgr = MigrationManager::with_default_config();
        mgr.fail("disk full");
        match mgr.phase() {
            MigrationPhase::Failed { reason } => assert_eq!(reason, "disk full"),
            _ => panic!("expected Failed phase"),
        }
    }

    #[test]
    fn test_migration_manager_add_scanned_entry_increments_count() {
        let mut mgr = MigrationManager::with_default_config();
        mgr.add_scanned_entry(MigrationEntry {
            ino: 1,
            kind: InodeKind::File,
            path: "/f".into(),
            size: 0,
            checksum: None,
        });
        assert_eq!(mgr.scanned_count(), 1);
        assert_eq!(mgr.checkpoint().entries_scanned, 1);
    }

    #[test]
    fn test_migration_manager_record_copied_updates_bytes() {
        let mut mgr = MigrationManager::with_default_config();
        mgr.record_copied("/file", 500);
        assert_eq!(mgr.bytes_copied(), 500);
        assert_eq!(mgr.copied_count(), 1);
    }

    #[test]
    fn test_migration_manager_files_returns_only_files() {
        let mut mgr = MigrationManager::with_default_config();
        mgr.add_scanned_entry(MigrationEntry {
            ino: 1,
            kind: InodeKind::File,
            path: "/f".into(),
            size: 0,
            checksum: None,
        });
        mgr.add_scanned_entry(MigrationEntry {
            ino: 2,
            kind: InodeKind::Directory,
            path: "/d".into(),
            size: 0,
            checksum: None,
        });
        let files = mgr.files();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].ino, 1);
    }

    #[test]
    fn test_migration_manager_directories_returns_only_dirs() {
        let mut mgr = MigrationManager::with_default_config();
        mgr.add_scanned_entry(MigrationEntry {
            ino: 1,
            kind: InodeKind::File,
            path: "/f".into(),
            size: 0,
            checksum: None,
        });
        mgr.add_scanned_entry(MigrationEntry {
            ino: 2,
            kind: InodeKind::Directory,
            path: "/d".into(),
            size: 0,
            checksum: None,
        });
        let dirs = mgr.directories();
        assert_eq!(dirs.len(), 1);
        assert_eq!(dirs[0].ino, 2);
    }

    #[test]
    fn test_migration_manager_error_count_tracks_errors() {
        let mut mgr = MigrationManager::with_default_config();
        mgr.checkpoint.add_error("e1");
        mgr.checkpoint.add_error("e2");
        assert_eq!(mgr.error_count(), 2);
    }

    #[test]
    fn test_migration_manager_can_resume_false_when_idle() {
        let mgr = MigrationManager::with_default_config();
        assert!(!mgr.can_resume());
    }

    #[test]
    fn test_migration_manager_can_resume_true_after_checkpoint() {
        let mut mgr = MigrationManager::with_default_config();
        mgr.add_scanned_entry(MigrationEntry {
            ino: 1,
            kind: InodeKind::File,
            path: "/f".into(),
            size: 0,
            checksum: None,
        });
        mgr.record_copied("/f", 0);
        assert!(mgr.can_resume());
    }

    #[test]
    fn test_compute_checksum_empty_data_is_zero() {
        assert_eq!(MigrationManager::compute_checksum(&[]), 0);
    }

    #[test]
    fn test_compute_checksum_same_data_same_result() {
        let data = [1u8, 2, 3, 4, 5];
        let a = MigrationManager::compute_checksum(&data);
        let b = MigrationManager::compute_checksum(&data);
        assert_eq!(a, b);
    }

    #[test]
    fn test_compute_checksum_different_data_different_result() {
        let a = MigrationManager::compute_checksum(&[1u8, 2, 3]);
        let b = MigrationManager::compute_checksum(&[3u8, 2, 1]);
        assert_ne!(a, b);
    }

    #[test]
    fn test_migration_config_skip_empty_dirs_default_false() {
        let config = MigrationConfig::default();
        assert!(!config.skip_empty_dirs);
    }
}
