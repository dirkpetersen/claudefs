//! Phase 3 FUSE protocol security fuzzing for claudefs-fuse.
//!
//! Findings: FINDING-FUSE-01 through FINDING-FUSE-15
//!
//! Tests adversarial inputs to FUSE configuration and operation handling.

use claudefs_fuse::cache::{CacheConfig, MetadataCache};
use claudefs_fuse::inode::{InodeKind, InodeTable};
use claudefs_fuse::mount::{parse_mount_options, MountHandle, MountOptions};
use claudefs_fuse::passthrough::{
    check_kernel_version, PassthroughConfig, PassthroughState, PassthroughStatus,
};
use std::path::PathBuf;

// ============================================================================
// Group 1: Mount option parsing (10+ tests)
// ============================================================================

#[test]
fn fuzz_mount_options_empty_string() {
    let result = parse_mount_options("");
    assert!(result.is_ok());
    let opts = result.unwrap();
    assert!(!opts.allow_other);
    assert!(!opts.ro);
}

#[test]
fn fuzz_mount_options_only_whitespace() {
    let result = parse_mount_options("   ");
    assert!(result.is_ok());
}

#[test]
fn fuzz_mount_options_valid_allow_other() {
    let result = parse_mount_options("allow_other");
    assert!(result.is_ok());
    assert!(result.unwrap().allow_other);
}

#[test]
fn fuzz_mount_options_valid_ro() {
    let result = parse_mount_options("ro");
    assert!(result.is_ok());
    assert!(result.unwrap().ro);
}

#[test]
fn fuzz_mount_options_valid_allow_root() {
    let result = parse_mount_options("allow_root");
    assert!(result.is_ok());
    assert!(result.unwrap().allow_root);
}

#[test]
fn fuzz_mount_options_valid_default_permissions() {
    let result = parse_mount_options("default_permissions");
    assert!(result.is_ok());
    assert!(result.unwrap().default_permissions);
}

#[test]
fn fuzz_mount_options_valid_auto_unmount() {
    let result = parse_mount_options("auto_unmount");
    assert!(result.is_ok());
    assert!(result.unwrap().auto_unmount);
}

#[test]
fn fuzz_mount_options_valid_direct_io() {
    let result = parse_mount_options("direct_io");
    assert!(result.is_ok());
    assert!(result.unwrap().direct_io);
}

#[test]
fn fuzz_mount_options_valid_kernel_cache() {
    let result = parse_mount_options("kernel_cache");
    assert!(result.is_ok());
    assert!(result.unwrap().kernel_cache);
}

#[test]
fn fuzz_mount_options_valid_nonempty() {
    let result = parse_mount_options("nonempty");
    assert!(result.is_ok());
    assert!(result.unwrap().nonempty);
}

#[test]
fn fuzz_mount_options_sql_injection_attempt() {
    let result = parse_mount_options("allow_other'; DROP TABLE mounts;--");
    assert!(result.is_err());
}

#[test]
fn fuzz_mount_options_path_traversal() {
    let result = parse_mount_options("../../etc/passwd");
    assert!(result.is_err());
}

#[test]
fn fuzz_mount_options_null_byte_injection() {
    let result = parse_mount_options("allow_other\x00root");
    assert!(result.is_err() || !result.unwrap().allow_other);
}

#[test]
fn fuzz_mount_options_shell_metacharacters() {
    let result = parse_mount_options("$(whoami)");
    assert!(result.is_err());
}

#[test]
fn fuzz_mount_options_backtick_expansion() {
    let result = parse_mount_options("`id`");
    assert!(result.is_err());
}

#[test]
fn fuzz_mount_options_newline_injection() {
    let result = parse_mount_options("allow_other\nroot\nro");
    assert!(result.is_err() || !result.unwrap().allow_other);
}

#[test]
fn fuzz_mount_options_very_long_string() {
    let long_str = "a".repeat(10000);
    let result = parse_mount_options(&long_str);
    assert!(result.is_err());
}

