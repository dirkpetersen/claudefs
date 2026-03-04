# Task: Write storage_deep_security_tests_v2.rs for claudefs-security crate

Write a comprehensive security test module for the `claudefs-storage` crate focusing on allocator boundaries, block cache poisoning, quota enforcement, wear leveling bias, and hot swap state machine security.

## File location
`crates/claudefs-security/src/storage_deep_security_tests_v2.rs`

## Module structure
```rust
//! Deep security tests v2 for claudefs-storage: allocator, cache, quota, wear, hot swap.
//!
//! Part of A10 Phase 7: Storage deep security audit v2

#[cfg(test)]
mod tests {
    // imports and tests here
}
```

## Available types (verified from lib.rs pub use statements)

```rust
use claudefs_storage::{
    // Allocator
    BuddyAllocator, AllocatorConfig, AllocatorStats,
    // Block
    BlockId, BlockRef, BlockSize, PlacementHint,
    // Block cache
    BlockCache, BlockCacheConfig, CacheEntry, CacheStats,
    // Checksum
    Checksum, ChecksumAlgorithm, BlockHeader,
    // Quota
    QuotaManager as StorageQuotaManager, QuotaLimit as StorageQuotaLimit,
    QuotaUsage as StorageQuotaUsage, QuotaStatus as StorageQuotaStatus, TenantQuota, QuotaStats,
    // Wear leveling
    WearLevelingEngine, WearConfig, WearLevel, ZoneWear, PlacementAdvice, WearAlert,
    // Hot swap
    HotSwapManager, DeviceState, DrainProgress, BlockMigration, MigrationState, HotSwapStats, HotSwapEvent, HotSwapError,
    // Erasure
    ErasureCodingEngine, EcProfile, EcShard, EcStripe, EcConfig, EcStats, EcError, StripeState,
    // NVMe
    PassthroughManager, PassthroughConfig as NvmePassthroughConfig,
    QueuePair, QueuePairId, CoreId, NsId, QueueState, NvmeOpType, SubmissionEntry, CompletionEntry, CompletionStatus,
    // Device
    DeviceRole,
    // Error
    StorageError, StorageResult,
};
// Module paths if needed:
use claudefs_storage::wear_leveling::WritePattern;
```

**IMPORTANT**: Not all may be public or may have different names. If any import fails, remove it. Adjust type names based on compilation errors. The types listed come from reading the lib.rs pub use statements.

## Existing tests to AVOID duplicating

The existing `storage_deep_security_tests.rs` already covers:
- IntegrityManager/IntegrityConfig (5 tests)
- AtomicWriteEngine/AtomicWriteCapability (5 tests)
- RecoveryManager/AllocatorBitmap (5 tests)
- WriteJournal/JournalConfig (5 tests)
- ScrubEngine/HotSwapManager basics (5 tests)

The existing `storage_encryption_tests.rs` already covers:
- EncryptionEngine mock mode, key rotation, nonce, tag, algorithm

DO NOT duplicate any of these. Focus only on the NEW categories below.

## Test categories (25 tests total, 5 per category)

### Category 1: Allocator Boundary Security (5 tests)

1. **test_allocator_stats_after_alloc_free** — Create BuddyAllocator with device_idx=0, total_blocks_4k=1024. Allocate a BlockSize::Page4K block. Free it. Check stats show allocations_count >= 1 and frees_count >= 1.

2. **test_allocator_exhaust_capacity** — Create BuddyAllocator with total_blocks_4k=16. Allocate 4K blocks until allocation fails. Verify the allocator returns an error when capacity is exhausted.

3. **test_allocator_large_block_alignment** — Create BuddyAllocator with total_blocks_4k=4096. Allocate a Block1M (requires 256 x 4K blocks). Verify it succeeds. Free it. Allocate again.

4. **test_allocator_free_returns_to_pool** — Create BuddyAllocator with total_blocks_4k=64. Allocate all capacity with 4K blocks. Free half of them. Allocate again — should succeed for the freed capacity.

5. **test_allocator_zero_capacity_rejected** — Try to create BuddyAllocator with total_blocks_4k=0. Verify it returns an error or handles gracefully. (FINDING: if zero capacity accepted, all allocations would fail).

### Category 2: Block Cache Poisoning (5 tests)

6. **test_cache_insert_get_roundtrip** — Create BlockCache with max_entries=100. Create CacheEntry with known data and checksum. Insert it. Get it back. Verify data matches.

7. **test_cache_eviction_at_capacity** — Create BlockCache with max_entries=3. Insert 4 entries. Verify oldest entry is evicted (get returns None for it). Verify newest 3 still present.

