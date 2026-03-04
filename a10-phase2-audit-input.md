# A10 Security Audit — Phase 2 Enhanced Tests

You are writing Rust test code for the `claudefs-security` crate. This crate depends on:
- `claudefs-reduce` (encryption, key_manager modules)
- `claudefs-transport` (tls, conn_auth, zerocopy, protocol modules)
- `claudefs-repl` (batch_auth, conduit, tls_policy, auth_ratelimit modules)
- `claudefs-gateway` (auth module)

## Task

Create a new file `crates/claudefs-security/src/phase2_audit.rs` containing comprehensive Phase 2 security audit tests. This file should be a `#[cfg(test)]` module.

## Required Test Groups

### Group 1: Nonce Collision Detection (4 tests)
Test that the encrypt function generates unique nonces for every call, even with same key and plaintext.

```rust
use claudefs_reduce::encryption::{encrypt, decrypt, random_nonce, EncryptionAlgorithm, EncryptionKey, Nonce};
use std::collections::HashSet;
```

- `test_nonce_never_repeats_same_key_same_plaintext`: Encrypt the same plaintext 1000 times with same key, collect all nonces, verify all unique.
- `test_nonce_distribution_all_bytes_covered`: Generate 10000 nonces, verify every byte position 0..12 has seen multiple distinct values (not stuck at 0).
- `test_concurrent_nonce_generation`: Spawn 8 threads each generating 1000 nonces, collect into shared set, verify no duplicates across threads.
- `test_nonce_is_not_counter_based`: Generate 100 nonces sequentially, verify they're not sequential (difference between consecutive nonces varies).

### Group 2: HKDF Key Isolation (3 tests)
```rust
use claudefs_reduce::encryption::derive_chunk_key;
```

- `test_hkdf_different_masters_different_outputs`: Same chunk_hash but different master keys → different derived keys.
- `test_hkdf_all_zero_master_still_derives`: Master key of all zeros still produces a non-zero derived key.
- `test_hkdf_output_has_good_entropy`: Derive 100 keys from sequential chunk hashes, verify no two share more than 4 bytes in common (out of 32).

### Group 3: Key Manager Lifecycle (3 tests)
```rust
use claudefs_reduce::key_manager::{KeyManager, KeyManagerConfig, KeyVersion};
use claudefs_reduce::encryption::EncryptionKey;
```

- `test_key_rotation_10_times_all_old_deks_recoverable`: Create a DEK at each version, rotate 10 times, verify all 10 DEKs can still be unwrapped.
- `test_history_pruning_loses_oldest_keys`: Rotate more than max_history times, verify the oldest key version can no longer unwrap.
- `test_rewrap_preserves_dek_value`: Wrap a DEK, rotate key, rewrap the old wrapped key, unwrap with new key, verify DEK bytes match original.

### Group 4: TLS Certificate Validation (4 tests)
```rust
use claudefs_transport::tls::{generate_self_signed_ca, generate_node_cert, TlsConfig, TlsConnector, TlsAcceptor, load_certs_from_pem, load_private_key_from_pem};
```

- `test_node_cert_is_not_ca`: Generate a node cert, load it, verify it's not marked as CA (defense against cert confusion).
- `test_different_cas_produce_different_certs`: Two independent CAs produce certs with different fingerprints.
- `test_ca_cert_contains_proper_pem_markers`: Verify CA cert PEM contains "BEGIN CERTIFICATE" and "END CERTIFICATE".
- `test_node_cert_signed_by_ca`: Generate CA, generate node cert from that CA, verify the node cert PEM is different from the CA PEM.

### Group 5: Connection Auth Edge Cases (4 tests)
```rust
use claudefs_transport::conn_auth::{ConnectionAuthenticator, AuthConfig, AuthLevel, CertificateInfo, AuthResult, RevocationList};
```

- `test_auth_level_tls_only_allows_without_client_cert_checks`: AuthLevel::TlsOnly + valid cert → allowed (no strict checks).
- `test_revocation_list_duplicate_serial_noop`: Adding same serial twice doesn't grow list.
- `test_very_old_cert_rejected_strict_age`: Cert issued 500 days ago with max_cert_age_days=365 → rejected.
- `test_auth_stats_increment_correctly`: Run 5 allowed + 3 denied, verify stats match.

### Group 6: Zero-Copy Pool Security (3 tests)
```rust
use claudefs_transport::zerocopy::{RegionPool, ZeroCopyConfig};
```

- `test_released_region_data_is_zeroed`: Write 0xFF pattern, release, re-acquire, verify all zeros.
- `test_pool_grow_shrink_consistency`: Grow by 5, shrink by 3, verify total/available counts are consistent.
- `test_pool_max_regions_enforced`: Configure max_regions=5, try to acquire 10, verify only 5 succeed.

### Group 7: Batch Auth Security (3 tests)
```rust
use claudefs_repl::batch_auth::{BatchAuthenticator, BatchAuthKey, ReplicationBatchEntry};
```

- `test_batch_auth_wrong_key_fails`: Sign with key A, verify with key B → fail.
- `test_batch_auth_modified_entry_fails`: Sign a batch, modify one entry's data, verify → fail.
- `test_batch_auth_empty_batch_valid`: Sign and verify an empty batch → success.

### Group 8: NFS Auth Boundary Tests (3 tests)
```rust
use claudefs_gateway::auth::{AuthSysCred, AuthCred, SquashPolicy};
use claudefs_gateway::rpc::OpaqueAuth;
```

- `test_auth_sys_max_gids_accepted`: Create cred with exactly 16 GIDs → success.
- `test_auth_sys_over_max_gids_rejected`: Create cred with 17 GIDs → error.
- `test_auth_cred_unknown_flavor_maps_to_nobody`: Unknown flavor → uid=65534, gid=65534.

## Implementation Requirements

1. The file should start with `//! Phase 2 security audit: enhanced tests for nonce security, key lifecycle, TLS, and auth boundaries.`
2. Use `#[cfg(test)]` at the module level.
3. Each test should have a clear doc comment explaining what security property it validates.
4. Use `proptest` where appropriate for randomized testing.
5. Tests should be independent (no shared mutable state between tests).
6. Use descriptive assertion messages that reference the finding ID (e.g., "PHASE2-AUDIT: Nonce collision detected").
7. For the concurrent nonce test, use `std::thread::spawn` (not tokio) since it's a sync operation.

## Constraints

- Do NOT use any `unsafe` code.
- Do NOT import anything not available in the existing Cargo.toml dependencies.
- Keep the file under 500 lines.
- All tests must compile and pass.

## Output

Output ONLY the complete Rust source file content. No markdown code fences. No explanation. Just the Rust code.
