//! FUSE crash recovery security tests.

#[cfg(test)]
mod tests {
    use claudefs_fuse::crash_recovery::{
        CrashRecovery, OpenFileRecord, PendingWrite, RecoveryConfig, RecoveryJournal, RecoveryState,
    };

    #[test]
    fn test_fuse_cr_sec_initial_state_is_idle() {
        let config = RecoveryConfig::default_config();
        let recovery = CrashRecovery::new(config);
        assert!(matches!(recovery.state(), RecoveryState::Idle));
    }

    #[test]
    fn test_fuse_cr_sec_begin_scan_from_idle_transitions_to_scanning() {
        let config = RecoveryConfig::default_config();
        let mut recovery = CrashRecovery::new(config);
        let result = recovery.begin_scan();
        assert!(result.is_ok());
        assert!(matches!(recovery.state(), RecoveryState::Scanning));
    }

    #[test]
    fn test_fuse_cr_sec_begin_scan_from_non_idle_returns_error() {
        let config = RecoveryConfig::default_config();
        let mut recovery = CrashRecovery::new(config);
        recovery.begin_scan().unwrap();
        let result = recovery.begin_scan();
        assert!(result.is_err());
        assert!(matches!(recovery.state(), RecoveryState::Scanning));
    }

    #[test]
    fn test_fuse_cr_sec_state_machine_follows_idle_scanning_replaying_complete() {
        let config = RecoveryConfig::default_config();
        let mut recovery = CrashRecovery::new(config);
        assert!(matches!(recovery.state(), RecoveryState::Idle));
        recovery.begin_scan().unwrap();
        assert!(matches!(recovery.state(), RecoveryState::Scanning));
        recovery.begin_replay(10).unwrap();
        assert!(matches!(recovery.state(), RecoveryState::Replaying { .. }));
        recovery.complete(0).unwrap();
        assert!(matches!(recovery.state(), RecoveryState::Complete { .. }));
    }

    #[test]
    fn test_fuse_cr_sec_fail_can_be_called_from_any_state() {
        let config = RecoveryConfig::default_config();
        let mut recovery = CrashRecovery::new(config);
        recovery.fail("test failure from idle".to_string());
        assert!(matches!(recovery.state(), RecoveryState::Failed(_)));

        let config2 = RecoveryConfig::default_config();
        let mut recovery2 = CrashRecovery::new(config2);
        recovery2.begin_scan().unwrap();
        recovery2.fail("test failure from scanning".to_string());
        assert!(matches!(recovery2.state(), RecoveryState::Failed(_)));

        let config3 = RecoveryConfig::default_config();
        let mut recovery3 = CrashRecovery::new(config3);
        recovery3.begin_scan().unwrap();
        recovery3.begin_replay(5).unwrap();
        recovery3.fail("test failure from replaying".to_string());
        assert!(matches!(recovery3.state(), RecoveryState::Failed(_)));
    }

    #[test]
    fn test_fuse_cr_sec_open_file_record_o_rdonly_not_writable() {
        let record = OpenFileRecord {
            ino: 1,
            fd: 10,
            pid: 100,
            flags: 0,
            path_hint: "/file".to_string(),
        };
        assert!(!record.is_writable());
    }

    #[test]
    fn test_fuse_cr_sec_open_file_record_o_wronly_is_writable() {
        let record = OpenFileRecord {
            ino: 1,
            fd: 10,
            pid: 100,
            flags: 1,
            path_hint: "/file".to_string(),
        };
        assert!(record.is_writable());
    }

    #[test]
    fn test_fuse_cr_sec_open_file_record_o_rdwr_is_writable() {
        let record = OpenFileRecord {
            ino: 1,
            fd: 10,
            pid: 100,
            flags: 2,
            path_hint: "/file".to_string(),
        };
        assert!(record.is_writable());
    }

    #[test]
    fn test_fuse_cr_sec_open_file_record_o_append_is_append_only() {
        let record = OpenFileRecord {
            ino: 1,
            fd: 10,
            pid: 100,
            flags: 1024,
            path_hint: "/file".to_string(),
        };
        assert!(record.is_append_only());
    }

    #[test]
    fn test_fuse_cr_sec_pending_write_age_secs_correct_when_now_greater() {
        let write = PendingWrite {
            ino: 1,
            offset: 0,
            len: 4096,
            dirty_since_secs: 100,
        };
        let age = write.age_secs(200);
        assert_eq!(age, 100);
    }

    #[test]
    fn test_fuse_cr_sec_pending_write_age_secs_saturates_when_now_less() {
        let write = PendingWrite {
            ino: 1,
            offset: 0,
            len: 4096,
            dirty_since_secs: 200,
        };
        let age = write.age_secs(100);
        assert_eq!(age, 0);
    }