8. **test_cache_dirty_entry_tracking** — Create BlockCache. Insert entry with dirty=true. Verify it's tracked as dirty. Insert entry with dirty=false. Verify stats track dirty count.

9. **test_cache_checksum_stored_correctly** — Create CacheEntry with specific checksum. Insert into cache. Retrieve. Verify checksum field matches what was stored (no silent corruption of metadata).

10. **test_cache_pinned_entry_survives_eviction** — Create BlockCache with max_entries=2. Insert pinned entry. Insert 2 more unpinned entries. Verify pinned entry survives eviction. (FINDING: pinned entries could exhaust cache if not limited).

### Category 3: Storage Quota Enforcement (5 tests)

11. **test_storage_quota_hard_limit_blocks** — Create TenantQuota with bytes_hard=1000. Set usage to 999. Call can_allocate(2). Verify returns false (would exceed hard limit).

12. **test_storage_quota_soft_limit_grace** — Create TenantQuota with bytes_soft=100, grace_period_secs=3600. Set usage to 150, soft_exceeded_since=Some(now). Check status at now+1800 (within grace). Verify SoftExceeded. Check at now+3601 (past grace). Verify GraceExpired.

13. **test_storage_quota_zero_limits** — Create QuotaLimit with bytes_hard=0 and inodes_hard=0. Set usage to 0. Verify can_allocate(1) returns false. (FINDING: zero hard limit means permanently blocked).

14. **test_storage_quota_usage_at_exactly_hard** — Create TenantQuota with bytes_hard=100. Set usage to exactly 100. Call check_status(). Verify HardExceeded (usage == hard means exceeded, not at boundary). If not HardExceeded, document the boundary behavior.

15. **test_storage_quota_stats_tracking** — Create QuotaManager. Add 3 tenants. Set one at soft limit, one at hard limit, one ok. Call stats. Verify tenants_at_soft_limit=1, tenants_at_hard_limit=1.

### Category 4: Wear Leveling Security (5 tests)

16. **test_wear_leveling_hot_zone_detection** — Create WearLevelingEngine with hot_zone_threshold=80.0. Record writes to zone 0 until it exceeds threshold. Call check_alerts(). Verify a HighWear alert is generated for zone 0.

17. **test_wear_advice_after_writes** — Create WearLevelingEngine. Record many writes to zone 0 (making it hot). Call get_wear_advice(). Verify advice is ColdZone or FreshZone (not the hot zone).

18. **test_wear_alert_severity** — Create WearLevelingEngine. Record extreme writes to make zone very hot. Check alerts. Verify severity > 0 (higher severity for worse wear).

19. **test_wear_no_writes_no_alerts** — Create WearLevelingEngine with default config. Don't record any writes. Call check_alerts(). Verify empty (no false alerts).

20. **test_wear_write_pattern_tracking** — Create WearLevelingEngine. Record sequential writes to zone 0. Record random writes to zone 1. Verify both zones are tracked separately.

### Category 5: Hot Swap State Machine (5 tests)

21. **test_hot_swap_register_and_drain** — Create HotSwapManager. Register device 0 as Data role. Start drain. Verify drain progress shows device 0 with total blocks.

22. **test_hot_swap_drain_unregistered_fails** — Create HotSwapManager. Try to drain device 99 (never registered). Verify error returned.

23. **test_hot_swap_double_register_fails** — Create HotSwapManager. Register device 0. Try to register device 0 again. Verify error (duplicate registration).

24. **test_hot_swap_remove_active_device** — Create HotSwapManager. Register device 0. Try to remove device 0 without draining first. Document whether this succeeds or requires drain first. (FINDING: if removal allowed without drain, data loss risk).

25. **test_hot_swap_fail_device_state** — Create HotSwapManager. Register device 0. Call fail_device(0, "media error"). Verify device state is Failed. Try to start drain on failed device. Document behavior.

## Implementation notes
- Use `fn make_xxx()` helper functions for creating test objects
- Mark security findings with `// FINDING-STOR-DEEP2-XX: description`
- If a type is not public or has different name, skip that test and add an alternative
- DO NOT duplicate tests from existing storage_deep_security_tests.rs or storage_encryption_tests.rs
- Each test focuses on one property
- Use `assert!`, `assert_eq!`, `matches!`
- DO NOT use any async code or tokio — all tests are synchronous
- For BlockRef, use BlockRef::new(BlockId(n), BlockSize::Page4K) or whatever constructor is available
- For Checksum, use Checksum::new(0x12345678) or similar constructor

## Output format
Output ONLY the complete Rust source file. No explanations, no markdown fences.