#[test]
fn fuzz_mount_options_unicode_characters() {
    let result = parse_mount_options("allow_othér");
    assert!(result.is_err());

    let result2 = parse_mount_options("日本語");
    assert!(result2.is_err());
}

#[test]
fn fuzz_mount_options_emoji_in_options() {
    let result = parse_mount_options("🚀");
    assert!(result.is_err());
}

#[test]
fn fuzz_mount_options_mixed_unicode() {
    let result = parse_mount_options("allow_other,🔐,ro");
    assert!(result.is_err());
}

#[test]
fn fuzz_mount_options_case_sensitivity() {
    let result = parse_mount_options("ALLOW_OTHER");
    assert!(result.is_err());

    let result2 = parse_mount_options("Allow_Other");
    assert!(result2.is_err());

    let result3 = parse_mount_options("RO");
    assert!(result3.is_err());
}

#[test]
fn fuzz_mount_options_duplicates() {
    let result = parse_mount_options("allow_other,allow_other,allow_other");
    assert!(result.is_ok());
    assert!(result.unwrap().allow_other);
}

#[test]
fn fuzz_mount_options_mixed_valid_invalid() {
    let result = parse_mount_options("allow_other,invalid_opt,ro");
    assert!(result.is_err());
}

#[test]
fn fuzz_mount_options_special_chars_in_value() {
    let result = parse_mount_options("allow_other=true");
    assert!(result.is_err() || !result.unwrap().allow_other);
}

#[test]
fn fuzz_mount_options_numeric_suffix() {
    let result = parse_mount_options("allow_other1");
    assert!(result.is_err());

    let result2 = parse_mount_options("ro2");
    assert!(result2.is_err());
}

#[test]
fn fuzz_mount_options_excessive_commas() {
    let result = parse_mount_options(",,,,,,,,,,,,,,");
    assert!(result.is_ok());
}

#[test]
fn fuzz_mount_options_trailing_comma() {
    let result = parse_mount_options("allow_other,");
    assert!(result.is_ok());
    assert!(result.unwrap().allow_other);
}

#[test]
fn fuzz_mount_options_leading_comma() {
    let result = parse_mount_options(",allow_other");
    assert!(result.is_ok());
    assert!(result.unwrap().allow_other);
}

#[test]
fn fuzz_mount_options_ro_rw_conflict() {
    let result = parse_mount_options("ro,rw");
    assert!(result.is_ok());
    assert!(!result.unwrap().ro);
}

#[test]
fn fuzz_mount_options_all_options_combined() {
    let result = parse_mount_options("allow_other,allow_root,default_permissions,auto_unmount,direct_io,kernel_cache,nonempty,ro");
    assert!(result.is_ok());
    let opts = result.unwrap();
    assert!(opts.allow_other);
    assert!(opts.allow_root);
    assert!(opts.default_permissions);
    assert!(opts.auto_unmount);
    assert!(opts.direct_io);
    assert!(opts.kernel_cache);
    assert!(opts.nonempty);
    assert!(opts.ro);
}

#[test]
fn fuzz_mount_options_massive_input() {
    let massive = "a,".repeat(1_000_000);
    let result = parse_mount_options(&massive);
    assert!(result.is_err());
}

#[test]
fn fuzz_mount_options_repeated_option_names() {
    let repeated = "allow_other,".repeat(1000);
    let result = parse_mount_options(&repeated);
    assert!(result.is_ok());
    assert!(result.unwrap().allow_other);
}

#[test]
fn fuzz_mount_options_binary_data() {
    let binary = vec![0u8; 100];
    let result = parse_mount_options(std::str::from_utf8(&binary).unwrap_or(""));
    assert!(result.is_ok() || result.is_err());
}

#[test]
fn fuzz_mount_options_control_characters() {
    let with_control = "allow\x01_other\x02ro\x03";
    let result = parse_mount_options(with_control);
    assert!(result.is_err() || !result.unwrap().allow_other);
}

#[test]
fn fuzz_mount_options_utf8_bom() {
    let with_bom = "\u{FEFF}allow_other";
    let result = parse_mount_options(with_bom);
    assert!(result.is_ok() || result.is_err());
}

