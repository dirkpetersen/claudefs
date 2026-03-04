# Task: Write meta_deep_security_tests.rs for claudefs-security crate

Write a comprehensive security test module `meta_deep_security_tests.rs` for the `claudefs-security` crate that tests security properties of the `claudefs-meta` crate's distributed metadata modules.

## File location
`crates/claudefs-security/src/meta_deep_security_tests.rs`

## Module structure

The file must follow this exact pattern:

```rust
//! Deep security tests for claudefs-meta crate: transactions, locking, tenants, quotas, shards.
//!
//! Part of A10 Phase 5: Meta deep security audit — auth gaps, isolation, atomicity, DoS vectors

#[cfg(test)]
mod tests {
    // imports and tests here
}
```

## Available types from claudefs-meta (verified from lib.rs pub use statements)

These are ALL confirmed public re-exports from `claudefs_meta`:

```rust
use claudefs_meta::{
    // Core types
    InodeId, NodeId, ShardId, MetaOp, MetaError, Timestamp, FileType, InodeAttr, Term, LogIndex,
    // Transaction types (from transaction module)
    TransactionId, TransactionState, Transaction, TransactionManager,
    // Locking types (from locking module)
    LockType, LockManager,
    // Tenant types (from tenant module)
    TenantId, TenantConfig, TenantUsage, TenantManager,
    // Quota types (from quota module)
    QuotaTarget, QuotaLimit, QuotaUsage, QuotaEntry, QuotaManager,
    // Shard types (from shard module)
    ShardRouter, ShardInfo, ShardAssigner,
    // Lease types (from lease module)
    LeaseManager, LeaseType,
    // FileHandle types (from filehandle module)
    FileHandleManager, FileHandle, OpenFlags,
    // Membership types (from membership module)
    MembershipManager, MemberInfo, NodeState, MembershipEvent,
    // ReadIndex types (from readindex module)
    ReadIndexManager, ReadStatus, PendingRead,
    // KvStore (from kvstore module)
    KvStore, MemoryKvStore,
    // Raft log (from raft_log module)
    RaftLogStore,
    // Service
    MetadataService, MetadataServiceConfig,
};
```

Note: `TransactionParticipant` may not be re-exported, use `claudefs_meta::transaction::TransactionParticipant` if needed.
Note: `MetadataJournal` is in `claudefs_meta::journal` module — use `claudefs_meta::journal::MetadataJournal` if needed.

## Test categories (25 tests total, 5 per category)

### Category 1: Transaction Security (5 tests)

1. **test_transaction_vote_change_allowed** — Begin transaction with 2 participant shards. Have shard 1 vote_commit, then vote_abort for the same shard. Verify the vote was silently changed (FINDING: vote overwrite allowed, no idempotency).

2. **test_transaction_nonparticipant_vote** — Begin transaction with shards [ShardId(1), ShardId(2)]. Try to vote_commit with ShardId(99). Verify it returns an error (participant not found).

3. **test_transaction_check_votes_before_all_voted** — Begin transaction with 2 shards, only vote one. Call check_votes. Verify state remains Preparing (not prematurely decided).

4. **test_transaction_double_begin_unique_ids** — Call begin_transaction twice. Verify the two TransactionIds are different (monotonically increasing).

5. **test_transaction_abort_overrides_commit** — Begin transaction with 2 shards. Shard 1 votes commit, shard 2 votes abort. Call check_votes. Verify state transitions to Aborting (any abort → abort all).

### Category 2: Locking Security (5 tests)

6. **test_lock_write_blocks_read** — Acquire a Write lock on inode 42 by node 1. Try to acquire Read lock on same inode by node 2. Verify it fails with PermissionDenied.

7. **test_lock_write_blocks_write** — Acquire Write lock on inode 42 by node 1. Try Write lock on same inode by node 2. Verify PermissionDenied.

8. **test_lock_read_allows_read** — Acquire Read lock on inode 42 by node 1. Acquire Read lock by node 2. Verify both succeed (read locks are shared).

9. **test_lock_release_nonexistent_silent** — Call release(999999) with no locks. Verify it returns Ok(()) silently (FINDING: no error on releasing nonexistent lock).

