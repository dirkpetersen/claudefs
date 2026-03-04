//! Crash recovery infrastructure for FUSE filesystem.
//!
//! This module provides state machine-based crash recovery to restore filesystem
//! consistency after an unclean shutdown. It tracks open files and pending writes
//! from a recovery journal and orchestrates the replay process.

use crate::error::Result;
use crate::inode::InodeId;

/// State of the crash recovery process.
///
/// The recovery process follows a strict state machine:
/// `Idle` -> `Scanning` -> `Replaying` -> `Complete` or `Failed`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryState {
    /// No recovery in progress; ready to begin scanning.
    Idle,
    /// Scanning the recovery journal for open files and pending writes.
    Scanning,
    /// Replaying recorded operations.
    Replaying {
        /// Number of operations already replayed.
        replayed: u32,
        /// Total number of operations to replay.
        total: u32,
    },
    /// Recovery completed successfully.
    Complete {
        /// Number of operations successfully recovered.
        recovered: u32,
        /// Number of orphaned resources cleaned up.
        orphaned: u32,
    },
    /// Recovery failed with an error.
    Failed(String),
}

impl RecoveryState {
    /// Returns `true` if recovery is currently in progress.
    ///
    /// Recovery is in progress during `Scanning` or `Replaying` states.
    pub fn is_in_progress(&self) -> bool {
        matches!(
            self,
            RecoveryState::Scanning | RecoveryState::Replaying { .. }
        )
    }

    /// Returns `true` if recovery has reached a terminal state.
    ///
    /// Terminal states are `Complete` (success) and `Failed` (error).
    pub fn is_complete(&self) -> bool {
        matches!(
            self,
            RecoveryState::Complete { .. } | RecoveryState::Failed(_)
        )
    }
}

/// Record of a file that was open at the time of crash.
///
/// Used during recovery to restore file handles and identify
/// files that may have pending writes.
#[derive(Debug, Clone)]
pub struct OpenFileRecord {
    /// Inode number of the open file.
    pub ino: InodeId,
    /// File descriptor number assigned by the kernel.
    pub fd: u64,
    /// Process ID that opened the file.
    pub pid: u32,
    /// Open flags (O_RDONLY, O_WRONLY, O_RDWR, O_APPEND, etc.).
    pub flags: u32,
    /// Path hint for diagnostic messages (may not be canonical).
    pub path_hint: String,
}

impl OpenFileRecord {
    /// Returns `true` if the file was opened for writing.
    ///
    /// This is true if either `O_WRONLY` (1) or `O_RDWR` (2) flag is set.
    pub fn is_writable(&self) -> bool {
        (self.flags & 1 != 0) || (self.flags & 2 != 0)
    }

    /// Returns `true` if the file was opened in append-only mode.
    ///
    /// Checks for the `O_APPEND` flag (0x400 = 1024).
    pub fn is_append_only(&self) -> bool {
        self.flags & 1024 != 0
    }
}

/// Record of a pending write that was not flushed before crash.
///
/// Used during recovery to identify dirty ranges that may need
/// special handling or cleanup.
#[derive(Debug, Clone)]
pub struct PendingWrite {
    /// Inode number of the file with pending write.
    pub ino: InodeId,
    /// Byte offset where the write began.
    pub offset: u64,
    /// Length of the write in bytes.
    pub len: u64,
    /// Timestamp (in seconds) when the write was first dirtied.
    pub dirty_since_secs: u64,
}

impl PendingWrite {
    /// Returns the age of this pending write in seconds.
    ///
    /// Computed as `now - dirty_since_secs`, saturating at zero.
    pub fn age_secs(&self, now: u64) -> u64 {
        now.saturating_sub(self.dirty_since_secs)
    }

    /// Returns `true` if this pending write is older than `max_age_secs`.
    ///
    /// Stale writes may indicate incomplete flushes or lost transactions.
    pub fn is_stale(&self, now: u64, max_age_secs: u64) -> bool {
        self.age_secs(now) > max_age_secs
    }
}

/// In-memory journal of recovery records collected during scanning.
///
/// Aggregates open file records and pending writes for analysis
/// during the recovery replay phase.
#[derive(Debug, Clone, Default)]
pub struct RecoveryJournal {
    open_files: Vec<OpenFileRecord>,
    pending_writes: Vec<PendingWrite>,
}

impl RecoveryJournal {
    /// Creates a new empty recovery journal.
    pub fn new() -> Self {
        Self {
            open_files: Vec::new(),
            pending_writes: Vec::new(),
        }
    }

    /// Adds an open file record to the journal.
    pub fn add_open_file(&mut self, record: OpenFileRecord) {
        self.open_files.push(record);
    }

    /// Adds a pending write record to the journal.
    pub fn add_pending_write(&mut self, write: PendingWrite) {
        self.pending_writes.push(write);
    }