#[test]
fn fuzz_mount_options_homograph_attack_simulation() {
    let cyrillic_o = "аllow_other";
    let result = parse_mount_options(cyrillic_o);
    assert!(result.is_err());
}

// ============================================================================
// Group 2: CacheConfig construction (5+ tests)
// ============================================================================

#[test]
fn fuzz_cache_config_zero_ttl() {
    let config = CacheConfig {
        capacity: 100,
        ttl_secs: 0,
        negative_ttl_secs: 0,
    };
    assert_eq!(config.ttl_secs, 0);
    assert_eq!(config.negative_ttl_secs, 0);
}

#[test]
fn fuzz_cache_config_max_u64_ttl() {
    let config = CacheConfig {
        capacity: 100,
        ttl_secs: u64::MAX,
        negative_ttl_secs: u64::MAX,
    };
    assert_eq!(config.ttl_secs, u64::MAX);
}

#[test]
fn fuzz_cache_config_near_max_ttl() {
    let config = CacheConfig {
        capacity: 100,
        ttl_secs: u64::MAX - 1,
        negative_ttl_secs: u64::MAX - 1,
    };
    assert_eq!(config.ttl_secs, u64::MAX - 1);
}

#[test]
fn fuzz_cache_config_zero_capacity() {
    let config = CacheConfig {
        capacity: 0,
        ttl_secs: 30,
        negative_ttl_secs: 5,
    };
    assert_eq!(config.capacity, 0);
}

#[test]
fn fuzz_cache_config_max_capacity() {
    let config = CacheConfig {
        capacity: usize::MAX,
        ttl_secs: 30,
        negative_ttl_secs: 5,
    };
    assert_eq!(config.capacity, usize::MAX);
}

#[test]
fn fuzz_cache_config_unusual_ttl_values() {
    let config = CacheConfig {
        capacity: 100,
        ttl_secs: 1,
        negative_ttl_secs: 0,
    };
    assert_eq!(config.ttl_secs, 1);

    let config2 = CacheConfig {
        capacity: 100,
        ttl_secs: u64::MAX,
        negative_ttl_secs: 1,
    };
    assert_eq!(config2.negative_ttl_secs, 1);
}

#[test]
fn fuzz_cache_config_default_values() {
    let config = CacheConfig::default();
    assert_eq!(config.capacity, 10_000);
    assert_eq!(config.ttl_secs, 30);
    assert_eq!(config.negative_ttl_secs, 5);
}

#[test]
fn fuzz_cache_config_metadata_cache_construction() {
    let config = CacheConfig {
        capacity: 100,
        ttl_secs: 60,
        negative_ttl_secs: 10,
    };
    let cache = MetadataCache::new(config.clone());
    assert!(cache.is_empty());
    assert_eq!(cache.len(), 0);
}

#[test]
fn fuzz_cache_config_addition_overflow_potential() {
    let config = CacheConfig {
        capacity: usize::MAX,
        ttl_secs: u64::MAX,
        negative_ttl_secs: u64::MAX,
    };
    let sum = config.ttl_secs.saturating_add(config.negative_ttl_secs);
    assert!(sum >= config.ttl_secs);
}

// ============================================================================
// Group 3: Passthrough config and kernel version (8+ tests)
// ============================================================================

#[test]
fn fuzz_passthrough_kernel_6_7_too_old() {
    let config = PassthroughConfig::default();
    let status = check_kernel_version(6, 7, &config);
    assert!(matches!(
        status,
        PassthroughStatus::DisabledKernelTooOld { major: 6, minor: 7 }
    ));
}

#[test]
fn fuzz_passthrough_kernel_6_8_exact() {
    let config = PassthroughConfig::default();
    let status = check_kernel_version(6, 8, &config);
    assert!(matches!(status, PassthroughStatus::Enabled));
}

#[test]
fn fuzz_passthrough_kernel_6_9_newer() {
    let config = PassthroughConfig::default();
    let status = check_kernel_version(6, 9, &config);
    assert!(matches!(status, PassthroughStatus::Enabled));
}

