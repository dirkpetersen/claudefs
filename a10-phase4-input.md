# A10 Phase 4: FUSE Extensions + Storage Encryption Security Tests

You are writing security audit tests for the ClaudeFS distributed filesystem.

## Task

Create TWO new test modules in `crates/claudefs-security/src/`:

1. `fuse_ext_security_tests.rs` — Security tests for newly added FUSE extension modules
2. `storage_encryption_tests.rs` — Security tests for storage layer encryption

## Context

The security crate is at `crates/claudefs-security/`. It already has 698 passing tests across 25 test modules.
The existing `Cargo.toml` has dependencies on all 8 crates. Tests use `#[cfg(test)]` modules.

## Module 1: fuse_ext_security_tests.rs (25 tests)

Test security properties of these FUSE extension modules:

### A. IdMapper Security (idmap.rs) — 5 tests
The IdMapper translates UIDs/GIDs. Security-critical because wrong mappings = privilege escalation.

```rust
use claudefs_fuse::idmap::{IdMapper, IdMapMode, IdMapEntry};
```

1. `test_squash_root_not_preserved` — In Squash mode, UID 0 maps to nobody_uid (65534), NOT root. Verify root privilege is stripped.
2. `test_rangeshift_overflow_wraps` — RangeShift with host_base near u32::MAX. Test that `host_base + count` doesn't overflow/wrap and grant wrong mappings. Use: host_base=u32::MAX-5, local_base=0, count=10. Mappings for UIDs >= host_base should be checked — overflow shouldn't map to unexpected IDs.
3. `test_table_mode_unmapped_uid_passthrough` — Unmapped UIDs pass through as-is in Table mode. This means a client can claim any unmapped UID. Document as finding FUSE-EXT-01.
4. `test_identity_root_always_zero` — Identity mode always preserves root (UID 0 → 0). This is correct but should be noted — root access is never blocked in identity mode.
5. `test_reverse_map_not_available_for_rangeshift` — reverse_map_uid returns None for non-Table modes. This means there's no way to validate reverse mappings in RangeShift mode. Document as finding FUSE-EXT-02.

### B. POSIX ACL Security (posix_acl.rs) — 5 tests
ACL evaluation must be correct or users get wrong permissions.

```rust
use claudefs_fuse::posix_acl::{PosixAcl, AclEntry, AclTag, AclPerms};
```

6. `test_acl_no_entries_denies_all` — Empty ACL denies all access. check_access with no entries should return false.
7. `test_acl_mask_does_not_limit_owner` — According to POSIX.1e, the mask should NOT limit the owner (UserObj). Verify that even with a restrictive mask, the owner's UserObj perms are NOT masked. NOTE: Check the actual implementation — if it DOES mask the owner, that's a bug (FINDING FUSE-EXT-03).
8. `test_acl_unbounded_entries` — No limit on number of ACL entries. Add 10000 entries and verify no panic. Document as FUSE-EXT-04 (DoS risk: unbounded ACL size).
9. `test_acl_duplicate_user_entries` — Two User(1000) entries with different perms. Which one wins? Document behavior as finding FUSE-EXT-05.
10. `test_acl_root_uid_zero_bypasses_acl` — UID 0 (root) should match UserObj when file_uid is 0. But no special root bypass is implemented — root gets the same ACL treatment as any user. Document as FUSE-EXT-06.

### C. FlockRegistry Security (flock.rs) — 5 tests

```rust
use claudefs_fuse::flock::{FlockRegistry, FlockHandle, FlockType, FlockConflict};
```

11. `test_flock_no_ttl_deadlock_risk` — Locks have no TTL. A process that acquires a lock and crashes leaves it held forever. Document as FUSE-EXT-07 (same pattern as META-07).
12. `test_flock_pid_zero_allowed` — PID 0 (kernel thread) can acquire locks. Verify this is handled without special casing.
13. `test_flock_large_fd_values` — fd = u64::MAX is accepted. Verify no overflow in HashMap key computation.
14. `test_flock_upgrade_race_window` — Upgrade from shared to exclusive checks other holders. But the check and upgrade are not atomic — another process could acquire a shared lock between the check and upgrade. Document as FUSE-EXT-08.
15. `test_flock_holder_count_mismatch_after_unlock` — Unlock via FlockType::Unlock removes from locks but the by_inode index may become stale. Verify holder_count accuracy after unlock operations.