    /// Returns the number of open file records in the journal.
    pub fn open_file_count(&self) -> usize {
        self.open_files.len()
    }

    /// Returns the number of pending write records in the journal.
    pub fn pending_write_count(&self) -> usize {
        self.pending_writes.len()
    }

    /// Returns references to all writable open files in the journal.
    ///
    /// Filters out read-only files, returning only those with write
    /// or read-write access.
    pub fn writable_open_files(&self) -> Vec<&OpenFileRecord> {
        self.open_files.iter().filter(|f| f.is_writable()).collect()
    }

    /// Returns references to pending writes older than `max_age_secs`.
    ///
    /// Uses `now_secs` as the current timestamp for age calculation.
    pub fn stale_pending_writes(&self, now_secs: u64, max_age_secs: u64) -> Vec<&PendingWrite> {
        self.pending_writes
            .iter()
            .filter(|w| w.is_stale(now_secs, max_age_secs))
            .collect()
    }
}

/// Configuration parameters for crash recovery.
#[derive(Debug, Clone, Copy)]
pub struct RecoveryConfig {
    /// Maximum time in seconds allowed for the recovery process.
    pub max_recovery_secs: u64,
    /// Maximum number of open files to track during recovery.
    pub max_open_files: usize,
    /// Age threshold in seconds for identifying stale pending writes.
    pub stale_write_age_secs: u64,
}

impl RecoveryConfig {
    /// Returns the default recovery configuration.
    ///
    /// Defaults: 30s timeout, 10,000 open files, 300s stale threshold.
    pub fn default_config() -> Self {
        Self {
            max_recovery_secs: 30,
            max_open_files: 10_000,
            stale_write_age_secs: 300,
        }
    }
}

/// Orchestrates crash recovery using a state machine.
///
/// Manages the recovery process from idle through scanning, replaying,
/// and completion or failure. Maintains a journal of recovery records.
pub struct CrashRecovery {
    config: RecoveryConfig,
    state: RecoveryState,
    journal: RecoveryJournal,
}

impl CrashRecovery {
    /// Creates a new crash recovery engine with the given configuration.
    pub fn new(config: RecoveryConfig) -> Self {
        Self {
            config,
            state: RecoveryState::Idle,
            journal: RecoveryJournal::new(),
        }
    }

    /// Returns a reference to the current recovery state.
    pub fn state(&self) -> &RecoveryState {
        &self.state
    }

    /// Begins the scanning phase of recovery.
    ///
    /// Only valid from the `Idle` state. Transitions to `Scanning`.
    pub fn begin_scan(&mut self) -> Result<()> {
        if !matches!(self.state, RecoveryState::Idle) {
            return Err(crate::error::FuseError::InvalidArgument {
                msg: "begin_scan only allowed in Idle state".to_string(),
            });
        }

        self.state = RecoveryState::Scanning;
        Ok(())
    }

    /// Records an open file discovered during scanning.
    ///
    /// Only valid in the `Scanning` state. Enforces `max_open_files` limit.
    pub fn record_open_file(&mut self, record: OpenFileRecord) -> Result<()> {
        if !matches!(self.state, RecoveryState::Scanning) {
            return Err(crate::error::FuseError::InvalidArgument {
                msg: "record_open_file only allowed in Scanning state".to_string(),
            });
        }

        if self.journal.open_file_count() >= self.config.max_open_files {
            return Err(crate::error::FuseError::InvalidArgument {
                msg: "max_open_files exceeded".to_string(),
            });
        }

        self.journal.add_open_file(record);
        Ok(())
    }

    /// Records a pending write discovered during scanning.
    ///
    /// Only valid in the `Scanning` state.
    pub fn record_pending_write(&mut self, write: PendingWrite) -> Result<()> {
        if !matches!(self.state, RecoveryState::Scanning) {
            return Err(crate::error::FuseError::InvalidArgument {
                msg: "record_pending_write only allowed in Scanning state".to_string(),
            });
        }

        self.journal.add_pending_write(write);
        Ok(())
    }

    /// Begins the replay phase with the given total operation count.
    ///
    /// Only valid in the `Scanning` state. Transitions to `Replaying`.
    pub fn begin_replay(&mut self, total: u32) -> Result<()> {
        if !matches!(self.state, RecoveryState::Scanning) {
            return Err(crate::error::FuseError::InvalidArgument {
                msg: "begin_replay only allowed in Scanning state".to_string(),
            });
        }

        self.state = RecoveryState::Replaying { replayed: 0, total };
        Ok(())
    }

    /// Advances the replay counter by the given number of operations.
    ///
    /// Clamps at the total operation count. Only has effect in `Replaying` state.
    pub fn advance_replay(&mut self, count: u32) {
        if let RecoveryState::Replaying { replayed, total } = &mut self.state {
            *replayed = (*replayed + count).min(*total);
        }
    }