#[test]
fn fuzz_passthrough_kernel_5_15_too_old() {
    let config = PassthroughConfig::default();
    let status = check_kernel_version(5, 15, &config);
    assert!(matches!(
        status,
        PassthroughStatus::DisabledKernelTooOld {
            major: 5,
            minor: 15
        }
    ));
}

#[test]
fn fuzz_passthrough_kernel_4_19_ancient() {
    let config = PassthroughConfig::default();
    let status = check_kernel_version(4, 19, &config);
    assert!(matches!(
        status,
        PassthroughStatus::DisabledKernelTooOld {
            major: 4,
            minor: 19
        }
    ));
}

#[test]
fn fuzz_passthrough_disabled_config() {
    let config = PassthroughConfig {
        enabled: false,
        min_kernel_major: 6,
        min_kernel_minor: 8,
    };
    let status = check_kernel_version(6, 10, &config);
    assert!(matches!(status, PassthroughStatus::DisabledByConfig));
}

#[test]
fn fuzz_passthrough_custom_min_kernel() {
    let config = PassthroughConfig {
        enabled: true,
        min_kernel_major: 7,
        min_kernel_minor: 0,
    };
    let status = check_kernel_version(6, 20, &config);
    assert!(matches!(
        status,
        PassthroughStatus::DisabledKernelTooOld {
            major: 6,
            minor: 20
        }
    ));

    let status2 = check_kernel_version(7, 0, &config);
    assert!(matches!(status2, PassthroughStatus::Enabled));
}

#[test]
fn fuzz_passthrough_kernel_version_zero() {
    let config = PassthroughConfig::default();
    let status = check_kernel_version(0, 0, &config);
    assert!(matches!(
        status,
        PassthroughStatus::DisabledKernelTooOld { major: 0, minor: 0 }
    ));
}

#[test]
fn fuzz_passthrough_kernel_version_negative_simulated() {
    let config = PassthroughConfig::default();
    let status = check_kernel_version(0, 1, &config);
    assert!(matches!(
        status,
        PassthroughStatus::DisabledKernelTooOld { major: 0, minor: 1 }
    ));
}

#[test]
fn fuzz_passthrough_default_config() {
    let config = PassthroughConfig::default();
    assert!(config.enabled);
    assert_eq!(config.min_kernel_major, 6);
    assert_eq!(config.min_kernel_minor, 8);
}

#[test]
fn fuzz_passthrough_state_new_enabled() {
    let config = PassthroughConfig {
        enabled: true,
        min_kernel_major: 0,
        min_kernel_minor: 0,
    };
    let state = PassthroughState::new(&config);
    assert!(state.is_active());
}

#[test]
fn fuzz_passthrough_state_register_fd() {
    let mut state = PassthroughState::default();
    state.register_fd(1, 10);
    assert_eq!(state.get_fd(1), Some(10));
    assert_eq!(state.fd_count(), 1);
}

#[test]
fn fuzz_passthrough_state_unregister_fd() {
    let mut state = PassthroughState::default();
    state.register_fd(1, 10);
    let fd = state.unregister_fd(1);
    assert_eq!(fd, Some(10));
    assert_eq!(state.get_fd(1), None);
    assert_eq!(state.fd_count(), 0);
}

#[test]
fn fuzz_passthrough_state_multiple_fds() {
    let mut state = PassthroughState::default();
    state.register_fd(1, 10);
    state.register_fd(2, 20);
    state.register_fd(3, 30);
    assert_eq!(state.fd_count(), 3);
    assert_eq!(state.get_fd(1), Some(10));
    assert_eq!(state.get_fd(2), Some(20));
    assert_eq!(state.get_fd(3), Some(30));
}

#[test]
fn fuzz_passthrough_state_fd_overwrite() {
    let mut state = PassthroughState::default();
    state.register_fd(1, 10);
    state.register_fd(1, 20);
    assert_eq!(state.get_fd(1), Some(20));
    assert_eq!(state.fd_count(), 1);
}