### D. InterruptTracker Security (interrupt.rs) — 5 tests

```rust
use claudefs_fuse::interrupt::{InterruptTracker, RequestId, RequestState};
```

16. `test_interrupt_tracker_request_id_collision` — Register same RequestId twice. The second insert silently overwrites the first. Document as FUSE-EXT-09 (request state confusion).
17. `test_interrupt_completed_request_still_tracked` — After interrupt(), the request stays in pending map. It's never auto-removed. Document as FUSE-EXT-10 (memory leak for interrupted requests).
18. `test_drain_timed_out_counts_as_completed` — drain_timed_out increments total_completed. But timed-out requests should NOT count as completed — they were abandoned. Document as FUSE-EXT-11.
19. `test_interrupt_after_complete_returns_false` — Interrupting a completed (removed) request returns false. This is correct behavior.
20. `test_max_pending_enforcement` — At max_pending capacity, new requests are rejected. Verify this DoS protection works.

### E. DirCache Security (dir_cache.rs) — 5 tests

```rust
use claudefs_fuse::dir_cache::{DirCache, DirCacheConfig, DirEntry};
use claudefs_fuse::inode::InodeKind;
use std::time::Duration;
```

21. `test_dir_cache_negative_entry_no_size_limit` — Negative entries have no max count. Insert 100000 negative entries and verify memory grows unboundedly. Document as FUSE-EXT-12.
22. `test_dir_cache_stale_after_mutation` — After inserting a snapshot, if the directory is mutated externally but not invalidated, the cache serves stale data. Document as FUSE-EXT-13 (TOCTOU).
23. `test_dir_cache_eviction_not_lru` — When at max_dirs, eviction removes `pop()` from keys — not LRU, just arbitrary HashMap iteration order. Document as FUSE-EXT-14.
24. `test_dir_cache_lookup_double_miss_count` — lookup() increments misses even when get_snapshot increments misses too, leading to double-counting. Verify and document.
25. `test_dir_cache_entry_name_injection` — Entry names with path separators ("/"), null bytes, or ".." are not validated. Insert entries with malicious names and verify they're stored as-is. Document as FUSE-EXT-15.

## Module 2: storage_encryption_tests.rs (25 tests)

Test security properties of storage layer encryption.

### A. EncryptionKey Security — 5 tests

```rust
use claudefs_storage::encryption::{
    EncryptionEngine, EncryptionConfig, EncryptionKey, EncryptionAlgorithm, EncryptedBlock,
};
```

1. `test_key_material_not_zeroized_on_drop` — EncryptionKey stores key_bytes as Vec<u8> without Zeroize. When the key is dropped, the memory may still contain key material. Document as STOR-ENC-01 (CRITICAL: no key zeroization).
2. `test_mock_encryption_is_xor_not_aead` — The encrypt() method uses XOR cipher, not real AES-GCM. While labeled as mock, this could end up in production. Document as STOR-ENC-02 (CRITICAL: mock cipher in production code path).
3. `test_nonce_derived_from_plaintext` — The nonce is derived from plaintext content (first 12 bytes). This means identical plaintexts produce identical nonces — catastrophic for AES-GCM. Document as STOR-ENC-03 (CRITICAL: nonce reuse from plaintext).
4. `test_tag_derived_from_plaintext` — The authentication tag is derived from plaintext content, not from actual AEAD computation. This provides zero integrity protection. Document as STOR-ENC-04.
5. `test_key_bytes_accessible_via_as_bytes` — as_bytes() is pub(crate), meaning any code in claudefs-storage can read the raw key material. Verify this is intentional.

### B. EncryptionEngine Security — 5 tests