    #[test]
    fn test_fuse_cr_sec_pending_write_is_stale_true_when_age_exceeds_max() {
        let write = PendingWrite {
            ino: 1,
            offset: 0,
            len: 4096,
            dirty_since_secs: 100,
        };
        assert!(write.is_stale(500, 300));
    }

    #[test]
    fn test_fuse_cr_sec_pending_write_is_stale_false_when_age_within_max() {
        let write = PendingWrite {
            ino: 1,
            offset: 0,
            len: 4096,
            dirty_since_secs: 400,
        };
        assert!(!write.is_stale(500, 300));
    }

    #[test]
    fn test_fuse_cr_sec_recovery_journal_new_has_zero_counts() {
        let journal = RecoveryJournal::new();
        assert_eq!(journal.open_file_count(), 0);
        assert_eq!(journal.pending_write_count(), 0);
    }

    #[test]
    fn test_fuse_cr_sec_recovery_journal_writable_open_files_filters_read_only() {
        let mut journal = RecoveryJournal::new();
        let writable = OpenFileRecord {
            ino: 1,
            fd: 10,
            pid: 100,
            flags: 2,
            path_hint: "/file1".to_string(),
        };
        let readonly = OpenFileRecord {
            ino: 2,
            fd: 11,
            pid: 100,
            flags: 0,
            path_hint: "/file2".to_string(),
        };
        journal.add_open_file(writable);
        journal.add_open_file(readonly);
        let writable_files = journal.writable_open_files();
        assert_eq!(writable_files.len(), 1);
        assert_eq!(writable_files[0].ino, 1);
    }

    #[test]
    fn test_fuse_cr_sec_recovery_journal_stale_pending_writes_filters_by_age() {
        let mut journal = RecoveryJournal::new();
        let stale_write = PendingWrite {
            ino: 1,
            offset: 0,
            len: 4096,
            dirty_since_secs: 100,
        };
        let fresh_write = PendingWrite {
            ino: 2,
            offset: 0,
            len: 4096,
            dirty_since_secs: 450,
        };
        journal.add_pending_write(stale_write);
        journal.add_pending_write(fresh_write);
        let stale = journal.stale_pending_writes(500, 300);
        assert_eq!(stale.len(), 1);
        assert_eq!(stale[0].ino, 1);
    }

    #[test]
    fn test_fuse_cr_sec_recovery_journal_mixed_writable_readonly_counted_correctly() {
        let mut journal = RecoveryJournal::new();
        for i in 0..5u64 {
            journal.add_open_file(OpenFileRecord {
                ino: i,
                fd: i,
                pid: 100,
                flags: 2,
                path_hint: format!("/file{}", i),
            });
        }
        for i in 5..10u64 {
            journal.add_open_file(OpenFileRecord {
                ino: i,
                fd: i,
                pid: 100,
                flags: 0,
                path_hint: format!("/file{}", i),
            });
        }
        assert_eq!(journal.open_file_count(), 10);
        assert_eq!(journal.writable_open_files().len(), 5);
    }

    #[test]
    fn test_fuse_cr_sec_recovery_journal_pending_write_count_tracks_additions() {
        let mut journal = RecoveryJournal::new();
        assert_eq!(journal.pending_write_count(), 0);
        journal.add_pending_write(PendingWrite {
            ino: 1,
            offset: 0,
            len: 4096,
            dirty_since_secs: 100,
        });
        assert_eq!(journal.pending_write_count(), 1);
        journal.add_pending_write(PendingWrite {
            ino: 2,
            offset: 4096,
            len: 4096,
            dirty_since_secs: 100,
        });
        assert_eq!(journal.pending_write_count(), 2);
    }

    #[test]
    fn test_fuse_cr_sec_record_open_file_errors_if_not_scanning() {
        let config = RecoveryConfig::default_config();
        let mut recovery = CrashRecovery::new(config);
        let record = OpenFileRecord {
            ino: 1,
            fd: 10,
            pid: 100,
            flags: 2,
            path_hint: "/file".to_string(),
        };
        let result = recovery.record_open_file(record);
        assert!(result.is_err());
    }

    #[test]
    fn test_fuse_cr_sec_record_pending_write_errors_if_not_scanning() {
        let config = RecoveryConfig::default_config();
        let mut recovery = CrashRecovery::new(config);
        let write = PendingWrite {
            ino: 1,
            offset: 0,
            len: 4096,
            dirty_since_secs: 100,
        };
        let result = recovery.record_pending_write(write);
        assert!(result.is_err());
    }

    #[test]
    fn test_fuse_cr_sec_begin_replay_errors_if_not_scanning() {
        let config = RecoveryConfig::default_config();
        let mut recovery = CrashRecovery::new(config);
        let result = recovery.begin_replay(10);
        assert!(result.is_err());
    }

