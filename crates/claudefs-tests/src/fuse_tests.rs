//! FUSE client tests for ClaudeFS
//!
//! Tests for claudefs-fuse crate: error types, cache configuration, and file locking.

use claudefs_fuse::cache::{CacheConfig, MetadataCache};
use claudefs_fuse::error::FuseError;
use claudefs_fuse::inode::InodeId;
use claudefs_fuse::locking::{LockManager, LockRecord, LockType};

fn make_inode_id(val: u64) -> InodeId {
    val
}

#[test]
fn test_not_found_error_display() {
    let err = FuseError::NotFound { ino: 42 };
    let msg = err.to_string();
    assert!(msg.contains("42"));
}

#[test]
fn test_permission_denied_error_display() {
    let err = FuseError::PermissionDenied {
        ino: 1,
        op: "write".to_string(),
    };
    let msg = err.to_string();
    assert!(msg.contains("1") || msg.contains("write"));
}

#[test]
fn test_already_exists_error_display() {
    let err = FuseError::AlreadyExists {
        name: "test.txt".to_string(),
    };
    let msg = err.to_string();
    assert!(msg.contains("test.txt"));
}

#[test]
fn test_mount_failed_error_display() {
    let err = FuseError::MountFailed {
        mountpoint: "/mnt/cfs".to_string(),
        reason: "device not found".to_string(),
    };
    let msg = err.to_string();
    assert!(msg.contains("/mnt/cfs"));
    assert!(msg.contains("device not found"));
}

#[test]
fn test_not_supported_error_display() {
    let err = FuseError::NotSupported {
        op: "flock".to_string(),
    };
    let msg = err.to_string();
    assert!(msg.contains("flock"));
}

#[test]
fn test_not_found_errno() {
    let err = FuseError::NotFound { ino: 42 };
    assert_eq!(err.to_errno(), libc::ENOENT);
}

#[test]
fn test_permission_denied_errno() {
    let err = FuseError::PermissionDenied {
        ino: 1,
        op: "read".to_string(),
    };
    assert_eq!(err.to_errno(), libc::EACCES);
}

#[test]
fn test_is_directory_errno() {
    let err = FuseError::IsDirectory { ino: 3 };
    assert_eq!(err.to_errno(), libc::EISDIR);
}

#[test]
fn test_cache_config_default_values() {
    let config = CacheConfig::default();
    assert_eq!(config.capacity, 10_000);
    assert_eq!(config.ttl_secs, 30);
    assert_eq!(config.negative_ttl_secs, 5);
}

#[test]
fn test_cache_config_custom_values() {
    let config = CacheConfig {
        capacity: 50_000,
        ttl_secs: 60,
        negative_ttl_secs: 10,
    };
    assert_eq!(config.capacity, 50_000);
    assert_eq!(config.ttl_secs, 60);
    assert_eq!(config.negative_ttl_secs, 10);
}

#[test]
fn test_metadata_cache_new() {
    let config = CacheConfig::default();
    let cache = MetadataCache::new(config);
    assert!(cache.is_empty());
}

#[test]
fn test_metadata_cache_default() {
    let cache = MetadataCache::default();
    assert!(cache.is_empty());
}

#[test]
fn test_lock_manager_new() {
    let mgr = LockManager::new();
    assert_eq!(mgr.total_locks(), 0);
}

#[test]
fn test_lock_manager_shared_locks_no_conflict() {
    let mut mgr = LockManager::new();
    let ino = make_inode_id(1);

    let lock1 = LockRecord {
        lock_type: LockType::Shared,
        owner: 1,
        pid: 100,
        start: 0,
        end: u64::MAX,
    };
    let lock2 = LockRecord {
        lock_type: LockType::Shared,
        owner: 2,
        pid: 101,
        start: 0,
        end: u64::MAX,
    };

    assert!(mgr.try_lock(ino, lock1).unwrap());
    assert!(mgr.try_lock(ino, lock2).unwrap());
}

#[test]
fn test_lock_manager_exclusive_conflicts_with_shared() {
    let mut mgr = LockManager::new();
    let ino = make_inode_id(1);

    let shared = LockRecord {
        lock_type: LockType::Shared,
        owner: 1,
        pid: 100,
        start: 0,
        end: u64::MAX,
    };
    let exclusive = LockRecord {
        lock_type: LockType::Exclusive,
        owner: 2,
        pid: 101,
        start: 0,
        end: u64::MAX,
    };

    assert!(mgr.try_lock(ino, shared).unwrap());
    assert!(!mgr.try_lock(ino, exclusive).unwrap());
}

