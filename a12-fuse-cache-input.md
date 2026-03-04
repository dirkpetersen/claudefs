# Task: Write fuse_cache_security_tests.rs for claudefs-security crate

Write a comprehensive security test module for the `claudefs-fuse` crate focusing on cache coherence protocols, crash recovery state machine, write buffer integrity, data cache eviction, and session validation.

## File location
`crates/claudefs-security/src/fuse_cache_security_tests.rs`

## Module structure
```rust
//! FUSE cache/recovery security tests: coherence, crash recovery, writebuf, datacache, session.
//!
//! Part of A10 Phase 12: FUSE cache & recovery security audit

#[cfg(test)]
mod tests {
    // imports and tests here
}
```

## Available types (verified from source)

```rust
use claudefs_fuse::cache_coherence::{
    CoherenceProtocol, CoherenceManager, CacheLease, CacheInvalidation,
    LeaseId, LeaseState, InvalidationReason, VersionVector, CoherenceResult,
};
use claudefs_fuse::crash_recovery::{
    CrashRecovery, RecoveryConfig, RecoveryState, RecoveryJournal,
    OpenFileRecord, PendingWrite,
};
use claudefs_fuse::writebuf::{WriteBuf, WriteBufConfig, WriteRange};
use claudefs_fuse::datacache::{DataCache, DataCacheConfig, CachedData, DataCacheStats};
use claudefs_fuse::session::{SessionConfig, SessionStats};
use claudefs_fuse::inode::InodeId;
```

**IMPORTANT**: Not all may be public. If any import fails, remove it and skip those tests.

## Existing tests to AVOID duplicating
- `fuse_security_tests.rs`: basic FUSE ops, permission checks
- `fuse_ext_security_tests.rs`: dir_cache, flock, idmap, interrupt, path resolver, posix ACL
- `fuse_deep_security_tests.rs`: buffer pool, passthrough, capability, mount opts, rate limit, WORM

DO NOT duplicate these. Focus on cache coherence, crash recovery, write buffer, data cache, session.

## Test categories (25 tests total, 5 per category)

### Category 1: Cache Coherence Security (5 tests)

1. **test_coherence_grant_and_check_lease** — Create CoherenceManager with CloseToOpen protocol. Grant lease for inode 1, client 100. Verify check_lease(1) returns Some. Verify active_lease_count() == 1. Verify lease state is Active.

2. **test_coherence_revoke_generates_invalidation** — Create manager. Grant lease for inode 1. Call revoke_lease(1). Verify CacheInvalidation returned. Verify invalidation reason is LeaseExpired. Verify check_lease(1) returns None.

3. **test_coherence_version_vector_conflict** — Create two VersionVectors. Update vv1 with inode 1 → version 5. Update vv2 with inode 1 → version 3. Call vv1.conflicts(&vv2). Verify inode 1 is in conflict list (since versions diverge). Merge vv2 into vv1. Verify get(1) returns max.

4. **test_coherence_invalidate_remote_write** — Create manager. Grant lease for inode 1. Call invalidate(1, RemoteWrite(42), version 2). Verify pending_invalidations() has 1 entry. Call drain_invalidations(). Verify empty after drain.

5. **test_coherence_is_coherent** — Create manager with Strict protocol. Verify is_coherent(inode) behavior for an inode with no lease (should be true — no lease means no stale cache). Grant lease. Verify is_coherent returns true while lease is active. Revoke lease. Verify behavior after revoke.

### Category 2: Crash Recovery State Machine (5 tests)

6. **test_recovery_initial_state** — Create CrashRecovery with default config. Verify state() is Idle. Verify journal().open_file_count() == 0 and pending_write_count() == 0.

7. **test_recovery_scan_and_record** — Create recovery. Begin scan. Verify state is Scanning. Record 3 open files. Verify journal().open_file_count() == 3. Record 2 pending writes. Verify pending_write_count() == 2.

8. **test_recovery_replay_progress** — Create recovery. Begin scan. Begin replay with total=10. Verify state is Replaying { replayed: 0, total: 10 }. Advance replay by 5. Verify state is Replaying { replayed: 5, total: 10 }. Complete with 2 orphaned. Verify state is Complete { recovered: 5, orphaned: 2 }.

9. **test_recovery_fail_and_reset** — Create recovery. Begin scan. Call fail("disk error"). Verify state is Failed("disk error"). Call reset(). Verify state returns to Idle.