    #[test]
    fn test_fuse_cr_sec_complete_errors_if_not_replaying() {
        let config = RecoveryConfig::default_config();
        let mut recovery = CrashRecovery::new(config);
        let result = recovery.complete(0);
        assert!(result.is_err());
    }

    #[test]
    fn test_fuse_cr_sec_max_open_files_limit_enforced() {
        let mut config = RecoveryConfig::default_config();
        config.max_open_files = 2;
        let mut recovery = CrashRecovery::new(config);
        recovery.begin_scan().unwrap();

        let ok1 = recovery.record_open_file(OpenFileRecord {
            ino: 1,
            fd: 10,
            pid: 100,
            flags: 0,
            path_hint: "/file1".to_string(),
        });
        assert!(ok1.is_ok());

        let ok2 = recovery.record_open_file(OpenFileRecord {
            ino: 2,
            fd: 11,
            pid: 100,
            flags: 0,
            path_hint: "/file2".to_string(),
        });
        assert!(ok2.is_ok());

        let fail = recovery.record_open_file(OpenFileRecord {
            ino: 3,
            fd: 12,
            pid: 100,
            flags: 0,
            path_hint: "/file3".to_string(),
        });
        assert!(fail.is_err());
    }

    #[test]
    fn test_fuse_cr_sec_full_happy_path_idle_scan_replay_complete() {
        let config = RecoveryConfig::default_config();
        let mut recovery = CrashRecovery::new(config);
        assert!(matches!(recovery.state(), RecoveryState::Idle));

        recovery.begin_scan().unwrap();
        assert!(matches!(recovery.state(), RecoveryState::Scanning));

        recovery
            .record_open_file(OpenFileRecord {
                ino: 1,
                fd: 10,
                pid: 100,
                flags: 2,
                path_hint: "/file".to_string(),
            })
            .unwrap();

        recovery.begin_replay(5).unwrap();
        if let RecoveryState::Replaying { total, .. } = recovery.state() {
            assert_eq!(*total, 5);
        } else {
            panic!("Expected Replaying state");
        }

        recovery.advance_replay(3);
        recovery.advance_replay(2);
        recovery.complete(0).unwrap();
        assert!(matches!(recovery.state(), RecoveryState::Complete { .. }));
    }

    #[test]
    fn test_fuse_cr_sec_advance_replay_clamps_at_total() {
        let config = RecoveryConfig::default_config();
        let mut recovery = CrashRecovery::new(config);
        recovery.begin_scan().unwrap();
        recovery.begin_replay(5).unwrap();

        recovery.advance_replay(10);

        if let RecoveryState::Replaying { replayed, total } = recovery.state() {
            assert_eq!(*replayed, 5);
            assert_eq!(*total, 5);
        } else {
            panic!("Expected Replaying state");
        }
    }

    #[test]
    fn test_fuse_cr_sec_reset_clears_journal_and_returns_to_idle() {
        let config = RecoveryConfig::default_config();
        let mut recovery = CrashRecovery::new(config);
        recovery.begin_scan().unwrap();
        recovery
            .record_open_file(OpenFileRecord {
                ino: 1,
                fd: 10,
                pid: 100,
                flags: 2,
                path_hint: "/file".to_string(),
            })
            .unwrap();
        recovery
            .record_pending_write(PendingWrite {
                ino: 1,
                offset: 0,
                len: 4096,
                dirty_since_secs: 100,
            })
            .unwrap();
        assert_eq!(recovery.journal().open_file_count(), 1);
        assert_eq!(recovery.journal().pending_write_count(), 1);

        recovery.reset();
        assert!(matches!(recovery.state(), RecoveryState::Idle));
        assert_eq!(recovery.journal().open_file_count(), 0);
        assert_eq!(recovery.journal().pending_write_count(), 0);
    }

    #[test]
    fn test_fuse_cr_sec_after_fail_state_is_failed_with_reason() {
        let config = RecoveryConfig::default_config();
        let mut recovery = CrashRecovery::new(config);
        recovery.fail("disk read error".to_string());

        if let RecoveryState::Failed(reason) = recovery.state() {
            assert_eq!(reason, "disk read error");
        } else {
            panic!("Expected Failed state");
        }
    }

    #[test]
    fn test_fuse_cr_sec_after_complete_recovered_count_matches_advanced_replayed() {
        let config = RecoveryConfig::default_config();
        let mut recovery = CrashRecovery::new(config);
        recovery.begin_scan().unwrap();
        recovery.begin_replay(10).unwrap();

        recovery.advance_replay(4);
        recovery.advance_replay(3);
        recovery.complete(2).unwrap();

        if let RecoveryState::Complete {
            recovered,
            orphaned,
        } = recovery.state()
        {
            assert_eq!(*recovered, 7);
            assert_eq!(*orphaned, 2);
        } else {
            panic!("Expected Complete state");
        }
    }
}