#[test]
fn test_lock_manager_exclusive_conflicts_with_exclusive() {
    let mut mgr = LockManager::new();
    let ino = make_inode_id(1);

    let lock1 = LockRecord {
        lock_type: LockType::Exclusive,
        owner: 1,
        pid: 100,
        start: 0,
        end: u64::MAX,
    };
    let lock2 = LockRecord {
        lock_type: LockType::Exclusive,
        owner: 2,
        pid: 101,
        start: 0,
        end: u64::MAX,
    };

    assert!(mgr.try_lock(ino, lock1).unwrap());
    assert!(!mgr.try_lock(ino, lock2).unwrap());
}

#[test]
fn test_lock_manager_unlock_removes_lock() {
    let mut mgr = LockManager::new();
    let ino = make_inode_id(1);

    let lock = LockRecord {
        lock_type: LockType::Exclusive,
        owner: 1,
        pid: 100,
        start: 0,
        end: u64::MAX,
    };
    assert!(mgr.try_lock(ino, lock).unwrap());
    assert_eq!(mgr.lock_count(ino), 1);

    mgr.unlock(ino, 1);
    assert_eq!(mgr.lock_count(ino), 0);
}

#[test]
fn test_lock_manager_different_inodes_no_interference() {
    let mut mgr = LockManager::new();
    let ino1 = make_inode_id(1);
    let ino2 = make_inode_id(2);

    let lock1 = LockRecord {
        lock_type: LockType::Exclusive,
        owner: 1,
        pid: 100,
        start: 0,
        end: u64::MAX,
    };
    let lock2 = LockRecord {
        lock_type: LockType::Exclusive,
        owner: 2,
        pid: 101,
        start: 0,
        end: u64::MAX,
    };

    assert!(mgr.try_lock(ino1, lock1).unwrap());
    assert!(mgr.try_lock(ino2, lock2).unwrap());
}

#[test]
fn test_lock_manager_byte_range_no_overlap_no_conflict() {
    let mut mgr = LockManager::new();
    let ino = make_inode_id(1);

    let lock1 = LockRecord {
        lock_type: LockType::Exclusive,
        owner: 1,
        pid: 100,
        start: 0,
        end: 100,
    };
    let lock2 = LockRecord {
        lock_type: LockType::Exclusive,
        owner: 2,
        pid: 101,
        start: 200,
        end: 300,
    };

    assert!(mgr.try_lock(ino, lock1).unwrap());
    assert!(mgr.try_lock(ino, lock2).unwrap());
}

#[test]
fn test_lock_manager_byte_range_overlap_conflicts() {
    let mut mgr = LockManager::new();
    let ino = make_inode_id(1);

    let lock1 = LockRecord {
        lock_type: LockType::Exclusive,
        owner: 1,
        pid: 100,
        start: 0,
        end: 200,
    };
    let lock2 = LockRecord {
        lock_type: LockType::Exclusive,
        owner: 2,
        pid: 101,
        start: 100,
        end: 300,
    };

    assert!(mgr.try_lock(ino, lock1).unwrap());
    assert!(!mgr.try_lock(ino, lock2).unwrap());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fuse_error_not_found() {
        let err = FuseError::NotFound { ino: 123 };
        let msg = err.to_string();
        assert!(msg.contains("123"));
    }

    #[test]
    fn test_fuse_error_permission_denied() {
        let err = FuseError::PermissionDenied {
            ino: 456,
            op: "read".to_string(),
        };
        assert_eq!(err.to_errno(), libc::EACCES);
    }

    #[test]
    fn test_fuse_error_already_exists() {
        let err = FuseError::AlreadyExists {
            name: "foo".to_string(),
        };
        assert_eq!(err.to_errno(), libc::EEXIST);
    }

    #[test]
    fn test_fuse_error_not_supported() {
        let err = FuseError::NotSupported {
            op: "xattr".to_string(),
        };
        assert_eq!(err.to_errno(), libc::ENOSYS);
    }

    #[test]
    fn test_cache_config_capacity() {
        let config = CacheConfig {
            capacity: 5000,
            ttl_secs: 120,
            negative_ttl_secs: 30,
        };
        assert_eq!(config.capacity, 5000);
    }

    #[test]
    fn test_lock_manager_default() {
        let mgr = LockManager::default();
        assert_eq!(mgr.total_locks(), 0);
    }
}