6. `test_encrypt_without_key_reveals_nothing` — Attempting encryption without a key returns an error, doesn't leak data. Verify.
7. `test_encryption_disabled_by_default` — EncryptionConfig default has enabled=false. Encryption must be explicitly enabled. Verify this is safe-by-default.
8. `test_none_algorithm_plaintext_passthrough` — EncryptionAlgorithm::None stores plaintext as ciphertext. Verify and document as STOR-ENC-05 (data at rest is unencrypted).
9. `test_key_rotation_preserves_old_keys` — After rotation, old keys remain in the HashMap for decryption. But they're never removed or expired. Document as STOR-ENC-06 (key accumulation without cleanup).
10. `test_same_plaintext_same_ciphertext` — XOR with same key produces identical ciphertext for identical plaintext. This is a deterministic encryption oracle. Document as STOR-ENC-07.

### C. EncryptedBlock Integrity — 5 tests

11. `test_encrypted_block_tag_not_verified` — decrypt() never checks the authentication tag. Any ciphertext with any tag decrypts successfully. Document as STOR-ENC-08 (CRITICAL: no authentication).
12. `test_encrypted_block_nonce_not_verified` — decrypt() ignores the nonce entirely. Document as STOR-ENC-09.
13. `test_ciphertext_tamper_undetected` — Modify ciphertext bytes and decrypt. XOR decryption still "succeeds" with garbage output. Document as STOR-ENC-10.
14. `test_encrypted_block_key_id_not_authenticated` — An attacker can change key_id in EncryptedBlock to cause decryption with wrong key. Document as STOR-ENC-11.
15. `test_original_size_not_verified` — original_size field is not checked against actual ciphertext length. Document as STOR-ENC-12.

### D. Key Management — 5 tests

16. `test_generate_mock_key_predictable` — generate_mock() uses (0..32).map(|i| (i as u8) ^ 0x5A). This is a fixed pattern, not random. Document as STOR-ENC-13.
17. `test_key_id_from_system_time` — Mock key IDs use nanosecond timestamp. In tests, two keys created in rapid succession may collide. Verify.
18. `test_set_current_key_accepts_any_registered` — No permission check on who can change the active key. Document as STOR-ENC-14.
19. `test_decrypt_with_removed_key_fails` — If a key is somehow removed from the HashMap, decrypt fails with "Key not found". But there's no API to remove keys. Verify.
20. `test_key_length_zero_for_none` — EncryptionAlgorithm::None accepts empty key_bytes. Verify this edge case.

### E. Error Handling and Stats — 5 tests

21. `test_encryption_error_increments_and_stops` — Verify encryption_errors counter increments on failure but doesn't affect subsequent operations.
22. `test_stats_overflow_at_u64_max` — Encrypt u64::MAX+1 times. Verify stats don't wrap or panic (test by setting stats to near-max values).
23. `test_multiple_key_rotation_stats` — Rotate keys 5 times. Verify key_rotations count is 5.
24. `test_decrypt_missing_key_error_message` — Error message includes key_id. Verify it doesn't include the key material itself.
25. `test_config_serialization_roundtrip` — EncryptionConfig serializes/deserializes correctly via serde.

## Important Implementation Notes

1. Every test function starts with `#[test]` (no async needed for these)
2. Use descriptive test names matching the list above
3. For "document as FINDING" tests, add a comment like:
   ```rust
   // FINDING FUSE-EXT-01 (MEDIUM): Unmapped UIDs pass through as-is in Table mode
   // Risk: Client can claim any unmapped UID without validation
   ```
4. Tests should be self-contained — no external state needed
5. Use standard assertions: assert!, assert_eq!, assert_ne!
6. For boundary tests, use u32::MAX, u64::MAX where appropriate
7. Each test module should have a `mod tests { ... }` wrapper with `use super::*;` if needed, or direct imports

## File Structure

Each file should look like:
```rust
//! Security tests for [module description]
//!
//! Part of A10 Phase 4: Extended FUSE + Storage Security Audit

#[cfg(test)]
mod tests {
    use relevant::imports;

    #[test]
    fn test_name() {
        // FINDING ID (SEVERITY): Description
        // Risk: explanation
        // ... test code ...
    }
}
```

## What NOT to do
- Don't use `unsafe` code
- Don't use `tokio::test` — these are all sync tests
- Don't modify any existing files other than creating the new test files
- Don't add new dependencies

## Existing lib.rs modules pattern (for reference)
```rust
#[cfg(test)]
pub mod fuse_ext_security_tests;
#[cfg(test)]
pub mod storage_encryption_tests;
```
These module declarations will be added to lib.rs separately.