10. **test_recovery_stale_pending_writes** — Create recovery. Begin scan. Record pending writes with dirty_since_secs of 100 and 500. Query stale_pending_writes(now=600, max_age=200). Verify only the write at 100 is stale (age=500 > 200), not the one at 500 (age=100 < 200).

### Category 3: Write Buffer Security (5 tests)

11. **test_writebuf_buffer_and_take** — Create WriteBuf with default config. Buffer a write for inode 1 at offset 0 with data b"hello". Verify is_dirty(1) returns true. Call take_dirty(1). Verify returns WriteRange with offset=0 and data matches.

12. **test_writebuf_coalesce_adjacent** — Create WriteBuf. Buffer write at offset 0, len 100. Buffer write at offset 100, len 100 (adjacent). Call coalesce(ino). Take dirty. Verify coalesced into fewer ranges (ideally 1 range of 200 bytes).

13. **test_writebuf_discard** — Create WriteBuf. Buffer writes for inode 1 and inode 2. Discard inode 1. Verify is_dirty(1) returns false. Verify is_dirty(2) still true.

14. **test_writebuf_total_buffered** — Create WriteBuf. Buffer 3 writes of 100 bytes each to different inodes. Verify total_buffered() == 300.

15. **test_writebuf_dirty_inodes_list** — Create WriteBuf. Buffer writes for inodes 1, 2, 3. Call dirty_inodes(). Verify returns 3 inodes. Take dirty for inode 2. Verify dirty_inodes() returns 2.

### Category 4: Data Cache Security (5 tests)

16. **test_datacache_insert_and_get** — Create DataCache with default config. Insert data for inode 1 with generation 1. Get inode 1. Verify CachedData matches with correct generation.

17. **test_datacache_eviction_on_max_files** — Create DataCache with max_files=2. Insert data for inodes 1, 2, 3. Verify len() == 2 (oldest evicted). Verify stats().evictions >= 1.

18. **test_datacache_invalidate** — Create cache. Insert data for inode 1. Invalidate inode 1. Verify get(1) returns None. Verify stats().hits and misses counted correctly.

19. **test_datacache_generation_invalidation** — Create cache. Insert for inode 1 with generation 5. Call invalidate_if_generation(1, 5). Verify evicted. Insert for inode 2 with generation 3. Call invalidate_if_generation(2, 2). Verify NOT evicted (generation doesn't match).

20. **test_datacache_max_bytes_limit** — Create cache with max_bytes=100. Insert 50 bytes for inode 1 (succeeds). Insert 60 bytes for inode 2 (should evict inode 1 to make room, or reject). Verify total_bytes() <= 100.

### Category 5: Session & Config Validation (5 tests)

21. **test_session_config_defaults** — Create SessionConfig::default(). Verify mountpoint is empty PathBuf. Document default fs_config and server_config values.

22. **test_session_stats_default** — Create SessionStats::default(). Verify all counters are 0: requests_processed, bytes_read, bytes_written, errors.

23. **test_recovery_config_defaults** — Create RecoveryConfig::default_config(). Verify max_recovery_secs > 0 (should be 30). Verify max_open_files > 0 (should be 10000). Verify stale_write_age_secs > 0 (should be 300).

24. **test_writebuf_config_defaults** — Create WriteBufConfig::default(). Verify flush_threshold > 0 (should be ~1MB). Verify max_coalesce_gap > 0. Verify dirty_timeout_ms > 0.

25. **test_datacache_config_defaults** — Create DataCacheConfig::default(). Verify max_files > 0 (256). Verify max_bytes > 0 (64MB). Verify max_file_size > 0 (4MB). Verify max_file_size <= max_bytes.

## Implementation notes
- Use `fn make_xxx()` helper functions for creating test objects
- Mark findings with `// FINDING-FUSE-CACHE-XX: description`
- If a type is not public, skip that test and add an alternative
- Each test focuses on one property
- Use `assert!`, `assert_eq!`, `matches!`
- DO NOT use any async code — all tests are synchronous
- For InodeId, use InodeId::new(1) or InodeId(1) depending on constructor
- For LeaseId, use LeaseId::new(1)
- For OpenFileRecord: OpenFileRecord { ino: InodeId::new(1), fd: 1, pid: 100, flags: 0, path_hint: "test".into() }
- For PendingWrite: PendingWrite { ino: InodeId::new(1), offset: 0, len: 100, dirty_since_secs: 100 }

## Output format
Output ONLY the complete Rust source file. No explanations, no markdown fences.