    /// Completes recovery with the given orphaned resource count.
    ///
    /// Only valid in the `Replaying` state. Transitions to `Complete`.
    pub fn complete(&mut self, orphaned: u32) -> Result<()> {
        if !matches!(self.state, RecoveryState::Replaying { .. }) {
            return Err(crate::error::FuseError::InvalidArgument {
                msg: "complete only allowed in Replaying state".to_string(),
            });
        }

        let replayed = match self.state {
            RecoveryState::Replaying { replayed, .. } => replayed,
            _ => 0,
        };

        self.state = RecoveryState::Complete {
            recovered: replayed,
            orphaned,
        };
        Ok(())
    }

    /// Marks recovery as failed with the given reason.
    ///
    /// Can be called from any state. Transitions to `Failed`.
    pub fn fail(&mut self, reason: String) {
        self.state = RecoveryState::Failed(reason);
    }

    /// Resets the recovery engine to idle state with a fresh journal.
    pub fn reset(&mut self) {
        self.state = RecoveryState::Idle;
        self.journal = RecoveryJournal::new();
    }

    /// Returns a reference to the recovery journal.
    pub fn journal(&self) -> &RecoveryJournal {
        &self.journal
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_begin_scan_transitions_state() {
        let mut recovery = CrashRecovery::new(RecoveryConfig::default_config());

        assert!(matches!(recovery.state(), RecoveryState::Idle));

        recovery.begin_scan().unwrap();
        assert!(matches!(recovery.state(), RecoveryState::Scanning));
    }

    #[test]
    fn test_begin_scan_errors_if_not_idle() {
        let mut recovery = CrashRecovery::new(RecoveryConfig::default_config());

        recovery.begin_scan().unwrap();

        let result = recovery.begin_scan();
        assert!(result.is_err());
    }

    #[test]
    fn test_record_open_file_errors_if_not_scanning() {
        let mut recovery = CrashRecovery::new(RecoveryConfig::default_config());

        let record = OpenFileRecord {
            ino: 100,
            fd: 1,
            pid: 1,
            flags: 2,
            path_hint: "/test".to_string(),
        };

        let result = recovery.record_open_file(record);
        assert!(result.is_err());
    }

    #[test]
    fn test_max_open_files_limit() {
        let mut config = RecoveryConfig::default_config();
        config.max_open_files = 2;
        let mut recovery = CrashRecovery::new(config);

        recovery.begin_scan().unwrap();

        let record = OpenFileRecord {
            ino: 100,
            fd: 1,
            pid: 1,
            flags: 2,
            path_hint: "/test".to_string(),
        };

        assert!(recovery.record_open_file(record.clone()).is_ok());
        assert!(recovery.record_open_file(record.clone()).is_ok());

        let result = recovery.record_open_file(record);
        assert!(result.is_err());
    }

    #[test]
    fn test_begin_replay_transitions() {
        let mut recovery = CrashRecovery::new(RecoveryConfig::default_config());

        recovery.begin_scan().unwrap();
        recovery.begin_replay(10).unwrap();

        assert!(matches!(
            recovery.state(),
            RecoveryState::Replaying {
                replayed: 0,
                total: 10
            }
        ));
    }

    #[test]
    fn test_advance_replay_increments() {
        let mut recovery = CrashRecovery::new(RecoveryConfig::default_config());

        recovery.begin_scan().unwrap();
        recovery.begin_replay(10).unwrap();

        recovery.advance_replay(3);

        assert!(matches!(
            recovery.state(),
            RecoveryState::Replaying {
                replayed: 3,
                total: 10
            }
        ));
    }

    #[test]
    fn test_advance_replay_clamps_at_total() {
        let mut recovery = CrashRecovery::new(RecoveryConfig::default_config());

        recovery.begin_scan().unwrap();
        recovery.begin_replay(10).unwrap();

        recovery.advance_replay(100);

        assert!(matches!(
            recovery.state(),
            RecoveryState::Replaying {
                replayed: 10,
                total: 10
            }
        ));
    }

    #[test]
    fn test_complete_transitions() {
        let mut recovery = CrashRecovery::new(RecoveryConfig::default_config());

        recovery.begin_scan().unwrap();
        recovery.begin_replay(10).unwrap();
        recovery.advance_replay(5);
        recovery.complete(2).unwrap();

        assert!(matches!(
            recovery.state(),
            RecoveryState::Complete {
                recovered: 5,
                orphaned: 2
            }
        ));
    }

    #[test]
    fn test_fail_from_any_state() {
        let mut recovery = CrashRecovery::new(RecoveryConfig::default_config());

        recovery.begin_scan().unwrap();
        recovery.fail("test error".to_string());

        assert!(matches!(
            recovery.state(),
            RecoveryState::Failed(s) if s == "test error"
        ));
    }

    #[test]
    fn test_reset_clears_journal() {
        let mut recovery = CrashRecovery::new(RecoveryConfig::default_config());

        recovery.begin_scan().unwrap();

        let record = OpenFileRecord {
            ino: 100,
            fd: 1,
            pid: 1,
            flags: 2,
            path_hint: "/test".to_string(),
        };
        recovery.record_open_file(record).unwrap();

        assert!(recovery.journal().open_file_count() > 0);

        recovery.reset();

        assert!(matches!(recovery.state(), RecoveryState::Idle));
        assert!(recovery.journal().open_file_count() == 0);
    }

    #[test]
    fn test_is_writable_with_ordwr_flags() {
        let record = OpenFileRecord {
            ino: 100,
            fd: 1,
            pid: 1,
            flags: 2,
            path_hint: "/test".to_string(),
        };
        assert!(record.is_writable());
    }

    #[test]
    fn test_is_writable_with_owronly_flags() {
        let record = OpenFileRecord {
            ino: 100,
            fd: 1,
            pid: 1,
            flags: 1,
            path_hint: "/test".to_string(),
        };
        assert!(record.is_writable());
    }

    #[test]
    fn test_is_append_only() {
        let record = OpenFileRecord {
            ino: 100,
            fd: 1,
            pid: 1,
            flags: 1024,
            path_hint: "/test".to_string(),
        };
        assert!(record.is_append_only());
    }

    #[test]
    fn test_stale_pending_writes_age_filter() {
        let mut recovery = CrashRecovery::new(RecoveryConfig::default_config());

        recovery.begin_scan().unwrap();

        let fresh = PendingWrite {
            ino: 100,
            offset: 0,
            len: 100,
            dirty_since_secs: 1000,
        };

        let stale = PendingWrite {
            ino: 200,
            offset: 0,
            len: 100,
            dirty_since_secs: 500,
        };

        recovery.record_pending_write(fresh).unwrap();
        recovery.record_pending_write(stale).unwrap();

        let stale_writes = recovery.journal().stale_pending_writes(1000, 300);

        assert_eq!(stale_writes.len(), 1);
        assert_eq!(stale_writes[0].ino, 200);
    }

    #[test]
    fn test_writable_open_files_filter() {
        let mut recovery = CrashRecovery::new(RecoveryConfig::default_config());

        recovery.begin_scan().unwrap();

        let writable = OpenFileRecord {
            ino: 100,
            fd: 1,
            pid: 1,
            flags: 2,
            path_hint: "/test".to_string(),
        };

        let readonly = OpenFileRecord {
            ino: 200,
            fd: 2,
            pid: 1,
            flags: 0,
            path_hint: "/test2".to_string(),
        };

        recovery.record_open_file(writable).unwrap();
        recovery.record_open_file(readonly).unwrap();

        let writable_files = recovery.journal().writable_open_files();

        assert_eq!(writable_files.len(), 1);
        assert_eq!(writable_files[0].ino, 100);
    }

    #[test]
    fn test_happy_path_recovery_sequence() {
        let mut recovery = CrashRecovery::new(RecoveryConfig::default_config());

        recovery.begin_scan().unwrap();

        let record = OpenFileRecord {
            ino: 100,
            fd: 1,
            pid: 1,
            flags: 2,
            path_hint: "/test".to_string(),
        };
        recovery.record_open_file(record).unwrap();

        let write = PendingWrite {
            ino: 100,
            offset: 0,
            len: 4096,
            dirty_since_secs: 1000,
        };
        recovery.record_pending_write(write).unwrap();

        recovery.begin_replay(1).unwrap();
        recovery.advance_replay(1);
        recovery.complete(0).unwrap();

        assert!(matches!(
            recovery.state(),
            RecoveryState::Complete {
                recovered: 1,
                orphaned: 0
            }
        ));
    }

    #[test]
    fn test_is_in_progress() {
        let mut recovery = CrashRecovery::new(RecoveryConfig::default_config());

        assert!(!recovery.state().is_in_progress());

        recovery.begin_scan().unwrap();
        assert!(recovery.state().is_in_progress());

        recovery.begin_replay(10).unwrap();
        assert!(recovery.state().is_in_progress());

        recovery.complete(0).unwrap();
        assert!(!recovery.state().is_in_progress());
    }

    #[test]
    fn test_is_complete() {
        let mut recovery = CrashRecovery::new(RecoveryConfig::default_config());

        assert!(!recovery.state().is_complete());

        recovery.begin_scan().unwrap();
        recovery.begin_replay(1).unwrap();
        assert!(!recovery.state().is_complete());

        recovery.complete(0).unwrap();
        assert!(recovery.state().is_complete());

        recovery.reset();
        recovery.fail("error".to_string());
        assert!(recovery.state().is_complete());
    }
}