10. **test_lock_release_all_for_node_cleanup** — Acquire 3 locks across 2 inodes for node 1. Call release_all_for_node(node 1). Verify all 3 released and is_locked returns false.

### Category 3: Tenant Isolation (5 tests)

11. **test_tenant_inactive_rejects_assign** — Create tenant with active=true. Then remove it and re-create with active=false. Try to assign_inode. Verify PermissionDenied.

12. **test_tenant_quota_boundary** — Create tenant with max_inodes=2. Assign inode 100, then assign inode 101. Try assigning inode 102. Verify NoSpace error (off-by-one check: >= means at max_inodes you're rejected).

13. **test_tenant_duplicate_creation_fails** — Create tenant "t1". Try creating tenant "t1" again. Verify EntryExists error.

14. **test_tenant_release_inode_cleanup** — Assign inode 100 to tenant "t1". Call release_inode(100). Verify tenant_for_inode(100) returns None.

15. **test_tenant_empty_id_allowed** — Create tenant with TenantId::new(""). Verify it succeeds (FINDING: empty tenant IDs accepted, no validation).

### Category 4: Quota Enforcement (5 tests)

16. **test_quota_usage_saturating_add** — Create QuotaUsage, call add(i64::MAX, 0) twice. Verify bytes_used saturates at u64::MAX (doesn't overflow/wrap).

17. **test_quota_usage_negative_underflow** — Create QuotaUsage with bytes_used=5. Call add(-100, 0). Verify bytes_used saturates at 0 (doesn't wrap to large number).

18. **test_quota_is_over_quota_boundary** — Create QuotaEntry with max_bytes=100. Set usage.bytes_used=100. Verify is_over_quota() returns false (since > not >=). Set to 101, verify true.

19. **test_quota_set_and_get_roundtrip** — Create QuotaManager. set_quota for User(1000) with max_bytes=1GB, max_inodes=10000. Get quota. Verify limit matches.

20. **test_quota_remove_nonexistent** — Create QuotaManager. Call remove_quota for User(999). Verify returns false (not found).

### Category 5: Shard & Journal Security (5 tests)

21. **test_shard_router_deterministic** — Create ShardRouter with 256 shards. Call shard_for_inode on same inode 100 times. Verify always returns same shard.

22. **test_shard_leader_not_assigned** — Create ShardRouter. Query leader_for_shard for an unassigned shard. Verify returns NotLeader or None error.

23. **test_journal_sequence_monotonic** — Create MetadataJournal. Append 3 entries. Verify sequences are monotonically increasing (1, 2, 3).

24. **test_journal_compact_before** — Create MetadataJournal. Append 5 entries. Compact before sequence 3. Verify read_from(1, 10) returns only entries with seq >= 3.

25. **test_journal_replication_lag** — Create MetadataJournal. Append 10 entries. Check replication_lag(5). Verify lag is 5 (latest_seq - remote_seq).

## Implementation notes

- Use `fn make_xxx()` helper functions following the existing pattern
- Each test should have a comment like `// FINDING-META-DEEP-XX: description` for any security findings
- If a type is not publicly exported, skip that test and replace with an alternative test from the same category
- Keep tests simple and focused — test one property each
- Use `assert!`, `assert_eq!`, `matches!` — no unwrap in assertions

## Error handling

If `TransactionManager`, `LockManager`, `TenantManager`, `QuotaManager` are not all public, use whatever IS available. The key types that SHOULD be public based on lib.rs:
- `LockManager`, `LockType`, `LockEntry` — from `locking` module
- `TransactionId`, `TransactionState`, `TransactionParticipant`, `Transaction`, `TransactionManager` — from `transaction` module
- `TenantId`, `TenantConfig`, `TenantUsage`, `TenantManager` — from `tenant` module
- `QuotaTarget`, `QuotaLimit`, `QuotaUsage`, `QuotaEntry`, `QuotaManager` — from `quota` module
- `ShardRouter`, `ShardInfo` — from `shard` module
- `MetadataJournal` — from `journal` module

## Output format

Output ONLY the complete Rust source file. No explanations, no markdown fences, just the raw .rs content.
