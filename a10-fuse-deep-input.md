# Task: Write fuse_deep_security_tests.rs for claudefs-security crate

Write a comprehensive security test module for the `claudefs-fuse` crate focusing on buffer pool memory safety, passthrough FD management, capability negotiation, quota/rate limit enforcement, and WORM enforcement.

## File location
`crates/claudefs-security/src/fuse_deep_security_tests.rs`

## Module structure
```rust
//! Deep security tests for claudefs-fuse: buffer pool, passthrough, capability, quota, WORM.
//!
//! Part of A10 Phase 7: FUSE deep security audit

#[cfg(test)]
mod tests {
    // imports and tests here
}
```

## Available types (verified from lib.rs and module exploration)

```rust
use claudefs_fuse::buffer_pool::{Buffer, BufferPool, BufferPoolConfig, BufferPoolStats, BufferSize};
use claudefs_fuse::passthrough::{PassthroughConfig, PassthroughState, PassthroughStatus, check_kernel_version};
use claudefs_fuse::capability::{
    CapabilityNegotiator, KernelVersion, NegotiatedCapabilities, PassthroughMode,
    KERNEL_FUSE_PASSTHROUGH, KERNEL_ATOMIC_WRITES, KERNEL_DYNAMIC_IORING,
};
use claudefs_fuse::mount_opts::{CacheMode, MountOptions, ReadWriteMode};
use claudefs_fuse::quota_enforce::{QuotaEnforcer, QuotaStatus, QuotaUsage};
use claudefs_fuse::ratelimit::{RateLimiterConfig, RateLimitDecision, TokenBucket};
use claudefs_fuse::worm::{ImmutabilityMode, WormRegistry};
use claudefs_fuse::interrupt::{InterruptTracker, RequestId, RequestRecord, RequestState};
use claudefs_fuse::flock::{FlockConflict, FlockEntry, FlockHandle, FlockRegistry, FlockType};
use claudefs_fuse::inode::InodeId;
```

**IMPORTANT**: Not all may be public. If any import fails, remove it and skip those tests. Start with what's most likely available.

## Test categories (25 tests total, 5 per category)

### Category 1: Buffer Pool Memory Safety (5 tests)

1. **test_buffer_clear_only_partial** — Create BufferPool, acquire a Buffer, write sensitive data (0xFF) to entire buffer. Call buf.clear(). Verify that bytes beyond position 64 still contain 0xFF. (FINDING: clear() only zeroes first 64 bytes — sensitive data leakage risk).

2. **test_buffer_pool_exhaustion_still_allocates** — Create BufferPool with max_4k=2, max_64k=0, max_1m=0. Acquire 2 buffers. Acquire a 3rd. Document whether it succeeds (allocates fresh) or fails (capacity enforced). (FINDING: pool may allocate beyond max).

3. **test_buffer_id_uniqueness** — Create BufferPool. Acquire 100 buffers. Verify all buffer IDs are unique. Release them all. Acquire 100 more. Verify no ID reuse (or document if IDs are reused).

4. **test_buffer_size_bytes_correctness** — Verify BufferSize::Page4K.size_bytes() == 4096, Block64K == 65536, Block1M == 1048576. Acquire buffers of each size and verify buf.len() matches.

5. **test_buffer_pool_stats_accuracy** — Acquire 3 buffers, release 2. Verify stats: alloc_count == 3, return_count == 2, reuse_count is tracked correctly. Acquire 2 more (should reuse). Verify reuse_count incremented.

### Category 2: Passthrough & Capability Security (5 tests)

6. **test_passthrough_negative_fd_accepted** — Create PassthroughState. Call register_fd(1, -1). Verify the negative FD is stored. (FINDING: no validation of FD values — negative FDs are invalid but accepted).

7. **test_passthrough_fd_table_unbounded_growth** — Create PassthroughState. Register 10000 FD entries. Verify fd_count() == 10000. (FINDING: no limit on fd_table size — memory exhaustion vector).

8. **test_capability_panic_without_negotiate** — Create CapabilityNegotiator. Call is_negotiated() — verify false. DO NOT call capabilities() directly since it panics. This documents the panic risk. (FINDING: calling capabilities() before negotiate() panics).