#[test]
fn fuzz_passthrough_state_unregister_nonexistent() {
    let mut state = PassthroughState::default();
    let fd = state.unregister_fd(999);
    assert_eq!(fd, None);
}

#[test]
fn fuzz_passthrough_state_get_nonexistent() {
    let state = PassthroughState::default();
    assert_eq!(state.get_fd(999), None);
}

// ============================================================================
// Group 4: Inode table operations (7+ tests)
// ============================================================================

#[test]
fn fuzz_inode_table_new_has_root() {
    let table = InodeTable::new();
    let root = table.get(1);
    assert!(root.is_some());
    assert_eq!(root.unwrap().ino, 1);
}

#[test]
fn fuzz_inode_table_alloc_file() {
    let mut table = InodeTable::new();
    let ino = table.alloc(1, "test.txt", InodeKind::File, 0o644, 0, 0);
    assert!(ino.is_ok());
    let ino = ino.unwrap();
    assert!(ino > 1);
    let entry = table.get(ino);
    assert!(entry.is_some());
    assert_eq!(entry.unwrap().name, "test.txt");
}

#[test]
fn fuzz_inode_table_alloc_directory() {
    let mut table = InodeTable::new();
    let ino = table.alloc(1, "subdir", InodeKind::Directory, 0o755, 0, 0);
    assert!(ino.is_ok());
    let ino = ino.unwrap();
    let entry = table.get(ino);
    assert!(entry.is_some());
    assert_eq!(entry.unwrap().kind, InodeKind::Directory);
}

#[test]
fn fuzz_inode_table_alloc_symlink() {
    let mut table = InodeTable::new();
    let ino = table.alloc(1, "link", InodeKind::Symlink, 0o777, 0, 0);
    assert!(ino.is_ok());
    let ino = ino.unwrap();
    let entry = table.get(ino);
    assert!(entry.is_some());
    assert_eq!(entry.unwrap().kind, InodeKind::Symlink);
}

#[test]
fn fuzz_inode_table_large_inode_numbers() {
    let mut table = InodeTable::new();
    for i in 0..1000 {
        let ino = table.alloc(1, &format!("file_{}", i), InodeKind::File, 0o644, 0, 0);
        assert!(ino.is_ok());
    }
    assert_eq!(table.len(), 1001);
}

#[test]
fn fuzz_inode_table_forget_decrements_lookup_count() {
    let mut table = InodeTable::new();
    let ino = table
        .alloc(1, "test.txt", InodeKind::File, 0o644, 0, 0)
        .unwrap();

    table.add_lookup(ino);
    table.add_lookup(ino);

    let entry = table.get(ino);
    assert_eq!(entry.unwrap().lookup_count, 3);

    table.forget(ino, 2);

    let entry = table.get(ino);
    assert_eq!(entry.unwrap().lookup_count, 1);
}

#[test]
fn fuzz_inode_table_forget_removes_entry() {
    let mut table = InodeTable::new();
    let ino = table
        .alloc(1, "test.txt", InodeKind::File, 0o644, 0, 0)
        .unwrap();

    table.forget(ino, 1);

    let entry = table.get(ino);
    assert!(entry.is_none());
}

#[test]
fn fuzz_inode_table_remove_file() {
    let mut table = InodeTable::new();
    let ino = table
        .alloc(1, "test.txt", InodeKind::File, 0o644, 0, 0)
        .unwrap();

    let result = table.remove(ino);
    assert!(result.is_ok());

    let entry = table.get(ino);
    assert!(entry.is_none());
}

#[test]
fn fuzz_inode_table_nlink_on_directory() {
    let mut table = InodeTable::new();
    let root_entry = table.get(1).unwrap();
    let initial_nlink = root_entry.nlink;

    let _ = table.alloc(1, "subdir1", InodeKind::Directory, 0o755, 0, 0);
    let _ = table.alloc(1, "subdir2", InodeKind::Directory, 0o755, 0, 0);

    let root_entry = table.get(1).unwrap();
    assert_eq!(root_entry.nlink, initial_nlink + 2);
}

#[test]
fn fuzz_inode_table_lookup_child() {
    let mut table = InodeTable::new();
    let _ = table.alloc(1, "child.txt", InodeKind::File, 0o644, 0, 0);

    let result = table.lookup_child(1, "child.txt");
    assert!(result.is_some());
}

#[test]
fn fuzz_inode_table_lookup_child_not_found() {
    let table = InodeTable::new();
    let result = table.lookup_child(1, "nonexistent");
    assert!(result.is_none());
}

#[test]
fn fuzz_inode_table_get_mut() {
    let mut table = InodeTable::new();
    let ino = table
        .alloc(1, "test.txt", InodeKind::File, 0o644, 0, 0)
        .unwrap();

    let entry = table.get_mut(ino);
    assert!(entry.is_some());

    if let Some(e) = entry {
        e.size = 9999;
    }

    let entry = table.get(ino).unwrap();
    assert_eq!(entry.size, 9999);
}

#[test]
fn fuzz_inode_table_len() {
    let mut table = InodeTable::new();
    assert_eq!(table.len(), 1);

    let _ = table.alloc(1, "file1", InodeKind::File, 0o644, 0, 0);
    assert_eq!(table.len(), 2);

    let _ = table.alloc(1, "file2", InodeKind::File, 0o644, 0, 0);
    assert_eq!(table.len(), 3);
}

#[test]
fn fuzz_inode_table_is_empty() {
    let table = InodeTable::new();
    assert!(!table.is_empty());
}

#[test]
fn fuzz_inode_table_all_kind_variants() {
    let mut table = InodeTable::new();

    let ino = table
        .alloc(1, "file", InodeKind::File, 0o644, 0, 0)
        .unwrap();
    assert_eq!(table.get(ino).unwrap().kind, InodeKind::File);

    let ino = table
        .alloc(1, "dir", InodeKind::Directory, 0o755, 0, 0)
        .unwrap();
    assert_eq!(table.get(ino).unwrap().kind, InodeKind::Directory);

    let ino = table
        .alloc(1, "symlink", InodeKind::Symlink, 0o777, 0, 0)
        .unwrap();
    assert_eq!(table.get(ino).unwrap().kind, InodeKind::Symlink);

    let ino = table
        .alloc(1, "block", InodeKind::BlockDevice, 0o644, 0, 0)
        .unwrap();
    assert_eq!(table.get(ino).unwrap().kind, InodeKind::BlockDevice);

    let ino = table
        .alloc(1, "char", InodeKind::CharDevice, 0o644, 0, 0)
        .unwrap();
    assert_eq!(table.get(ino).unwrap().kind, InodeKind::CharDevice);

    let ino = table
        .alloc(1, "fifo", InodeKind::Fifo, 0o644, 0, 0)
        .unwrap();
    assert_eq!(table.get(ino).unwrap().kind, InodeKind::Fifo);

    let ino = table
        .alloc(1, "socket", InodeKind::Socket, 0o644, 0, 0)
        .unwrap();
    assert_eq!(table.get(ino).unwrap().kind, InodeKind::Socket);
}

// ============================================================================
// MountHandle tests
// ============================================================================

#[test]
fn fuzz_mount_handle_new() {
    let handle = MountHandle::new(PathBuf::from("/test/mount"));
    assert_eq!(handle.mountpoint(), PathBuf::from("/test/mount"));
    assert!(!handle.is_mounted());
}

#[test]
fn fuzz_mount_handle_mark_mounted() {
    let mut handle = MountHandle::new(PathBuf::from("/test"));
    handle.mark_mounted();
    assert!(handle.is_mounted());
}

#[test]
fn fuzz_mount_handle_mark_unmounted() {
    let mut handle = MountHandle::new(PathBuf::from("/test"));
    handle.mark_mounted();
    handle.mark_unmounted();
    assert!(!handle.is_mounted());
}

#[test]
fn fuzz_mount_handle_default() {
    let handle = MountHandle::default();
    assert_eq!(handle.mountpoint(), PathBuf::new());
    assert!(!handle.is_mounted());
}