9. **test_kernel_version_parse_edge_cases** — Parse "0.0.0", "999.999.999", empty string "", "abc", "6.8". Verify valid ones parse correctly and invalid ones return None.

10. **test_passthrough_kernel_version_boundary** — Check kernel version (6, 7) with PassthroughConfig enabled. Verify DisabledKernelTooOld. Check (6, 8). Verify Enabled. Check (6, 9). Verify Enabled.

### Category 3: Mount Options & Session Security (5 tests)

11. **test_mount_default_permissions_false_risk** — Create MountOptions::default(). Verify default_permissions is false. (FINDING: kernel permission checks disabled by default — relies on FUSE daemon for permission enforcement).

12. **test_mount_direct_io_with_kernel_cache** — Create MountOptions, set direct_io=true and kernel_cache=true simultaneously. Call to_fuse_args(). Document if conflicting options are accepted without warning.

13. **test_mount_options_to_fuse_args_content** — Create MountOptions with allow_other=true, read_only. Call to_fuse_args(). Verify output contains "allow_other" and "ro" flags.

14. **test_mount_empty_paths** — Create MountOptions with empty source and target paths. Document whether validation catches this or allows empty paths.

15. **test_mount_max_background_zero** — Create MountOptions with max_background=0 and congestion_threshold=0. Call to_fuse_args(). Verify the values are present (potential stall vector if background=0).

### Category 4: Rate Limiting & Quota Enforcement (5 tests)

16. **test_token_bucket_refill_overflow** — Create TokenBucket with very high capacity and refill_rate. Call refill() after a large time delta. Verify tokens don't overflow u64 (saturating behavior expected).

17. **test_token_bucket_consume_more_than_available** — Create TokenBucket with capacity 100. Consume 50 (success). Try consume 60 (should fail — only 50 remain). Verify decision is Throttle or Reject.

18. **test_quota_enforcer_check_boundary** — Create QuotaUsage at exact soft limit. Call check. Verify returns SoftExceeded, not HardExceeded. Set to hard limit. Verify HardExceeded.

19. **test_rate_limiter_burst_factor_multiplier** — Create RateLimiterConfig with burst_factor=2.0. Verify effective capacity is 2x the base. Consume up to burst limit.

20. **test_token_bucket_zero_refill_rate** — Create TokenBucket with refill_rate=0. Consume all tokens. Call refill(). Verify tokens remain 0 (no refill). (FINDING: zero refill rate creates permanent denial).

### Category 5: WORM & Immutability Enforcement (5 tests)

21. **test_worm_immutable_blocks_all_writes** — Create WormRegistry. Set inode to Immutable mode. Verify is_write_blocked returns true, is_delete_blocked returns true, is_rename_blocked returns true, is_truncate_blocked returns true.

22. **test_worm_append_only_allows_append** — Create WormRegistry. Set inode to AppendOnly mode. Verify is_write_blocked returns false (append still allowed), is_delete_blocked returns true, is_truncate_blocked returns true.

23. **test_worm_none_mode_allows_all** — Create WormRegistry. Set inode to None mode. Verify all operations return false (nothing blocked).

24. **test_worm_legal_hold_overrides** — Create WormRegistry. Set inode to LegalHold mode. Verify is_write_blocked, is_delete_blocked, is_rename_blocked all return true (legal hold is strictest).

25. **test_worm_mode_change_allowed** — Create WormRegistry. Set inode to AppendOnly. Then change to Immutable. Verify the change succeeds. (FINDING: mode can be escalated OR downgraded — no unidirectional enforcement).

## Implementation notes
- Use `fn make_xxx()` helper functions for creating test objects
- Mark security findings with `// FINDING-FUSE-DEEP-XX: description`
- If a type is not public, skip that test and add a simple alternative using types that ARE public
- Each test focuses on one property
- Use `assert!`, `assert_eq!`, `matches!`
- DO NOT use any async code or tokio — all tests are synchronous

## Output format
Output ONLY the complete Rust source file. No explanations, no markdown fences.
